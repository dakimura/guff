//! `inline` — suggest inlining of constants marked `//go:fix inline`, and
//! report call sites of known generic `//go:fix inline` funcs that the
//! upstream inliner cannot yet specialize (type-parameter inference).
//!
//! Port of the const-inlining subset of
//! `golang.org/x/tools/go/analysis/passes/inline` (the part golangci surfaces
//! as `Constant reflect.Ptr should be inlined`), plus the
//! `cannot inline: type parameter inference is not yet supported` diagnostic
//! for `golang.org/x/exp/{maps,slices}` go:fix generics (consul), and the
//! go-version gate for `io/ioutil` `//go:fix inline` wrappers (#75726).
//!
//! Full function/alias inlining is omitted. Stdlib / exp packages are often
//! loaded from export data (no source), so known inlinables are hardcoded in
//! addition to discovering `//go:fix inline` consts in packages that have
//! syntax.
//!
//! Package load uses `Mode::NONE`, which drops lead comments after the package
//! clause, so local `//go:fix` discovery re-parses with `PARSE_COMMENTS`.

use std::collections::{HashMap, HashSet};

/// A finding, and the edits that inline it when there are any. The two
/// `cannot inline` arms below carry an empty list: upstream reports those
/// without a fix because type-parameter inference is not implemented.
///
/// Constants need one edit; a type alias may need a second one that adds the
/// import its right-hand side names.
type Pending = Vec<(u32, String, Vec<TextEdit>)>;
use std::fs;
use std::sync::OnceLock;

use guff::ast::{CallExpr, CommentGroup, Decl, Expr, ExprStmt, GenDecl, Spec, ValueSpec};
use guff::parse_directive;
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{
    call_name, effective_file_go_version, object_pkg_path, toolchain_go_version, version_compare,
};
use guff_analysis::passes::inspect;
use guff_analysis::refactor;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::{ObjectData, ObjectId};
use guff_types::TypeId;
use guff_types::scope::lookup as scope_lookup;

use crate::expreq::unparen;

/// Stdlib consts with `//go:fix inline` that we cannot discover from export data.
/// Stdlib consts carrying `//go:fix inline`, and what they inline to.
///
/// Upstream reads both the set *and* the target from the declaration. guff
/// cannot: `reflect`'s source is not in export data, so the pair is written
/// down here — which is what the predicate this replaces already did for the
/// set half. The exposure is unchanged and pre-existing: `compat/drift.py`
/// watches finding sets, the linter inventory and config acceptance, not
/// x/tools' inlinable set, so a constant added upstream is invisible until some
/// corpus file uses it.
fn known_stdlib_inline_target(pkg_path: &str, name: &str) -> Option<&'static str> {
    match (pkg_path, name) {
        ("reflect", "Ptr") => Some("reflect.Pointer"),
        _ => None,
    }
}

/// Generic `//go:fix inline` funcs in `golang.org/x/exp/{maps,slices}`.
/// Upstream reports `cannot inline: type parameter inference is not yet
/// supported` at each call; we mirror that without porting inference.
fn is_known_generic_gofix_inline(pkg_path: &str, name: &str) -> bool {
    match pkg_path {
        "golang.org/x/exp/maps" => matches!(
            name,
            "Equal" | "EqualFunc" | "Clear" | "Clone" | "Copy" | "DeleteFunc"
        ),
        "golang.org/x/exp/slices" => matches!(
            name,
            "Sort"
                | "SortFunc"
                | "SortStableFunc"
                | "IsSorted"
                | "IsSortedFunc"
                | "Min"
                | "MinFunc"
                | "Max"
                | "MaxFunc"
                | "BinarySearch"
                | "BinarySearchFunc"
                | "Equal"
                | "EqualFunc"
                | "Compare"
                | "CompareFunc"
                | "Index"
                | "IndexFunc"
                | "Contains"
                | "ContainsFunc"
                | "Insert"
                | "Delete"
                | "DeleteFunc"
                | "Replace"
                | "Clone"
                | "Compact"
                | "CompactFunc"
                | "Grow"
                | "Clip"
                | "Reverse"
        ),
        _ => false,
    }
}

fn has_fix_inline(doc: &Option<CommentGroup>) -> bool {
    let Some(doc) = doc else {
        return false;
    };
    for c in &doc.list {
        if let Some(d) = parse_directive(c.slash, &c.text) {
            if d.tool == "go" && d.name == "fix" && d.args.trim() == "inline" {
                return true;
            }
        }
    }
    false
}

fn rhs_is_named_const(val: &Expr) -> bool {
    matches!(unparen(val), Expr::Ident(_) | Expr::SelectorExpr(_))
}

/// The inline target, as upstream's `incon.RHSName` spells it.
///
/// x/tools reads it from the `//go:fix inline` declaration rather than any
/// table (`passes/inline/inline.go:559`), which is why the local arm needs no
/// new data: `rhs_is_named_const` above already looks at this expression.
fn rhs_target(val: &Expr) -> Option<String> {
    match unparen(val) {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => match unparen(&sel.x) {
            Expr::Ident(pkg) => Some(format!("{}.{}", pkg.name, sel.sel.name)),
            _ => None,
        },
        _ => None,
    }
}

fn rhs_is_iota(val: &Expr) -> bool {
    matches!(unparen(val), Expr::Ident(id) if id.name == "iota")
}

/// Collect LHS names of consts marked `//go:fix inline` in a reparsed file.
fn go_fix_const_names(file: &guff::ast::File) -> Vec<(String, String)> {
    let mut names = Vec::new();
    for decl in &file.decls {
        let Decl::GenDecl(GenDecl {
            doc,
            tok: Some(Token::CONST),
            specs,
            ..
        }) = decl
        else {
            continue;
        };
        let decl_inline = has_fix_inline(doc);
        for spec in specs {
            let Spec::ValueSpec(ValueSpec {
                doc: spec_doc,
                names: spec_names,
                values,
                ..
            }) = spec
            else {
                continue;
            };
            if !decl_inline && !has_fix_inline(spec_doc) {
                continue;
            }
            for (i, name) in spec_names.iter().enumerate() {
                if i >= values.len() {
                    break;
                }
                if rhs_is_iota(&values[i]) || !rhs_is_named_const(&values[i]) {
                    continue;
                }
                let Some(target) = rhs_target(&values[i]) else {
                    continue;
                };
                names.push((name.name.clone(), target));
            }
        }
    }
    names
}

fn collect_inlinable_consts(pass: &Pass<'_>) -> HashMap<ObjectId, String> {
    let mut out = HashMap::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    let Some(type_pkg) = pass.type_pkg() else {
        return out;
    };
    let scope = artifacts.packages.get(type_pkg).scope();

    for (i, _) in pass.files().iter().enumerate() {
        let path = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .cloned()
            .or_else(|| pass.pkg().go_files.get(i).cloned());
        let Some(path) = path else {
            continue;
        };
        // Type-checking kept these bytes; re-reading them cost one `open` per
        // file of every package (see `buildtag::check_go_file`).
        let owned;
        let src: &[u8] = match pass.pkg().source_bytes(i) {
            Some(bytes) => bytes,
            None => match fs::read(&path) {
                Ok(read) => {
                    owned = read;
                    &owned
                }
                Err(_) => continue,
            },
        };
        // Cheap filter: almost no files carry `//go:fix inline`; avoid a full
        // PARSE_COMMENTS reparse on the common path.
        if memchr::memmem::find(src, b"go:fix inline").is_none() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let re_fset = FileSet::new();
        let Ok(parsed) = parse_file(&re_fset, name, src, PARSE_COMMENTS) else {
            continue;
        };
        for (const_name, target) in go_fix_const_names(&parsed) {
            let Some(obj) = scope_lookup(&artifacts.scopes, scope, &const_name) else {
                continue;
            };
            if matches!(artifacts.objects.get(obj), ObjectData::Const(_)) {
                out.insert(obj, target);
            }
        }
    }
    out
}

fn package_has_go_fix_inline(pass: &Pass<'_>) -> bool {
    // Retained bytes first; only files without them (and the `go_files` the
    // compiled list does not cover) are read. `source_files` is parallel to
    // `syntax`, which holds only the files that parsed — so an index into
    // `compiled_go_files` addresses it correctly only when nothing was dropped.
    let aligned = pass.pkg().source_files.len() == pass.pkg().compiled_go_files.len();
    for (i, path) in pass
        .pkg()
        .compiled_go_files
        .iter()
        .chain(pass.pkg().go_files.iter())
        .enumerate()
    {
        let owned;
        let src: &[u8] = match pass
            .pkg()
            .source_bytes(i)
            .filter(|_| aligned && i < pass.pkg().compiled_go_files.len())
        {
            Some(bytes) => bytes,
            None => match fs::read(path) {
                Ok(read) => {
                    owned = read;
                    &owned
                }
                Err(_) => continue,
            },
        };
        if memchr::memmem::find(src, b"go:fix inline").is_some() {
            return true;
        }
    }
    false
}

/// The text `obj` inlines to, or `None` when it is not inlinable.
fn inline_target(
    pass: &Pass<'_>,
    obj: ObjectId,
    local: &mut Option<HashMap<ObjectId, String>>,
) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj), ObjectData::Const(_)) {
        return None;
    }
    let name = obj.name(&artifacts.objects);
    if let Some(pkg_path) = object_pkg_path(pass, obj) {
        if let Some(target) = known_stdlib_inline_target(&pkg_path, name) {
            return Some(target.to_string());
        }
    }
    // Local `//go:fix inline` — same PackageId as the package under analysis
    // (path strings are often empty for the current package).
    let obj_pkg = obj.pkg(&artifacts.objects)?; // universe / unpackaged
    let type_pkg = pass.type_pkg()?;
    if obj_pkg != type_pkg {
        return None;
    }
    let set = local.get_or_insert_with(|| {
        if !package_has_go_fix_inline(pass) {
            return HashMap::new();
        }
        collect_inlinable_consts(pass)
    });
    set.get(&obj).cloned()
}

fn format_expr_name(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", format_expr_name(&sel.x), sel.sel.name),
        _ => "?".into(),
    }
}

/// True when this package directly imports an x/exp package that carries
/// generic `//go:fix inline` funcs. Prometheus (and most targets) do not —
/// skip CallExpr resolution entirely in that common case.
fn package_imports_exp_gofix(pass: &Pass<'_>) -> bool {
    pass.pkg().imports.keys().any(|p| {
        matches!(
            p.as_str(),
            "golang.org/x/exp/maps" | "golang.org/x/exp/slices"
        )
    })
}

fn package_imports_ioutil(pass: &Pass<'_>) -> bool {
    pass.pkg().imports.contains_key("io/ioutil")
}

/// `io/ioutil` funcs annotated `//go:fix inline` in GOROOT (go1.16+/1.17+).
fn is_known_ioutil_gofix_inline(name: &str) -> bool {
    matches!(
        name,
        "ReadAll" | "ReadFile" | "WriteFile" | "NopCloser" | "TempFile" | "TempDir"
    )
}

fn check_exp_gofix_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Pending) {
    let Some(name) = call_name(pass, &call.fun) else {
        return;
    };
    // call_name → "golang.org/x/exp/maps.Clone"
    let (pkg, func) = match name.rsplit_once('.') {
        Some((p, f)) => (p, f),
        None => return,
    };
    if is_known_generic_gofix_inline(pkg, func) {
        pending.push((
            call.lparen.0 as u32,
            "cannot inline: type parameter inference is not yet supported".into(),
            Vec::new(),
        ));
    }
}

/// Report when inlining an `io/ioutil` go:fix wrapper would pull a newer
/// dialect into an older caller file (upstream #75726 stopgap).
///
/// Skips call-as-statement sites (`ioutil.WriteFile(...);` with discarded
/// results): golangci's inliner does not emit the version diagnostic there
/// (vault `pkcs7/sign_test.go`), while assigned calls are reported.
fn check_ioutil_go_version(
    pass: &Pass<'_>,
    call: &CallExpr,
    stmt_calls: &HashSet<i64>,
    pending: &mut Pending,
) {
    if stmt_calls.contains(&call.lparen.0) {
        return;
    }
    let fun = unparen(&call.fun);
    let Expr::SelectorExpr(sel) = fun else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
        return;
    };
    let Some(pkg_path) = object_pkg_path(pass, obj) else {
        return;
    };
    if pkg_path != "io/ioutil" {
        return;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let name = obj.name(&artifacts.objects);
    if !is_known_ioutil_gofix_inline(name) {
        return;
    }
    let pos = call.lparen.0 as u32;
    let caller = effective_file_go_version(pass, pos);
    let callee = toolchain_go_version();
    if caller.is_empty() || callee.is_empty() {
        return;
    }
    // versions.Before(caller, callee)
    if version_compare(&caller, &callee) >= 0 {
        return;
    }
    let display = format_expr_name(&Expr::SelectorExpr(sel.clone()));
    pending.push((
        pos,
        format!("cannot inline call to {display} (declared using {callee}) into a file using {caller}"),
        Vec::new(),
    ));
}

/// Type aliases carrying `//go:fix inline` that guff cannot discover from
/// export data, keyed by `(package path, name)`.
///
/// Upstream learns this from a `goFixInlineAliasFact` exported by the package
/// that declares the alias; guff only analyses the packages being linted, so a
/// dependency's directive leaves no fact behind. Unlike the constant table
/// above this records only *that* the alias is inlinable — the replacement
/// text is rendered from the alias's right-hand side in the type arena, which
/// export data does carry.
///
/// One entry, because there is exactly one such alias in the standard library
/// and `golang.org/x/exp` combined: `constraints.Ordered = cmp.Ordered`,
/// redundant since Go 1.21 introduced `cmp.Ordered`.
fn is_known_inlinable_alias(pkg_path: &str, name: &str) -> bool {
    matches!((pkg_path, name), ("golang.org/x/exp/constraints", "Ordered"))
}

/// Names of type aliases marked `//go:fix inline` in a reparsed file.
/// (Go: the `*ast.TypeSpec` arm of `gofixdirective.Find`.)
fn go_fix_alias_names(file: &guff::ast::File) -> Vec<String> {
    let mut names = Vec::new();
    for decl in &file.decls {
        let Decl::GenDecl(GenDecl {
            doc,
            tok: Some(Token::TYPE),
            specs,
            ..
        }) = decl
        else {
            continue;
        };
        let decl_inline = has_fix_inline(doc);
        for spec in specs {
            let Spec::TypeSpec(ts) = spec else { continue };
            // Only an alias — `type A = B`, not `type A B`.
            if !ts.assign.is_valid() {
                continue;
            }
            if !decl_inline && !has_fix_inline(&ts.doc) {
                continue;
            }
            names.push(ts.name.name.clone());
        }
    }
    names
}

/// The alias `TypeName`s in the package under analysis that carry the
/// directive.
fn collect_inlinable_aliases(pass: &Pass<'_>) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    let Some(type_pkg) = pass.type_pkg() else {
        return out;
    };
    let scope = artifacts.packages.get(type_pkg).scope();
    for (i, _) in pass.files().iter().enumerate() {
        let path = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .cloned()
            .or_else(|| pass.pkg().go_files.get(i).cloned());
        let Some(path) = path else { continue };
        let owned;
        let src: &[u8] = match pass.pkg().source_bytes(i) {
            Some(bytes) => bytes,
            None => match fs::read(&path) {
                Ok(read) => {
                    owned = read;
                    &owned
                }
                Err(_) => continue,
            },
        };
        if memchr::memmem::find(src, b"go:fix inline").is_none() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let re_fset = FileSet::new();
        let Ok(parsed) = parse_file(&re_fset, name, src, PARSE_COMMENTS) else {
            continue;
        };
        for alias_name in go_fix_alias_names(&parsed) {
            let Some(obj) = scope_lookup(&artifacts.scopes, scope, &alias_name) else {
                continue;
            };
            if matches!(artifacts.objects.get(obj), ObjectData::TypeName(_)) {
                out.insert(obj);
            }
        }
    }
    out
}

/// Whether `obj` is a type alias that should be inlined at its uses.
///
/// # Where the discovery stops
///
/// Like `printf`'s wrapper induction and `ctrlflow`'s no-return induction,
/// this runs *inside* a package. Upstream exports a `goFixInlineAliasFact`
/// per package, so an alias declared in a **sibling** package of the same
/// module is inlinable at its uses there; here it is not, and advertising
/// `fact_types` on `inline` would schedule it across every transitive import
/// for facts that could not be produced anyway (the reason `printf_wrappers`
/// gives for the same choice). Measured, as a golangci-only report:
/// `type H[T b.Ord] []T` where `b.Ord` is a `//go:fix inline` alias in a
/// sibling package. The constant arm above already has this same boundary.
fn inlinable_alias(
    pass: &Pass<'_>,
    obj: ObjectId,
    local: &mut Option<HashSet<ObjectId>>,
) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if !matches!(artifacts.objects.get(obj), ObjectData::TypeName(_)) {
        return false;
    }
    let name = obj.name(&artifacts.objects);
    if let Some(pkg_path) = object_pkg_path(pass, obj) {
        if is_known_inlinable_alias(&pkg_path, name) {
            return true;
        }
    }
    let Some(obj_pkg) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let Some(type_pkg) = pass.type_pkg() else {
        return false;
    };
    if obj_pkg != type_pkg {
        return false;
    }
    let set = local.get_or_insert_with(|| {
        if !package_has_go_fix_inline(pass) {
            return HashSet::new();
        }
        collect_inlinable_aliases(pass)
    });
    set.contains(&obj)
}

/// Whether package `from` is allowed to import package `to` under the
/// `internal/` visibility rule. (Go: `packagepath.CanImport`.)
fn can_import(from: &str, to: &str) -> bool {
    if to == "internal" || to.starts_with("internal/") {
        // Only std packages may import `internal/...`, and we cannot reliably
        // know whether we are in std, so upstream uses a first-segment
        // heuristic.
        let first = from.split('/').next().unwrap_or("");
        if first.contains('.') {
            return false; // example.com/foo is not std
        }
        if first == "testdata" {
            return false;
        }
    }
    if let Some(stem) = to.strip_suffix("/internal") {
        return from.starts_with(stem);
    }
    if let Some(i) = to.rfind("/internal/") {
        return from.starts_with(&to[..i]);
    }
    true
}

/// The `TypeName`s of the named, alias and type-parameter types within `t`
/// (including `t` itself). The same name may appear more than once.
/// (Go: `typenames`.)
fn typenames(arena: &guff_types::TypeArena, t: TypeId, out: &mut Vec<ObjectId>) {
    typenames_seen(arena, t, out, &mut HashSet::new());
}

fn typenames_seen(
    arena: &guff_types::TypeArena,
    t: TypeId,
    out: &mut Vec<ObjectId>,
    seen: &mut HashSet<TypeId>,
) {
    if !seen.insert(t) {
        return;
    }
    use guff_types::arena::TypeData;
    match arena.get(t) {
        TypeData::Named(n) => {
            out.push(n.obj());
            for &a in guff_types::named_type_args(arena, t)
                .map(|l| l.list())
                .unwrap_or(&[])
            {
                typenames_seen(arena, a, out, seen);
            }
        }
        TypeData::Alias(al) => {
            out.push(al.obj());
            let targs: Vec<TypeId> = al
                .type_args()
                .map(|l| l.list().to_vec())
                .unwrap_or_default();
            for a in targs {
                typenames_seen(arena, a, out, seen);
            }
        }
        TypeData::TypeParam(tp) => out.push(tp.obj()),
        TypeData::Pointer(p) => typenames_seen(arena, p.elem(), out, seen),
        TypeData::Slice(sl) => typenames_seen(arena, sl.elem(), out, seen),
        TypeData::Array(ar) => typenames_seen(arena, ar.elem(), out, seen),
        TypeData::Chan(c) => typenames_seen(arena, c.elem(), out, seen),
        TypeData::Map(m) => {
            typenames_seen(arena, m.key(), out, seen);
            typenames_seen(arena, m.elem(), out, seen);
        }
        _ => {}
    }
}

/// `astutil.Format`: the expression as `go/printer` renders it, which is what
/// upstream's message carries — so a generic alias keeps its type arguments
/// (`b.Pair[string, int]`).
///
/// Rendering the AST rather than slicing the source is not a detail: the first
/// attempt subtracted `File.pos()` from the position, but a `Pos` indexes the
/// whole `FileSet` and a file's `package` keyword is not its base, so the
/// message came out as `Type alias a BSD-style // lice should be inlined` on
/// tailscale's `tempfork/heap`. A source-text fallback also disagrees with the
/// printer wherever the two differ, and silently, which is worse than either.
fn format_expr(pass: &Pass<'_>, e: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    match guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(e)) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// Report a use of an inlinable type alias, with the edits that replace it by
/// the alias's right-hand side. (Go: `analyzer.inlineAlias`.)
///
/// `expr_id` is the whole use expression — the `SelectorExpr` for `pkg.A`, the
/// `IndexExpr` for `A[int]` — because that is what carries the instantiated
/// type and what upstream spans.
fn report_alias_inline(
    pass: &Pass<'_>,
    expr_id: u32,
    span: (u32, u32),
    display: String,
    pending: &mut Pending,
) {
    let Some(info) = pass.types_info() else { return };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let arena = &artifacts.types;
    let objects = &artifacts.objects;
    let packages = &artifacts.packages;

    let Some(tv) = info.types.get(&expr_id) else {
        return;
    };
    let guff_types::arena::TypeData::Alias(alias) = arena.get(tv.typ) else {
        return;
    };
    let Some(rhs) = alias.rhs() else { return };

    let Some(file) = refactor::enclosing_file(pass, span.0) else {
        return;
    };
    let cur_pkg = pass.type_pkg();
    let cur_path = pass.pkg().pkg_path.clone();

    // The alias's own type parameters do not appear in the instantiated result.
    let type_param_objs: HashSet<ObjectId> = alias
        .type_params()
        .map(|l| {
            (0..l.len())
                .filter_map(|i| match arena.get(l.at(i)) {
                    guff_types::arena::TypeData::TypeParam(tp) => Some(tp.obj()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();

    let mut tns = Vec::new();
    typenames(arena, rhs, &mut tns);

    let mut prefixes: HashMap<guff_types::PackageId, String> = HashMap::new();
    let mut import_edits: Vec<TextEdit> = Vec::new();
    for tn in tns {
        if type_param_objs.contains(&tn) {
            continue;
        }
        let tn_pkg = tn.pkg(objects);
        let tn_name = tn.name(objects).to_string();
        let same_package = match (tn_pkg, cur_pkg) {
            (None, _) => true, // universe scope
            (Some(p), Some(c)) => p == c,
            (Some(_), None) => false,
        };
        if same_package {
            // No import is needed, but the name must still mean the same thing
            // at the use site as it does in the right-hand side.
            // (Go: `scope.LookupParent(tn.Name(), id.Pos())` must find `tn`.)
            let Some(file_scope) = info.scopes.get(&file.id).copied() else {
                return;
            };
            let Some(scope) =
                guff_types::scope::innermost(&artifacts.scopes, file_scope, span.0)
            else {
                return;
            };
            match guff_types::scope::lookup_parent(
                &artifacts.scopes,
                objects,
                scope,
                &tn_name,
                span.0,
            ) {
                Some((_, found)) if found == tn => {}
                _ => return,
            }
            if let Some(p) = tn_pkg {
                prefixes.insert(p, String::new());
            }
            continue;
        }
        let pkg_id = tn_pkg.expect("same_package covers the None case");
        let pkg_path = packages.get(pkg_id).path().to_string();
        if !can_import(&cur_path, &pkg_path) {
            return;
        }
        if prefixes.contains_key(&pkg_id) {
            continue;
        }
        let pkg_name = packages.get(pkg_id).name().to_string();
        let Some((prefix, edits)) =
            refactor::add_import(pass, file, &pkg_name, &pkg_path, &tn_name, span.0)
        else {
            return;
        };
        prefixes.insert(pkg_id, prefix.trim_end_matches('.').to_string());
        import_edits.extend(edits);
    }

    let new_text = {
        let qf = |pkg: guff_types::PackageId, parena: &guff_types::PackageArena| -> String {
            if Some(pkg) == cur_pkg {
                return String::new();
            }
            match prefixes.get(&pkg) {
                Some(p) => p.clone(),
                // Every package in `rhs` went through the loop above, so this
                // is unreachable; render the path rather than panicking.
                None => parena.get(pkg).path().to_string(),
            }
        };
        guff_types::type_string(arena, objects, packages, rhs, Some(&qf))
    };

    let mut edits = import_edits;
    edits.push(TextEdit {
        pos: span.0,
        end: span.1,
        new_text,
    });
    pending.push((
        span.0,
        format!("Type alias {display} should be inlined"),
        edits,
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "inline requires inspect analyzer".to_string())?
        .clone();

    // Idents that appear as SelectorExpr.Sel are reported via the selector.
    let mut selector_sels = HashSet::new();
    inspect.preorder_typed(node_mask!(SelectorExpr), pass.files(), |n| {
        if let NodeRef::SelectorExpr(sel) = n {
            selector_sels.insert(sel.sel.id);
        }
    });

    // `A[int]` / `A[K, V]`: the use expression upstream spans and types is the
    // index expression, not the `A` inside it. Key them by the id of their
    // operand so the Ident/SelectorExpr arms can widen.
    let mut indexed: HashMap<u32, (u32, Expr)> = HashMap::new();
    inspect.preorder_typed(
        node_mask!(IndexExpr, IndexListExpr),
        pass.files(),
        |n| match n {
            NodeRef::IndexExpr(ie) => {
                indexed.insert(unparen(&ie.x).id(), (ie.id, Expr::IndexExpr(ie.clone())));
            }
            NodeRef::IndexListExpr(ie) => {
                indexed.insert(
                    unparen(&ie.x).id(),
                    (ie.id, Expr::IndexListExpr(ie.clone())),
                );
            }
            _ => {}
        },
    );

    // Local `//go:fix inline` discovery re-reads sources; defer until a
    // non-stdlib candidate appears (prometheus typically only hits reflect.Ptr).
    let mut local: Option<HashMap<ObjectId, String>> = None;
    let mut local_aliases: Option<HashSet<ObjectId>> = None;
    let mut pending: Pending = Vec::new();
    // Only visit CallExpr when a known call-site diagnostic can fire.
    let visit_exp = package_imports_exp_gofix(pass);
    let visit_ioutil = package_imports_ioutil(pass);
    let visit_calls = visit_exp || visit_ioutil;

    // CallExprs used as statements (results discarded).
    let mut stmt_calls = HashSet::new();
    if visit_ioutil {
        inspect.preorder_typed(node_mask!(ExprStmt), pass.files(), |n| {
            if let NodeRef::ExprStmt(ExprStmt { x, .. }) = n {
                if let Expr::CallExpr(call) = unparen(x) {
                    stmt_calls.insert(call.lparen.0);
                }
            }
        });
    }

    let mask = if visit_calls {
        node_mask!(CallExpr, SelectorExpr, Ident)
    } else {
        node_mask!(SelectorExpr, Ident)
    };
    inspect.preorder_typed(mask, pass.files(), |n| {
        match n {
            NodeRef::CallExpr(call) => {
                if visit_exp {
                    check_exp_gofix_call(pass, call, &mut pending);
                }
                if visit_ioutil {
                    check_ioutil_go_version(pass, call, &stmt_calls, &mut pending);
                }
            }
            NodeRef::SelectorExpr(sel) => {
                let info = match pass.types_info() {
                    Some(i) => i,
                    None => return,
                };
                let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
                    return;
                };
                let Some(target) = inline_target(pass, obj, &mut local) else {
                    if inlinable_alias(pass, obj, &mut local_aliases) {
                        let base = Expr::SelectorExpr(sel.clone());
                        let (node, expr) = match indexed.get(&sel.id) {
                            Some((_, e)) => (e.id(), e.clone()),
                            None => (sel.id, base),
                        };
                        let span = (expr.pos().0 as u32, expr.end().0 as u32);
                        let display = format_expr(pass, &expr);
                        report_alias_inline(pass, node, span, display, &mut pending);
                    }
                    return;
                };
                let name = format_expr_name(&Expr::SelectorExpr(sel.clone()));
                // `reportInline` spans the *selector* when the name is
                // qualified — `cur.ParentEdgeKind() == edge.SelectorExpr_Sel`
                // upstream (inline.go:554) — not just the `Sel` ident.
                pending.push((
                    sel.x.pos().0 as u32,
                    format!("Constant {name} should be inlined"),
                    vec![TextEdit {
                        pos: sel.x.pos().0 as u32,
                        end: sel.sel.end().0 as u32,
                        new_text: target,
                    }],
                ));
            }
            NodeRef::Ident(id) => {
                if selector_sels.contains(&id.id) {
                    return;
                }
                let info = match pass.types_info() {
                    Some(i) => i,
                    None => return,
                };
                // Uses only — definitions of inlinable consts are not reported.
                let Some(obj) = info.uses.get(&id.id).copied() else {
                    return;
                };
                let Some(target) = inline_target(pass, obj, &mut local) else {
                    if inlinable_alias(pass, obj, &mut local_aliases) {
                        let base = Expr::Ident(id.clone());
                        let (node, expr) = match indexed.get(&id.id) {
                            Some((_, e)) => (e.id(), e.clone()),
                            None => (id.id, base),
                        };
                        let span = (expr.pos().0 as u32, expr.end().0 as u32);
                        let display = format_expr(pass, &expr);
                        report_alias_inline(pass, node, span, display, &mut pending);
                    }
                    return;
                };
                pending.push((
                    id.pos().0 as u32,
                    format!("Constant {} should be inlined", id.name),
                    vec![TextEdit {
                        pos: id.pos().0 as u32,
                        end: id.end().0 as u32,
                        new_text: target,
                    }],
                ));
            }
            _ => {}
        }
    });

    for (pos, message, text_edits) in pending {
        if text_edits.is_empty() {
            pass.reportf(pos, message);
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: String::new(),
                text_edits,
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "inline",
        doc: "apply fixes based on go:fix inline directives (constants + known generic call diagnostics)",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/inline",
        run: run as RunFn,
        // Match x/tools and golangci: still report when the package has
        // soft type errors (e.g. prometheus discovery under hybrid check).
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

