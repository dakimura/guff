//! guff-unused — unused package-level declarations.
//!
//! Simplified port of [`honnef.co/go/tools/unused`](https://pkg.go.dev/honnef.co/go/tools/unused)
//! for single-package analysis.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, GenDecl, Ident, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::facts::generated;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectArena, ObjectData, ObjectId, TypeArena, TypeData};
use guff_types::{pointer_elem, signature_recv};

/// `typString` (honnef `unused/unused.go`): the word that precedes the name in
/// the diagnostic.
fn object_kind(objects: &ObjectArena, types: &TypeArena, obj: ObjectId) -> &'static str {
    match objects.get(obj) {
        ObjectData::Func(_) => "func",
        ObjectData::Var(v) => {
            if v.is_field() {
                "field"
            } else {
                "var"
            }
        }
        ObjectData::Const(_) => "const",
        ObjectData::TypeName(_) => {
            let is_tparam = obj
                .typ(objects)
                .is_some_and(|t| matches!(types.get(t), TypeData::TypeParam(_)));
            if is_tparam {
                "type param"
            } else {
                "type"
            }
        }
        _ => "identifier",
    }
}

fn is_exported(name: &str) -> bool {
    name.chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

/// Receiver type name for `T`, `*T`, or indexed `T[...]` / `*T[...]`.
/// The receiver type as upstream prints it, type arguments included:
/// `holder[T]` for `func (h *holder[T]) run()`, `pair[K, V]` for two.
///
/// honnef names a method by its receiver *type*, and `types` prints a generic
/// receiver with its type parameter list. Dropping it — as using the base
/// identifier alone does — makes the message `func (*holder).run is unused`
/// where golangci-lint says `func (*holder[T]).run is unused`: same finding,
/// same line, different text.
fn recv_type_name(ty: &Expr) -> Option<String> {
    match ty {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::StarExpr(s) => recv_type_name(&s.x),
        Expr::ParenExpr(p) => recv_type_name(&p.x),
        Expr::IndexExpr(i) => {
            let base = recv_type_name(&i.x)?;
            let arg = recv_type_name(&i.index)?;
            Some(format!("{base}[{arg}]"))
        }
        Expr::IndexListExpr(i) => {
            let base = recv_type_name(&i.x)?;
            let args: Vec<String> = i.indices.iter().filter_map(recv_type_name).collect();
            if args.len() != i.indices.len() {
                return Some(base);
            }
            Some(format!("{base}[{}]", args.join(", ")))
        }
        _ => None,
    }
}

fn recv_type_ident(ty: &Expr) -> Option<&Ident> {
    match ty {
        Expr::Ident(id) => Some(id),
        Expr::StarExpr(s) => recv_type_ident(&s.x),
        Expr::IndexExpr(i) => recv_type_ident(&i.x),
        Expr::IndexListExpr(i) => recv_type_ident(&i.x),
        Expr::ParenExpr(p) => recv_type_ident(&p.x),
        _ => None,
    }
}

/// Name of a method object's receiver base type — `streamer` for
/// `func (s *streamer[T]) nextBatch()`. `None` for non-methods.
fn method_recv_base_name(
    types: &TypeArena,
    objects: &ObjectArena,
    obj: ObjectId,
) -> Option<String> {
    let sig = obj.typ(objects)?;
    if !matches!(types.get(sig), TypeData::Signature(_)) {
        return None;
    }
    let recv = signature_recv(types, sig)?;
    let mut t = recv.typ(objects)?;
    if matches!(types.get(t), TypeData::Pointer(_)) {
        t = pointer_elem(types, t);
    }
    match types.get(t) {
        TypeData::Named(n) => Some(n.obj().name(objects).to_string()),
        _ => None,
    }
}

/// Charge every use written under `node` to `owners` — honnef's `by` argument.
///
/// `attributed` records which `*ast.Ident`s were reached, so a use the walk
/// never visits can be treated as a root instead of silently losing its edge.
fn attribute_uses(
    info: &guff_types::api::Info,
    node: guff::walk::NodeRef<'_>,
    owners: &[ObjectId],
    local: &HashSet<ObjectId>,
    edges: &mut HashMap<ObjectId, HashSet<ObjectId>>,
    attributed: &mut HashSet<u32>,
) {
    guff::walk::preorder(node, |n| {
        if let guff::walk::NodeRef::Ident(id) = n {
            if let Some(target) = info.uses.get(&id.id) {
                // Mark it attributed either way: the "unreached ident is a
                // root" fallback must key on whether the *walk* saw it, not on
                // what it pointed at.
                attributed.insert(id.id);
                // Only a package-level declaration of this package can be
                // reached-or-not; an import, a local, a field is decided
                // elsewhere and storing it would grow the graph by an order of
                // magnitude for nothing.
                if local.contains(target) {
                    for owner in owners {
                        edges.entry(*owner).or_default().insert(*target);
                    }
                }
            }
        }
        true
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return Ok(None),
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return Ok(None),
    };

    let fset = pass.fset().clone();
    let pkg_name = pass.pkg().name.as_str();
    let mut candidates = HashSet::new();
    let mut roots = HashSet::new();
    let mut const_groups: Vec<Vec<ObjectId>> = Vec::new();
    let mut method_recv_type: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut method_display: HashMap<ObjectId, String> = HashMap::new();
    let mut iface_method_names: HashSet<String> = HashSet::new();

    // Every method name any interface *type* mentions — including an anonymous
    // one written inline, which is how a package keeps a method private to
    // itself and still calls it:
    //
    //     if setReadSizer, ok := wrec.(interface{ setReadSize(*int) }); ok {
    //
    // Collecting only from `type X interface {…}` declarations missed those, so
    // caddy's `setReadSize` and `tlsNetConn` were reported unused. They were
    // invisible until the ill-typed count dropped and the package was analysed
    // at all (COMPAT-HARDENING §4, 15th session).
    for file in pass.files() {
        guff::walk::preorder_prune(guff::walk::NodeRef::File(file), |n| {
            // A *generic* interface is skipped, and that is upstream's answer
            // rather than a simplification. staticcheck's unused builds a graph
            // edge from a concrete method to the interface method it implements,
            // and for an interface with type parameters it does not: dapr's
            //
            //     type streamer[T differ.Resource] interface { list(…) … }
            //     newResource[componentsapi.Component](…, new(components))
            //
            // leaves `(*components).list` unreachable, and golangci-lint reports
            // all four methods of all ten streamers — forty findings dapr
            // silences with `//nolint:unused`, which guff then called unused
            // directives. The non-generic form still counts as a use, which the
            // fixture pins beside this one.
            if let guff::walk::NodeRef::TypeSpec(spec) = n {
                if spec.type_params.is_some()
                    && matches!(spec.ty, guff::ast::Expr::InterfaceType(_))
                {
                    return false;
                }
            }
            if let guff::walk::NodeRef::InterfaceType(iface) = n {
                for field in &iface.methods.list {
                    for name in &field.names {
                        iface_method_names.insert(name.name.clone());
                    }
                }
            }
            true
        });
    }

    for file in pass.files() {
        if is_generated_at(pass, file.file_start.0 as u32) {
            // `GeneratedIsUsed` (on by default) makes honnef `g.use(obj, nil)`
            // every object declared in a generated file — a root, not an
            // absence. Under reachability the difference is visible: dropping
            // them would strand everything they reference.
            for decl in &file.decls {
                match decl {
                    Decl::FuncDecl(f) => {
                        if let Some(Some(obj)) = info.defs.get(&f.name.id) {
                            roots.insert(*obj);
                        }
                    }
                    Decl::GenDecl(GenDecl { specs, .. }) => {
                        for spec in specs {
                            match spec {
                                Spec::TypeSpec(TypeSpec { name, .. }) => {
                                    if let Some(Some(obj)) = info.defs.get(&name.id) {
                                        roots.insert(*obj);
                                    }
                                }
                                Spec::ValueSpec(ValueSpec { names, .. }) => {
                                    for id in names {
                                        if let Some(Some(obj)) = info.defs.get(&id.id) {
                                            roots.insert(*obj);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(f) => {
                    let Some(Some(obj)) = info.defs.get(&f.name.id) else {
                        continue;
                    };
                    if let Some(recv) = &f.recv {
                        if let Some(field) = recv.list.first() {
                            if let Some(ty) = field.ty.as_ref() {
                                if let Some(type_ident) = recv_type_ident(ty) {
                                    // Receiver type Idents are usually uses, not defs.
                                    let type_obj =
                                        info.uses.get(&type_ident.id).copied().or_else(|| {
                                            info.defs.get(&type_ident.id).and_then(|d| *d)
                                        });
                                    if let Some(type_obj) = type_obj {
                                        method_recv_type.insert(*obj, type_obj);
                                    }
                                    // `(*T).M` for a pointer receiver, `T.M`
                                    // for a value one — upstream only wraps the
                                    // type in parentheses when it printed a `*`
                                    // (unused.go's `newObject`). guff wrapped
                                    // both, which `normalize.py`'s
                                    // `_UNUSED_METHOD_QUAL` was erasing along
                                    // with the whole qualifier.
                                    let ptr = matches!(ty, Expr::StarExpr(_));
                                    let printed = recv_type_name(ty)
                                        .unwrap_or_else(|| type_ident.name.clone());
                                    let qual = if ptr {
                                        format!("(*{printed}).")
                                    } else {
                                        format!("{printed}.")
                                    };
                                    method_display.insert(*obj, format!("{qual}{}", f.name.name));
                                }
                            }
                        }
                        if f.name.name == "_" || is_exported(&f.name.name) {
                            roots.insert(*obj);
                        } else {
                            candidates.insert(*obj);
                        }
                        continue;
                    }
                    if f.name.name == "_"
                        || f.name.name == "init"
                        || is_exported(&f.name.name)
                    {
                        roots.insert(*obj);
                        continue;
                    }
                    if pkg_name == "main" && f.name.name == "main" {
                        roots.insert(*obj);
                        continue;
                    }
                    candidates.insert(*obj);
                }
                Decl::GenDecl(GenDecl { tok, specs, .. }) => {
                    let kind = matches!(tok, Some(Token::VAR | Token::CONST | Token::TYPE));
                    if !kind {
                        continue;
                    }
                    // honnef groups const specs by `astutil.GroupSpecs`: a spec
                    // joins the previous group only when it starts on the line
                    // right after the previous spec ends. A doc comment or a
                    // blank line starts a new group, so
                    // `const ( bucketCount = 256; /*doc*/ Exported = "…" )`
                    // does not let the exported member keep `bucketCount` alive
                    // (vault `helper/storagepacker`).
                    let mut decl_group: Vec<ObjectId> = Vec::new();
                    let mut prev_end_line: Option<i64> = None;
                    for spec in specs {
                        if *tok == Some(Token::CONST) {
                            let start_line = fset.position_for(spec.pos(), false).line;
                            let adjacent = prev_end_line == Some(start_line - 1);
                            if !adjacent && decl_group.len() > 1 {
                                const_groups.push(std::mem::take(&mut decl_group));
                            } else if !adjacent {
                                decl_group.clear();
                            }
                            prev_end_line = Some(fset.position_for(spec.end(), false).line);
                        }
                        match spec {
                            Spec::TypeSpec(TypeSpec { name, ty, .. }) => {
                                let Some(Some(obj)) = info.defs.get(&name.id) else {
                                    continue;
                                };
                                if name.name == "_" || is_exported(&name.name) {
                                    roots.insert(*obj);
                                } else {
                                    candidates.insert(*obj);
                                }
                                // Named interfaces are picked up by the
                                // whole-file sweep below; nothing extra to do
                                // here.
                            }
                            Spec::ValueSpec(ValueSpec { names, .. }) => {
                                for id in names {
                                    let Some(Some(obj)) = info.defs.get(&id.id) else {
                                        continue;
                                    };
                                    // honnef (9.9): an object named the blank
                                    // identifier is used — `g.use(obj, by)`.
                                    // Under reachability that has to be a root:
                                    // restic's
                                    //
                                    //     var _ = initDebug()
                                    //
                                    // is the only thing keeping six functions in
                                    // `internal/debug` alive, and skipping the
                                    // blank name outright dropped that edge.
                                    if id.name == "_" {
                                        roots.insert(*obj);
                                        continue;
                                    }
                                    if is_exported(&id.name) {
                                        roots.insert(*obj);
                                    } else {
                                        candidates.insert(*obj);
                                    }
                                    // honnef unused (10.1): const groups include
                                    // every non-blank name. If any member is
                                    // used, the whole group is marked used.
                                    if *tok == Some(Token::CONST) {
                                        decl_group.push(*obj);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if *tok == Some(Token::CONST) && decl_group.len() > 1 {
                        const_groups.push(decl_group);
                    }
                }
                _ => {}
            }
        }
    }

    // honnef's graph is *reachability from a root* (`unused.go`'s `color`),
    // not a flat "is this referenced anywhere" count: a reference written
    // inside a dead function does not keep its target alive. dapr's
    // `recompileAll` is called only by `update` and `delete`, and nothing calls
    // those, so upstream reports all three; guff counted references, reported
    // only two, and then called the `//nolint:unused` over the third an unused
    // directive.
    //
    // Every use is attributed to the top-level declaration it sits in — that is
    // honnef's `by` argument to `g.use`. An `*ast.Ident` the walk does not
    // reach falls back to the old unconditional treatment: a lost edge would be
    // a false positive, while a spurious root only costs a missed report.
    let mut edges: HashMap<ObjectId, HashSet<ObjectId>> = HashMap::new();
    let mut attributed: HashSet<u32> = HashSet::new();
    let local: HashSet<ObjectId> = candidates.union(&roots).copied().collect();
    for file in pass.files() {
        // honnef sees the objects of a generated file like any others but marks
        // them used (`GeneratedIsUsed`, on by default), so what they reference
        // stays alive. Skipping the file outright, as the candidate sweep below
        // does, would strand those edges.
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(f) => {
                    let Some(Some(obj)) = info.defs.get(&f.name.id) else {
                        continue;
                    };
                    attribute_uses(
                        info,
                        guff::walk::NodeRef::FuncDecl(f),
                        &[*obj],
                        &local,
                        &mut edges,
                        &mut attributed,
                    );
                }
                Decl::GenDecl(GenDecl { tok, specs, .. }) => {
                    if !matches!(tok, Some(Token::VAR | Token::CONST | Token::TYPE)) {
                        continue;
                    }
                    for spec in specs {
                        match spec {
                            Spec::TypeSpec(ts) => {
                                let Some(Some(obj)) = info.defs.get(&ts.name.id) else {
                                    continue;
                                };
                                attribute_uses(
                                    info,
                                    guff::walk::NodeRef::TypeSpec(ts),
                                    &[*obj],
                                    &local,
                                    &mut edges,
                                    &mut attributed,
                                );
                            }
                            Spec::ValueSpec(vs) => {
                                // honnef pairs `names[i]` with `values[i]`;
                                // charging the whole spec to every name it
                                // declares only ever keeps more alive.
                                let owners: Vec<ObjectId> = vs
                                    .names
                                    .iter()
                                    .filter_map(|id| info.defs.get(&id.id).copied().flatten())
                                    .collect();
                                if owners.is_empty() {
                                    continue;
                                }
                                attribute_uses(
                                    info,
                                    guff::walk::NodeRef::ValueSpec(vs),
                                    &owners,
                                    &local,
                                    &mut edges,
                                    &mut attributed,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Calls through an *instantiated* generic receiver (`streamer[T].nextBatch`)
    // record the substituted method copy in `Uses`, which is a different
    // ObjectId from the declaration. `Func` has no `Origin()` to map it back
    // (R18 DEFERRED), so remember (receiver type name, method name) too.
    let mut used_methods: HashSet<(String, String)> = HashSet::new();
    // Names that reachability can never retract: everything used that is not a
    // package-level declaration of this package — imported objects, locals,
    // fields. The receiver-type rules below match on names, and these were part
    // of the old flat set.
    let mut foreign_used_names: HashSet<String> = HashSet::new();
    // `used` starts empty and the roots go through the queue like any other
    // node: seeding it with them would make the very first `used.insert` say
    // "already there" and skip their outgoing edges.
    let mut used: HashSet<ObjectId> = HashSet::new();
    let mut queue: Vec<ObjectId> = roots.iter().copied().collect();
    for (id, obj) in &info.uses {
        if !attributed.contains(id) {
            queue.push(*obj);
        }
        // Both name-keyed sets take only uses that are *not* one of this
        // package's own declarations. A plain method call records the
        // declaration's own ObjectId and the reachability edge above already
        // carries it; letting the name rule fire there too would put every
        // called method back on the root set, which is what kept dapr's
        // `recompileAll` alive.
        if !candidates.contains(obj) {
            foreign_used_names.insert(obj.name(&artifacts.objects).to_string());
            if let Some(recv) = method_recv_base_name(&artifacts.types, &artifacts.objects, *obj) {
                used_methods.insert((recv, obj.name(&artifacts.objects).to_string()));
            }
        }
    }

    // Reachability, then the two name-keyed rules, to a fixed point: an
    // interface-satisfying method that becomes reachable can itself keep more
    // of the package alive.
    let mut used_type_names: HashSet<String> = HashSet::new();
    loop {
        while let Some(obj) = queue.pop() {
            if !used.insert(obj) {
                continue;
            }
            used_type_names.insert(obj.name(&artifacts.objects).to_string());
            if let Some(next) = edges.get(&obj) {
                queue.extend(next.iter().copied());
            }
        }

        // Both rules below queue only objects that are not used yet, so an
        // empty queue here *is* the fixed point. Re-queueing something already
        // reached would spin forever: the drain above would add nothing, the
        // rules would queue it again, and the loop would never see an empty
        // queue. (It did — two `unused` fixtures ran for minutes at 200% CPU.)
        for group in &const_groups {
            if group.iter().any(|obj| used.contains(obj)) {
                queue.extend(group.iter().copied().filter(|obj| !used.contains(obj)));
            }
        }

        // Methods that implement a package interface are used when their
        // receiver type is used (even if never called by name). Compare by type
        // *name* so hybrid typecheck ObjectId identity mismatches don't miss the
        // link.
        for (method, recv_ty) in &method_recv_type {
            if used.contains(method) || !candidates.contains(method) {
                continue;
            }
            let recv_name = recv_ty.name(&artifacts.objects);
            let known =
                used_type_names.contains(recv_name) || foreign_used_names.contains(recv_name);
            let name = method.name(&artifacts.objects);
            if known && iface_method_names.contains(name) {
                queue.push(*method);
                continue;
            }
            let key = (recv_name.to_string(), name.to_string());
            if used_methods.contains(&key) {
                queue.push(*method);
            }
        }

        if queue.is_empty() {
            break;
        }
    }

    let ignores = collect_lint_ignores(pass);

    let mut pending = Vec::new();
    for obj in candidates {
        if used.contains(&obj) {
            continue;
        }
        let name = obj.name(&artifacts.objects);
        let pos = obj.pos(&artifacts.objects);
        if ignores.covers(&fset, pos) {
            continue;
        }
        let display = method_display
            .get(&obj)
            .cloned()
            .unwrap_or_else(|| name.to_string());
        // `fmt.Sprintf("%s %s is unused", uo.obj.Kind, uo.obj.Name)`
        // (honnef `lintcmd/lint.go`). guff omitted the kind entirely, which
        // `normalize.py` stripped off upstream's side to match — one of the six
        // rows COMPAT-HARDENING §5 carried as "unexamined".
        let kind = object_kind(&artifacts.objects, &artifacts.types, obj);
        pending.push((pos, format!("{kind} {display} is unused")));
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

/// `//lint:ignore U1000 …` and `//lint:file-ignore U1000 …`, honnef's own
/// suppression syntax: an object on an ignored line is *used*
/// (`unused/unused.go`, "all objects annotated with a //lint:ignore U1000 are
/// considered used").
///
/// Upstream keys on the line of the node the comment is attached to
/// (`ast.NewCommentMap`) — for a trailing comment that is the comment's own
/// line, for a doc comment the declaration below it — so both are recorded.
/// rclone writes one of each in `cmd/serve/docker`, on a `var` and on a `func`
/// that only the linux build uses.
///
/// The shared load parses without `PARSE_COMMENTS`, so the comments have to be
/// read back out of the source; line numbers are the same in either parse, so
/// only the file name has to line up.
#[derive(Default)]
struct LintIgnores {
    lines: HashSet<(String, i64)>,
    files: HashSet<String>,
}

impl LintIgnores {
    fn is_empty(&self) -> bool {
        self.lines.is_empty() && self.files.is_empty()
    }

    fn covers(&self, fset: &guff::position::FileSet, pos: u32) -> bool {
        if self.is_empty() {
            return false;
        }
        let p = fset.position_for(guff::position::Pos(pos as i64), false);
        let base = base_name(&p.filename);
        self.files.contains(&base) || self.lines.contains(&(base, p.line))
    }
}

fn base_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

fn collect_lint_ignores(pass: &Pass<'_>) -> LintIgnores {
    use guff::parser::{parse_file, PARSE_COMMENTS};
    use guff::position::FileSet;

    let mut out = LintIgnores::default();
    for (index, path) in pass.pkg().compiled_go_files.iter().enumerate() {
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // The type-checker already read this file; re-opening it makes the
        // kernel do the work twice.
        let owned;
        let src: &[u8] = match pass.pkg().source_bytes(index) {
            Some(b) => b,
            None => match std::fs::read(path) {
                Ok(b) => {
                    owned = b;
                    &owned
                }
                Err(_) => continue,
            },
        };
        // Cheap reject before the reparse: most files have no directive.
        if !src.windows(7).any(|w| w == b"//lint:") {
            continue;
        }
        let rfset = FileSet::new();
        let Ok(rfile) = parse_file(&rfset, name, src, PARSE_COMMENTS) else {
            continue;
        };
        for group in &rfile.comments {
            for comment in &group.list {
                let Some(rest) = comment.text.strip_prefix("//lint:") else {
                    continue;
                };
                let mut fields = rest.split(' ');
                let command = fields.next().unwrap_or("");
                let checks = fields.next().unwrap_or("");
                if !checks.split(',').any(|c| c == "U1000") {
                    continue;
                }
                match command {
                    "file-ignore" => {
                        out.files.insert(name.to_string());
                    }
                    "ignore" => {
                        let at = rfset.position_for(comment.pos(), false);
                        out.lines.insert((name.to_string(), at.line));
                        // A doc comment belongs to the declaration below it; a
                        // trailing one belongs to the code already on its line,
                        // and must not reach down to the next declaration.
                        if !is_trailing(src, at.offset) {
                            if let Some(line) = next_decl_line(&rfile, &rfset, group.end()) {
                                out.lines.insert((name.to_string(), line));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    out
}

/// Whether anything but whitespace precedes `offset` on its line.
fn is_trailing(src: &[u8], offset: i64) -> bool {
    let mut i = offset as usize;
    while i > 0 {
        i -= 1;
        match src.get(i) {
            Some(b'\n') => return false,
            Some(c) if c.is_ascii_whitespace() => {}
            Some(_) => return true,
            None => return false,
        }
    }
    false
}

/// Line of the first declaration or spec that starts at or after `pos`.
fn next_decl_line(
    file: &guff::ast::File,
    fset: &guff::position::FileSet,
    pos: guff::position::Pos,
) -> Option<i64> {
    let mut best: Option<guff::position::Pos> = None;
    let mut consider = |p: guff::position::Pos| {
        if p.0 >= pos.0 && best.is_none_or(|b| p.0 < b.0) {
            best = Some(p);
        }
    };
    for decl in &file.decls {
        consider(decl.pos());
        if let Decl::GenDecl(GenDecl { specs, .. }) = decl {
            for spec in specs {
                consider(spec.pos());
            }
        }
    }
    best.map(|p| fset.position_for(p, false).line)
}

fn make_analyzer(run_despite_errors: bool) -> Analyzer {
    Analyzer {
        name: "unused",
        doc: "check for unused package-level declarations",
        url: "https://pkg.go.dev/honnef.co/go/tools/unused",
        run: run as RunFn,
        run_despite_errors,
        requires: vec![generated::analyzer()],
        fact_types: vec![],
    }
}

/// Default: skip ill-typed packages (matches honnef when types fail, and
/// avoids FP + wall cost on guff false-`ill_typed` packages).
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(false))
}

/// When `nolintlint` is also enabled, run on ill-typed packages so partial
/// type info can still mark live refs (restic `sys` field) and nolintlint
/// can report truly unused `//nolint:unused` directives.
pub fn analyzer_despite_errors() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(true))
}

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![analyzer()]
}

pub fn analyzers_despite_errors() -> Vec<&'static Analyzer> {
    vec![analyzer_despite_errors()]
}
