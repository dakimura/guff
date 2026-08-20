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
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
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
                                    let type_obj = info
                                        .uses
                                        .get(&type_ident.id)
                                        .copied()
                                        .or_else(|| {
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
                                    method_display
                                        .insert(*obj, format!("{qual}{}", f.name.name));
                                }
                            }
                        }
                        if is_exported(&f.name.name) {
                            roots.insert(*obj);
                        } else {
                            candidates.insert(*obj);
                        }
                        continue;
                    }
                    if f.name.name == "init" || is_exported(&f.name.name) {
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
                                if is_exported(&name.name) {
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
                                    if id.name == "_" {
                                        continue;
                                    }
                                    let Some(Some(obj)) = info.defs.get(&id.id) else {
                                        continue;
                                    };
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

    let mut used = roots;
    // Calls through an *instantiated* generic receiver (`streamer[T].nextBatch`)
    // record the substituted method copy in `Uses`, which is a different
    // ObjectId from the declaration. `Func` has no `Origin()` to map it back
    // (R18 DEFERRED), so remember (receiver type name, method name) too.
    let mut used_methods: HashSet<(String, String)> = HashSet::new();
    for obj in info.uses.values() {
        used.insert(*obj);
        if let Some(recv) = method_recv_base_name(&artifacts.types, &artifacts.objects, *obj) {
            used_methods.insert((recv, obj.name(&artifacts.objects).to_string()));
        }
    }

    for group in const_groups {
        if group.iter().any(|obj| used.contains(obj)) {
            for obj in group {
                used.insert(obj);
            }
        }
    }

    // Methods that implement a package interface are used when their receiver
    // type is used (even if never called by name). Compare by type *name* so
    // hybrid typecheck ObjectId identity mismatches don't miss the link.
    let used_type_names: HashSet<String> = used
        .iter()
        .map(|obj| obj.name(&artifacts.objects).to_string())
        .collect();
    for (method, recv_ty) in &method_recv_type {
        if !candidates.contains(method) {
            continue;
        }
        let recv_name = recv_ty.name(&artifacts.objects);
        if !used_type_names.contains(recv_name) {
            continue;
        }
        let name = method.name(&artifacts.objects);
        if iface_method_names.contains(name) {
            used.insert(*method);
        }
    }

    // Second pass over the same map, this time matching the instantiated-method
    // uses collected above.
    for (method, recv_ty) in &method_recv_type {
        if !candidates.contains(method) {
            continue;
        }
        let key = (
            recv_ty.name(&artifacts.objects).to_string(),
            method.name(&artifacts.objects).to_string(),
        );
        if used_methods.contains(&key) {
            used.insert(*method);
        }
    }

    let mut pending = Vec::new();
    for obj in candidates {
        if used.contains(&obj) {
            continue;
        }
        let name = obj.name(&artifacts.objects);
        let pos = obj.pos(&artifacts.objects);
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
