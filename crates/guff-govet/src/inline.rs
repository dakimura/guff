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

use guff::ast::{CallExpr, CommentGroup, Decl, Expr, GenDecl, Spec, ValueSpec};
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

/// How many type arguments the *call site* spells out.
///
/// `st.typeArguments(caller.Call)` — `f(x)` spells none, `f[int](x)` one,
/// `f[int, string](x)` two.
fn explicit_type_args(call: &CallExpr) -> usize {
    match unparen(&call.fun) {
        Expr::IndexExpr(_) => 1,
        Expr::IndexListExpr(il) => il.indices.len(),
        _ => 0,
    }
}

/// The callee's own type parameters, if it is a generic function.
fn callee_type_param_count(pass: &Pass<'_>, obj: ObjectId) -> Option<usize> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return None;
    }
    let typ = obj.typ(&artifacts.objects)?;
    let tps = guff_types::signature::signature_type_params(&artifacts.types, typ)?;
    Some(tps.len())
}

/// The generic `//go:fix inline` functions a directly-imported package
/// declares, read from its own source and memoised per directory.
///
/// [`is_known_generic_gofix_inline`] is a table of x/exp names because a
/// dependency loaded from export data has no doc comments. A package in the
/// **same module** has its source right there — `pass.pkg().imports` carries
/// the `Package`, `dir` and all — so the directive can simply be read.
///
/// vitess declares its own:
///
/// ```go
/// // Of returns a pointer to the given value
/// //
/// //go:fix inline
/// func Of[T any](x T) *T { return &x }
/// ```
///
/// in `vitess.io/vitess/go/ptr`, and calls it from sixteen places across five
/// packages. Upstream reports every one of them; guff had only the x/exp table
/// and reported none. The old note said `//go:fix` discovery stops at the
/// package boundary because upstream uses a fact — true for the *alias* and
/// *const* arms, which need the declaration's right-hand side, but this
/// diagnostic needs only the name and the directive.
fn dir_gofix_inline_funcs(dir: &std::path::Path) -> std::sync::Arc<HashSet<String>> {
    type Cache = std::collections::HashMap<std::path::PathBuf, std::sync::Arc<HashSet<String>>>;
    static FUNCS: OnceLock<std::sync::Mutex<Cache>> = OnceLock::new();
    let cache = FUNCS.get_or_init(|| std::sync::Mutex::new(Cache::new()));
    if let Ok(map) = cache.lock() {
        if let Some(hit) = map.get(dir) {
            return std::sync::Arc::clone(hit);
        }
    }

    let mut names: HashSet<String> = HashSet::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("go") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(src) = fs::read(&path) else {
                continue;
            };
            // Same cheap screen as everywhere else: almost no file carries the
            // directive, and a PARSE_COMMENTS reparse is not worth paying for
            // the ones that do not.
            if memchr::memmem::find(&src, b"go:fix inline").is_none() {
                continue;
            }
            let fset = FileSet::new();
            let Ok(parsed) = parse_file(&fset, file_name, &src, PARSE_COMMENTS) else {
                continue;
            };
            for decl in &parsed.decls {
                let Decl::FuncDecl(f) = decl else {
                    continue;
                };
                if has_fix_inline(&f.doc) {
                    names.insert(f.name.name.clone());
                }
            }
        }
    }
    let arc = std::sync::Arc::new(names);
    if let Ok(mut map) = cache.lock() {
        map.insert(dir.to_path_buf(), std::sync::Arc::clone(&arc));
    }
    arc
}

/// A call to a generic `//go:fix inline` function declared in a package this
/// one imports, whose type arguments the call site does not spell out:
///
/// ```go
/// typeArgs := st.typeArguments(caller.Call)
/// if len(typeArgs) != len(callee.TypeParams) {
///     return nil, fmt.Errorf("cannot inline: type parameter inference is not yet supported")
/// }
/// ```
///
/// Only generic callees are looked up, so the source scan runs for the handful
/// of packages that actually declare one.
fn check_local_gofix_generic_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Pending) {
    let Expr::SelectorExpr(sel) = unparen(call_fun_base(&call.fun)) else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };
    let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
        return;
    };
    let Some(n_params) = callee_type_param_count(pass, obj) else {
        return;
    };
    if n_params == 0 || explicit_type_args(call) == n_params {
        return;
    }
    let Some(pkg_path) = object_pkg_path(pass, obj) else {
        return;
    };
    // Already covered by the x/exp table and its version check.
    if matches!(
        pkg_path.as_str(),
        "golang.org/x/exp/maps" | "golang.org/x/exp/slices"
    ) {
        return;
    }
    let Some(dep) = pass.pkg().imports.get(&pkg_path) else {
        return;
    };
    if !dir_gofix_inline_funcs(&dep.dir).contains(sel.sel.name.as_str()) {
        return;
    }
    pending.push((
        call.lparen.0 as u32,
        "cannot inline: type parameter inference is not yet supported".into(),
        Vec::new(),
    ));
}

/// The selector under any explicit instantiation: `pkg.F` in both `pkg.F(x)`
/// and `pkg.F[int](x)`.
fn call_fun_base(fun: &Expr) -> &Expr {
    match unparen(fun) {
        Expr::IndexExpr(ix) => &ix.x,
        Expr::IndexListExpr(il) => &il.x,
        other => other,
    }
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
    if !is_known_generic_gofix_inline(pkg, func) {
        return;
    }
    // The table above is a claim about a *version* of x/exp, and it is only
    // true from 2025-02-10 on. When the dependency's source is on disk, ask it
    // instead of believing the table — see `vendored_has_gofix_inline`.
    if vendored_has_gofix_inline(pass, pkg, func) == Some(false) {
        return;
    }
    pending.push((
        call.lparen.0 as u32,
        "cannot inline: type parameter inference is not yet supported".into(),
        Vec::new(),
    ));
}

/// Does the **vendored** copy of `pkg_path` really carry `//go:fix inline` on
/// `name`? `None` when there is no vendored copy to read.
///
/// [`is_known_generic_gofix_inline`] hardcodes the generic `//go:fix inline`
/// functions of `golang.org/x/exp/{maps,slices}` because a dependency loaded
/// from export data has no doc comments to discover them in. That list is
/// correct for current x/exp and **wrong for older ones**: the directives were
/// added around 2025-02-10, and lazygit v0.64.1 vendors
/// `v0.0.0-20240719175910`, whose `slices` carries none. Upstream reads the
/// declaration, so it is right for every version; guff applied the table
/// unconditionally and invented fourteen findings there.
///
/// The source is read wherever this host can find it — a vendor directory
/// first, then the module cache via `go list` (see [`dependency_dir`]) — and
/// the table is reduced to a shortlist of which packages are worth opening.
/// Only when neither can be read does the table decide, which is the old
/// behaviour and keeps an offline run working.
///
/// go-ethereum v1.17.5 is why the module-cache half exists: it does not vendor
/// and pins x/exp at `v0.0.0-20230626212559`, so `maps.Copy` was reported by
/// guff alone. consul and vault do not vendor either, but are on 2025-05 and
/// 2025-08 x/exp, and keep the nine findings they match upstream on.
fn vendored_has_gofix_inline(pass: &Pass<'_>, pkg_path: &str, name: &str) -> Option<bool> {
    let dir = dependency_dir(&pass.pkg().dir, pkg_path)?;
    Some(dir_declares_gofix_inline(&dir, name))
}

/// Where the dependency's source is, if it is anywhere this host can read.
///
/// A vendor directory answers without asking anything: `<module>/vendor/<path>`
/// needs no version resolution. Without one the version does matter — the
/// directives entered x/exp around 2025-02-10, so consul and vault (2025-05 and
/// 2025-08) really do carry them while go-ethereum (`v0.0.0-20230626212559`)
/// does not — and `go list` is the authority on which version this module
/// selected, `replace` directives and workspaces included.
///
/// A failure is not a wrong answer: `None` means "could not look", and the
/// caller then falls back to the table, which is what guff did everywhere
/// before. That keeps an offline or sandboxed run behaving as it used to.
///
/// One subprocess per import path per process, memoised — the caller only ever
/// asks about `golang.org/x/exp/maps` and `golang.org/x/exp/slices`.
fn dependency_dir(from: &std::path::Path, pkg_path: &str) -> Option<std::path::PathBuf> {
    if let Some(vendor) = nearest_vendored_dir(from, pkg_path) {
        return Some(vendor);
    }

    type Cache = std::collections::HashMap<(std::path::PathBuf, String), Option<std::path::PathBuf>>;
    static DIRS: OnceLock<std::sync::Mutex<Cache>> = OnceLock::new();
    let cache = DIRS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let key = (from.to_path_buf(), pkg_path.to_string());
    if let Ok(map) = cache.lock() {
        if let Some(hit) = map.get(&key) {
            return hit.clone();
        }
    }

    let found = std::process::Command::new("go")
        .args(["list", "-f", "{{.Dir}}", "--", pkg_path])
        .current_dir(from)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| {
            let dir = String::from_utf8(out.stdout).ok()?;
            let dir = std::path::PathBuf::from(dir.trim());
            dir.is_dir().then_some(dir)
        });

    if let Ok(mut map) = cache.lock() {
        map.insert(key, found.clone());
    }
    found
}

/// Walk up from a package directory to the `vendor/<import path>` Go would
/// have resolved the import to, if the module vendors.
///
/// The walk stops at the module root — the first ancestor holding a `go.mod`,
/// checked for `vendor/` itself before stopping — so it cannot wander into an
/// unrelated tree above the module.
fn nearest_vendored_dir(from: &std::path::Path, pkg_path: &str) -> Option<std::path::PathBuf> {
    let mut cur = Some(from);
    while let Some(dir) = cur {
        let candidate = dir.join("vendor").join(pkg_path);
        if candidate.is_dir() {
            return Some(candidate);
        }
        if dir.join("go.mod").is_file() {
            return None;
        }
        cur = dir.parent();
    }
    None
}

/// Scan one directory's non-test Go files for `func <name>` carrying
/// `//go:fix inline`.
fn dir_declares_gofix_inline(dir: &std::path::Path, name: &str) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("go") {
            continue;
        }
        let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if file_name.ends_with("_test.go") {
            continue;
        }
        let Ok(src) = fs::read(&path) else {
            continue;
        };
        // Same cheap filter as the local scan: almost no file carries the
        // directive, and a full PARSE_COMMENTS reparse is not worth paying for
        // the ones that do not.
        if memchr::memmem::find(&src, b"go:fix inline").is_none() {
            continue;
        }
        let fset = FileSet::new();
        let Ok(parsed) = parse_file(&fset, file_name, &src, PARSE_COMMENTS) else {
            continue;
        };
        for decl in &parsed.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if f.name.name == name && has_fix_inline(&f.doc) {
                return true;
            }
        }
    }
    false
}

/// The `io/ioutil` `//go:fix inline` wrapper this call targets, and the name
/// upstream prints for it.
///
/// Both arms below start here, and both take the callee from the *object*
/// rather than from the syntax, because upstream does: it reaches the callee
/// with `typeutil.StaticCallee`, so an aliased import (`iou.ReadFile(p)`) and
/// a dot import (`ReadFile(p)`) name the same callee as `ioutil.ReadFile(p)`,
/// and all three are reported. The printed name comes from the same object —
/// `fmt.Sprintf("%s.%s", fn.Pkg().Name(), fn.Name())` in `AnalyzeCallee` — so
/// all three render `ioutil.ReadFile`: never the alias, never a bare
/// `ReadFile`. Rendering the source expression instead (what an earlier
/// revision did) printed `iou.ReadFile` and missed the dot import entirely,
/// which is two diffs on a shape a corpus target can reach.
///
/// A use that is not a call of the wrapper is not a call site: `ioutil.ReadFile`
/// as a value, `f := ioutil.ReadFile; f(p)` (the callee is `f`), and
/// `ioutil.Discard` (a var). `ioutil.ReadDir` carries no directive — its result
/// type differs from `os.ReadDir`'s, so it is not a forwarder — and so is not
/// in the table.
fn ioutil_gofix_callee(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    let ident = match unparen(&call.fun) {
        Expr::SelectorExpr(sel) => sel.sel.id,
        Expr::Ident(id) => id.id,
        _ => return None,
    };
    let obj = pass.types_info()?.uses.get(&ident).copied()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg = artifacts.packages.get(obj.pkg(&artifacts.objects)?);
    if pkg.path() != "io/ioutil" {
        return None;
    }
    let name = obj.name(&artifacts.objects);
    if !is_known_ioutil_gofix_inline(name) {
        return None;
    }
    Some(format!("{}.{}", pkg.name(), name))
}

/// Report when inlining an `io/ioutil` go:fix wrapper would pull a newer
/// dialect into an older caller file (upstream #75726 stopgap).
///
/// The comparison is `versions.Before(callerFileVersion, callee.GoVersion)`,
/// made at the top of `inline.Inline` before it looks at the call's context at
/// all — which is why every call site is reported, including one whose results
/// are discarded.
///
/// An earlier revision skipped call-as-statement sites, on the evidence that
/// golangci-lint was silent at vault `helper/pkcs7/sign_test.go:119`
/// (`ioutil.WriteFile(...)`, error dropped). That was a property of the
/// comparison config of the day, not of upstream: `issues.uniq-by-line` was
/// still on, it keeps one finding per (file, line) across all linters, and
/// `errcheck` — which sorts before `govet` — owns exactly the lines where an
/// error is dropped. Measured again with the key off, upstream reports the
/// statement sites too; with it on, both tools drop them. The guard never
/// matched a rule of upstream's; it only hid guff's own findings.
fn check_ioutil_go_version(pass: &Pass<'_>, call: &CallExpr, pending: &mut Pending) {
    let Some(display) = ioutil_gofix_callee(pass, call) else {
        return;
    };
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
    pending.push((
        pos,
        format!("cannot inline call to {display} (declared using {callee}) into a file using {caller}"),
        Vec::new(),
    ));
}

/// `Call of ioutil.X should be inlined` — the other side of the version gate.
///
/// Upstream decides this arm by *running* the inliner (`inline.Inline`) and
/// reporting unless the result is `Literalized` or carries a `BindingDecl`.
/// guff has no inliner, which is why the function arm is deferred in general
/// (`compat/golden/cases/inline-gofix-sibling/ratchet.json`).
///
/// The `io/ioutil` wrappers are the one class where the answer does not need
/// one. They are a closed set of six, already written down here because their
/// directive cannot be read from export data, and each body is a single
/// forwarding call whose parameters are passed straight through
/// (`func ReadFile(name string) ([]byte, error) { return os.ReadFile(name) }`).
/// Nothing in that shape can literalize or bind, and 22 measured shapes agree:
/// upstream reports every call of all six, in expression, statement, `defer`
/// and `go` position, nested in another call, through a parenthesized callee,
/// inside a closure or a composite literal, whether or not the caller imports
/// the forwarded-to package, and even when the caller shadows it.
///
/// The position is the call's, not its `Lparen`: upstream's two arms differ
/// there (`Pos: call.Pos()` here, `Reportf(call.Lparen, …)` for the error), and
/// `(ioutil.ReadFile)(p)` shows the difference — `call.Pos()` is the `(`.
///
/// Not ported: `withinTestOf`, which suppresses a call inside the callee's own
/// `TestX`/`ExampleX`/`BenchX`. It first requires the caller's package path to
/// equal the callee's, so for these six it can only fire inside GOROOT's
/// `io/ioutil` tests, which no target lints.
///
/// The version test is the same one [`check_ioutil_go_version`] uses, so the
/// two arms are complementary and never both fire. It inherits that function's
/// approximation of the callee's version — the running toolchain rather than
/// the Go that built golangci-lint's own binary, which on this machine reads
/// go1.26.2 against a go1.26.5 toolchain. `compat/normalize.py` drops the patch
/// component for exactly this reason; a caller pinned *between* the two is the
/// one window where guff picks the wrong arm, measured on 2026-09-21 and left
/// alone, because narrowing it belongs to the arm that owns the comparison.
fn check_ioutil_should_be_inlined(pass: &Pass<'_>, call: &CallExpr, pending: &mut Pending) {
    let Some(display) = ioutil_gofix_callee(pass, call) else {
        return;
    };
    // The version arm owns this call site when the caller's file is older.
    let caller = effective_file_go_version(pass, call.lparen.0 as u32);
    let callee = toolchain_go_version();
    if !caller.is_empty() && !callee.is_empty() && version_compare(&caller, &callee) < 0 {
        return;
    }
    pending.push((
        call.pos().0 as u32,
        format!("Call of {display} should be inlined"),
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

    // `check_local_gofix_generic_call` needs every call, whatever this package
    // imports: the declaring package is found from the callee, not from a
    // table of known paths.
    let _ = visit_calls;
    let mask = node_mask!(CallExpr, SelectorExpr, Ident);
    inspect.preorder_typed(mask, pass.files(), |n| {
        match n {
            NodeRef::CallExpr(call) => {
                if visit_exp {
                    check_exp_gofix_call(pass, call, &mut pending);
                }
                check_local_gofix_generic_call(pass, call, &mut pending);
                if visit_ioutil {
                    check_ioutil_go_version(pass, call, &mut pending);
                    check_ioutil_should_be_inlined(pass, call, &mut pending);
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

