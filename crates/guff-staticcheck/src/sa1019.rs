//! SA1019 — using a deprecated function, variable, constant or field.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1019`.
//!
//! Third-party deps are type-checked without `PARSE_COMMENTS` (function docs
//! dropped) and fact remapping can miss package facts. When an `IsDeprecated`
//! fact is absent for an external import, we lazily re-parse that package's
//! sources (cached, `Deprecated:` byte-filtered) — same shape as ST1020 /
//! govet-inline local discovery.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{CompositeLit, Decl, Expr, GenDecl, ImportSpec, SelectorExpr, Spec};
use guff::node_mask;
use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{NodeMask, NodeRef};
use guff_analysis::code::{
    knowledge_selector_name, object_of, object_pkg_path, stdlib_version, version_compare,
};
use guff_analysis::passes::facts::deprecated;
use guff_analysis::passes::facts::generated;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, DeprecatedResult, IsDeprecated, RunError, RunFn, Pass};
use guff_types::arena::{ObjectData, TypeData};

use crate::render::render_expr;
use crate::stdlib_deprecations::{
    stdlib_deprecations, stdlib_package_deprecation_msg, Deprecation, DEPRECATED_NEVER_USE,
    DEPRECATED_USE_NO_LONGER,
};

fn related_pkg_path(pass: &Pass<'_>, path: &str) -> bool {
    let cur = pass.pkg().pkg_path.as_str();
    path == cur
        || cur.strip_suffix("_test") == Some(path)
        || cur.strip_suffix(".test") == Some(path)
        || cur.strip_suffix(".test") == Some(path.strip_suffix("_test").unwrap_or(path))
}

fn is_stdlib_path(path: &str) -> bool {
    !path.contains('.')
}

fn format_go_version(s: &str) -> String {
    format!("Go {}", s.strip_prefix("go").unwrap_or(s))
}

fn deprecation_message(name: &str, depr: &IsDeprecated, std: Option<&Deprecation>) -> Option<String> {
    let std = std?;
    Some(match std.alternative_available_since {
        DEPRECATED_NEVER_USE => format!(
            "{name} has been deprecated since {} because it shouldn't be used: {}",
            format_go_version(std.deprecated_since),
            depr.msg
        ),
        v if v == std.deprecated_since || v == DEPRECATED_USE_NO_LONGER => format!(
            "{name} has been deprecated since {}: {}",
            format_go_version(std.deprecated_since),
            depr.msg
        ),
        alt => format!(
            "{name} has been deprecated since {} and an alternative has been available since {}: {}",
            format_go_version(std.deprecated_since),
            format_go_version(alt),
            depr.msg
        ),
    })
}

fn handle_deprecation(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    depr: &IsDeprecated,
    // Key for `knowledge.StdlibDeprecations` (honnef `SelectorName` / import path).
    table_key: &str,
    // Source-rendered name for the diagnostic (honnef `report.Render`).
    display_name: &str,
    pkg_path: &str,
    pos: u32,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Option<String> {
    let table = stdlib_deprecations();
    let table_key = table_key.trim_matches('"');
    let std = table.get(table_key).or_else(|| table.get(display_name));
    if std.is_none() && is_stdlib_path(pkg_path) {
        return None;
    }
    if let Some(std) = std {
        if version_compare(&stdlib_version(pass, pos), std.deprecated_since) < 0 {
            return None;
        }
    }
    if current_fn.is_some_and(|f| {
        // Deprecated functions may use deprecated symbols.
        deprs.objects.contains_key(&f)
    }) {
        return None;
    }
    if let Some(std) = std {
        deprecation_message(display_name, depr, Some(std))
    } else {
        Some(format!("{display_name} is deprecated: {}", depr.msg))
    }
}

fn extract_deprecated_message(doc: &Option<guff::ast::CommentGroup>) -> Option<String> {
    let doc = doc.as_ref()?;
    for part in doc.text().split("\n\n") {
        if let Some(rest) = part.strip_prefix("Deprecated: ") {
            // See the note in the deprecated fact pass: upstream does not
            // trim, and the trailing space reaches the printed message.
            return Some(rest.replace('\n', " "));
        }
    }
    None
}

fn receiver_type_name(ty: &Expr) -> Option<&str> {
    let mut t = ty;
    if let Expr::StarExpr(star) = t {
        t = &star.x;
    }
    match t {
        Expr::Ident(id) => Some(id.name.as_str()),
        Expr::IndexExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        Expr::IndexListExpr(idx) => match &*idx.x {
            Expr::Ident(id) => Some(id.name.as_str()),
            _ => None,
        },
        _ => None,
    }
}

fn method_fact_key(recv_type_name: &str, method: &str) -> String {
    format!("{recv_type_name}.{method}")
}

fn func_has_receiver(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    obj: guff_types::arena::ObjectId,
) -> bool {
    let ObjectData::Func(f) = objects.get(obj) else {
        return false;
    };
    let Some(sig) = f.typ() else {
        return false;
    };
    matches!(types.get(sig), TypeData::Signature(s) if s.recv().is_some())
}

fn method_recv_base_from_sig(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    obj: guff_types::arena::ObjectId,
) -> Option<String> {
    let ObjectData::Func(f) = objects.get(obj) else {
        return None;
    };
    let sig = f.typ()?;
    let TypeData::Signature(s) = types.get(sig) else {
        return None;
    };
    let recv = s.recv()?;
    let mut recv_typ = recv.typ(objects)?;
    let resolved = guff_types::alias::unalias_readonly(types, recv_typ);
    if let TypeData::Pointer(p) = types.get(resolved) {
        recv_typ = p.elem();
    }
    let resolved = guff_types::alias::unalias_readonly(types, recv_typ);
    match types.get(resolved) {
        TypeData::Named(_) => {
            let named = guff_types::named::named_obj(types, resolved);
            Some(named.name(objects).to_string())
        }
        _ => None,
    }
}

/// Base type name for a method receiver (`*pkg.T` / `pkg.T` → `T`).
fn selection_recv_base_name(pass: &Pass<'_>, sel: &SelectorExpr) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let selection = info.selections.get(&sel.id)?;
    let mut recv = selection.recv();
    let resolved = guff_types::alias::unalias_readonly(&artifacts.types, recv);
    if let TypeData::Pointer(p) = artifacts.types.get(resolved) {
        recv = p.elem();
    }
    let resolved = guff_types::alias::unalias_readonly(&artifacts.types, recv);
    match artifacts.types.get(resolved) {
        TypeData::Named(_) => {
            let obj = guff_types::named::named_obj(&artifacts.types, resolved);
            Some(obj.name(&artifacts.objects).to_string())
        }
        _ => None,
    }
}

#[derive(Default)]
struct PkgDeprecatedFacts {
    package: Option<String>,
    /// Package-level funcs / vars / consts / types (no receiver).
    objects: HashMap<String, String>,
    /// Methods keyed by `TypeName.Method` (receiver present). Separate from
    /// `objects` so `(*ACL).Create` does not poison `(*Namespaces).Create`.
    methods: HashMap<String, String>,
    /// Struct fields keyed by `TypeName.Field`, for the same reason.
    fields: HashMap<String, String>,
    /// True once PARSE_COMMENTS object/method extraction has run.
    objects_scanned: bool,
}

#[derive(Default)]
struct DepDeprecatedCache {
    /// Per-pass memo; process-global store is consulted first so shared
    /// third-party imports are not re-parsed for every root package.
    pkgs: HashMap<String, Arc<PkgDeprecatedFacts>>,
}

fn global_dep_store() -> &'static std::sync::Mutex<HashMap<String, Arc<PkgDeprecatedFacts>>> {
    static STORE: OnceLock<std::sync::Mutex<HashMap<String, Arc<PkgDeprecatedFacts>>>> =
        OnceLock::new();
    STORE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

fn src_has_deprecated_doc(src: &[u8]) -> bool {
    // Match doc-comment forms only — bare `Deprecated:` in strings is common
    // and would force expensive PARSE_COMMENTS on unrelated files.
    //
    // `windows().any()` compared byte-by-byte over every dependency file this
    // probe rejects, which is nearly all of them; `memmem` is the same search
    // vectorized. Both find the identical first match, so the answer cannot
    // change.
    memchr::memmem::find(src, b"// Deprecated:").is_some()
        || memchr::memmem::find(src, b"* Deprecated:").is_some()
}

/// Package comments sit immediately above the `package` clause. Object-level
/// `// Deprecated:` elsewhere must not force a Mode::NONE parse when we only
/// need the package fact (e.g. golang/protobuf/proto/deprecated.go).
fn src_has_package_deprecated_doc(src: &[u8]) -> bool {
    let preamble = match memchr::memmem::find(src, b"\npackage") {
        Some(i) => &src[..=i],
        None => {
            // Single-line / no leading newline before `package`.
            if let Some(i) = memchr::memmem::find(src, b"package") {
                &src[..i]
            } else {
                src
            }
        }
    };
    src_has_deprecated_doc(preamble)
}

/// Prefer conventional homes for package docs (`doc.go`, `{basename}.go`).
///
/// Each entry keeps its index in `files` so the caller can ask the package for
/// bytes it already holds instead of opening the file again.
fn prefer_package_doc_files<'a>(
    files: &'a [std::path::PathBuf],
    pkg_path: &str,
) -> Vec<(usize, &'a std::path::PathBuf)> {
    let base = format!("{}.go", pkg_path.rsplit('/').next().unwrap_or(""));
    let mut paths: Vec<(usize, &std::path::PathBuf)> = files
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            !p.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.ends_with("_test.go"))
        })
        .collect();
    // `sort_by_key` is stable, so files in the same bucket keep `files` order.
    paths.sort_by_key(|(_, p)| {
        let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
        if n == "doc.go" {
            0
        } else if n == base {
            1
        } else {
            2
        }
    });
    paths
}

/// Packages whose *file* docs we reparse (Mode::NONE) to recover a
/// package-level `Deprecated:` that facts did not carry over. Only the
/// conventional homes (`doc.go`, `{basename}.go`) are read, and each is
/// rejected by a byte-level probe before parsing.
fn worth_package_doc_scan(pkg_path: &str) -> bool {
    !is_stdlib_path(pkg_path)
}

/// Third-party / nested-module packages whose object/method docs we will
/// PARSE_COMMENTS-scan even without a package-level deprecation.
///
/// `Deprecated` facts only reach us for packages analysed from source. A
/// dependency read from export data carries no doc comments at all, so every
/// `Deprecated:` in a third-party module is invisible without this scan —
/// which used to be an allowlist of the handful of modules we had happened to
/// hit. The stdlib is excluded because it is covered by the
/// [`stdlib_deprecations`] table.
///
/// The scan is affordable because it is bounded by dependency *packages*, not
/// call sites: `dep_facts` memoises each package process-wide, and each file is
/// rejected by a byte-level `Deprecated:` probe before it is ever parsed.
fn worth_object_doc_scan(pkg_path: &str) -> bool {
    !is_stdlib_path(pkg_path)
}

/// Deps whose sources are local replaces / nested modules / test stubs — not
/// every in-repo package (that would reparse half of prometheus on cold wall).
fn is_local_source_dep(pass: &Pass<'_>, pkg_path: &str) -> bool {
    let Some(imp) = pass.pkg().imports.get(pkg_path) else {
        return false;
    };
    let files = if !imp.compiled_go_files.is_empty() {
        &imp.compiled_go_files
    } else {
        &imp.go_files
    };
    let Some(p) = files.first() else {
        return false;
    };
    let s = p.to_string_lossy();
    if s.contains("/pkg/mod/") || s.contains("/goroot/") || s.contains("/GOROOT/") {
        return false;
    }
    if s.contains("/testdata/") || s.contains("/stub/") {
        return true;
    }
    let Some(m) = pass.pkg().module.as_ref() else {
        return false;
    };
    let mod_dir = std::path::Path::new(&m.dir);
    let mut cur = p.parent();
    while let Some(dir) = cur {
        if dir == mod_dir {
            break;
        }
        if !dir.starts_with(mod_dir) {
            // Replace pointing outside the main module tree.
            return true;
        }
        if dir.join("go.mod").is_file() {
            // Nested module (e.g. vault/api).
            return true;
        }
        cur = dir.parent();
    }
    false
}

fn store_global(pkg_path: &str, facts: Arc<PkgDeprecatedFacts>) {
    if let Ok(mut g) = global_dep_store().lock() {
        g.insert(pkg_path.to_string(), facts);
    }
}

fn dep_facts<'a>(
    pass: &Pass<'_>,
    cache: &'a mut DepDeprecatedCache,
    pkg_path: &str,
    need_objects: bool,
) -> &'a PkgDeprecatedFacts {
    let cached_ok = cache
        .pkgs
        .get(pkg_path)
        .is_some_and(|e| !need_objects || e.objects_scanned);
    if cached_ok {
        return cache.pkgs.get(pkg_path).expect("checked");
    }

    if let Some(shared) = global_dep_store()
        .lock()
        .ok()
        .and_then(|g| g.get(pkg_path).cloned())
    {
        if !need_objects || shared.objects_scanned {
            cache.pkgs.insert(pkg_path.to_string(), shared);
            return cache.pkgs.get(pkg_path).expect("just inserted");
        }
    }

    if is_stdlib_path(pkg_path) {
        let empty = Arc::new(PkgDeprecatedFacts {
            objects_scanned: true,
            ..PkgDeprecatedFacts::default()
        });
        store_global(pkg_path, Arc::clone(&empty));
        cache.pkgs.insert(pkg_path.to_string(), empty);
        return cache.pkgs.get(pkg_path).expect("just inserted");
    }

    // Only scan (and cache globally) when this package can see the dep's
    // sources. A miss must not poison the process-wide store — another root
    // package may import the same path and need a real PARSE_COMMENTS scan.
    if !pass.pkg().imports.contains_key(pkg_path) {
        let empty = Arc::new(PkgDeprecatedFacts::default());
        cache.pkgs.insert(pkg_path.to_string(), empty);
        return cache.pkgs.get(pkg_path).expect("just inserted");
    }

    let scanned = Arc::new(scan_import_deprecated(pass, pkg_path, need_objects));
    store_global(pkg_path, Arc::clone(&scanned));
    cache.pkgs.insert(pkg_path.to_string(), scanned);
    cache.pkgs.get(pkg_path).expect("just inserted")
}

fn scan_import_deprecated(
    pass: &Pass<'_>,
    pkg_path: &str,
    need_objects: bool,
) -> PkgDeprecatedFacts {
    let Some(imp) = pass.pkg().imports.get(pkg_path) else {
        // Missing from this package's import graph — do not claim a completed
        // object scan (and callers must not poison the process-global cache).
        return PkgDeprecatedFacts::default();
    };
    let files = if !imp.compiled_go_files.is_empty() {
        &imp.compiled_go_files
    } else {
        &imp.go_files
    };
    let mut out = PkgDeprecatedFacts {
        objects_scanned: need_objects,
        ..PkgDeprecatedFacts::default()
    };
    // Package docs survive without PARSE_COMMENTS; object docs need it. Neither
    // needs object resolution — this scan reads `Deprecated:` doc text only.
    let parse_mode = if need_objects {
        COMMENTS_ONLY
    } else {
        guff::parser::SKIP_OBJECT_RESOLUTION
    };
    let mut paths = prefer_package_doc_files(files, pkg_path);
    if !need_objects {
        // Import diagnostics only need the package doc. Restrict to the
        // conventional homes (`doc.go`, `{basename}.go`) — walking every
        // file of every third-party import dominates prometheus cold ./...
        // wall. Object-level Deprecated: lives elsewhere and is handled by
        // the gated PARSE_COMMENTS path.
        let base = format!("{}.go", pkg_path.rsplit('/').next().unwrap_or(""));
        paths.retain(|(_, p)| {
            let n = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            n == "doc.go" || n == base.as_str()
        });
    }
    // `source_files` is parallel to `syntax`, which is parallel to
    // `compiled_go_files` — so the index is only a valid key into it when the
    // list above is the compiled one.
    let in_memory = !imp.compiled_go_files.is_empty();
    for (idx, path) in paths {
        let owned;
        let src: &[u8] = match imp.source_bytes(idx).filter(|_| in_memory) {
            // A dependency inside the same module is usually one of the root
            // packages guff already type-checked from source, and those keep
            // their bytes. Opening every dependency file again was a third of
            // this analyzer's CPU on prometheus `./...`.
            Some(bytes) => bytes,
            None => match fs::read(path) {
                Ok(read) => {
                    owned = read;
                    &owned
                }
                Err(_) => continue,
            },
        };
        // Package-only: preamble filter so object-level Deprecated: files are
        // skipped cheaply. Object scan: any doc Deprecated: may matter.
        let interesting = if need_objects {
            src_has_deprecated_doc(src)
        } else {
            src_has_package_deprecated_doc(src)
        };
        if !interesting {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let fset = FileSet::new();
        let Ok(file) = parse_file(&fset, name, src, parse_mode) else {
            continue;
        };
        if out.package.is_none() {
            if let Some(msg) = extract_deprecated_message(&file.doc) {
                out.package = Some(msg);
            }
        }
        if !need_objects {
            // Import diagnostics only need the package doc; stop once found.
            if out.package.is_some() {
                break;
            }
            continue;
        }
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(fd) => {
                    if let Some(msg) = extract_deprecated_message(&fd.doc) {
                        if let Some(recv) = fd.recv.as_ref().and_then(|r| r.list.first()) {
                            if let Some(ty) = &recv.ty {
                                if let Some(type_name) = receiver_type_name(ty) {
                                    out.methods.insert(
                                        method_fact_key(type_name, &fd.name.name),
                                        msg,
                                    );
                                }
                            }
                        } else {
                            out.objects.insert(fd.name.name.clone(), msg);
                        }
                    }
                }
                Decl::GenDecl(GenDecl {
                    doc: decl_doc,
                    tok: Some(tok),
                    specs,
                    ..
                }) if matches!(tok, Token::CONST | Token::VAR | Token::TYPE) => {
                    let decl_msg = extract_deprecated_message(decl_doc);
                    for spec in specs {
                        match spec {
                            Spec::ValueSpec(vs) => {
                                let msg = extract_deprecated_message(&vs.doc).or_else(|| {
                                    decl_msg.clone()
                                });
                                if let Some(msg) = msg {
                                    for n in &vs.names {
                                        out.objects.insert(n.name.clone(), msg.clone());
                                    }
                                }
                            }
                            Spec::TypeSpec(ts) => {
                                let msg = extract_deprecated_message(&ts.doc).or_else(|| {
                                    decl_msg.clone()
                                });
                                if let Some(msg) = msg {
                                    out.objects.insert(ts.name.name.clone(), msg);
                                }
                                // An interface's *methods* carry their own
                                // `Deprecated:` docs, and a call through the
                                // interface value is what the caller writes.
                                // Only concrete methods (`Decl::FuncDecl` with
                                // a receiver) were being collected, so a
                                // deprecated interface method was silent for
                                // every importer.
                                if let Expr::InterfaceType(it) = &ts.ty {
                                    for m in &it.methods.list {
                                        let Some(mmsg) = extract_deprecated_message(&m.doc)
                                        else {
                                            continue;
                                        };
                                        for name in &m.names {
                                            out.methods.insert(
                                                method_fact_key(&ts.name.name, &name.name),
                                                mmsg.clone(),
                                            );
                                        }
                                    }
                                }
                                // …and a struct's *fields*, for the same reason
                                // one level over: `Deprecated:` on a field is
                                // what the writer of `opts.Old = x` is warned
                                // about, and reading it needs the struct's own
                                // doc comments. Keyed by `Type.Field` rather
                                // than by the bare name, because a field called
                                // `Old` and a package-level `Old` are different
                                // objects that would otherwise collide.
                                if let Expr::StructType(st) = &ts.ty {
                                    for f in &st.fields.list {
                                        let Some(fmsg) = extract_deprecated_message(&f.doc)
                                        else {
                                            continue;
                                        };
                                        for name in &f.names {
                                            out.fields.insert(
                                                method_fact_key(&ts.name.name, &name.name),
                                                fmsg.clone(),
                                            );
                                        }
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
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut deprs = pass
        .result_of::<DeprecatedResult>(deprecated::analyzer())
        .cloned()
        .unwrap_or_default();

    for fact in pass.all_object_facts() {
        if let Some(d) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            deprs.objects.insert(fact.object, d.clone());
        }
    }
    for fact in pass.all_package_facts() {
        if let Some(d) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            deprs.packages.insert(fact.package, d.clone());
        }
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1019 requires inspect analyzer".to_string())?
        .clone();
    let _generated = pass.result_of::<generated::GeneratedResult>(generated::analyzer());

    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut current_fn: Option<guff_types::arena::ObjectId> = None;
    let mut dep_cache = DepDeprecatedCache::default();

    const WANTED: NodeMask = node_mask!(
        CompositeLit,
        FuncDecl,
        ImportSpec,
        SelectorExpr,
    );
    inspect.preorder_typed(WANTED, pass.files(), |node| {
        match node {
            NodeRef::FuncDecl(f) => {
                current_fn = pass
                    .types_info()
                    .and_then(|info| info.defs.get(&f.name.id).and_then(|o| *o));
            }
            NodeRef::SelectorExpr(sel) => {
                if let Some((pos, msg)) =
                    selector_diagnostic(pass, &deprs, &mut dep_cache, sel, current_fn)
                {
                    pending.push((pos, msg));
                }
            }
            NodeRef::CompositeLit(lit) => {
                for (pos, msg) in struct_lit_diagnostics(pass, &deprs, &mut dep_cache, lit, current_fn)
                {
                    pending.push((pos, msg));
                }
            }
            NodeRef::ImportSpec(spec) => {
                if let Some((pos, msg)) = import_diagnostic(pass, &deprs, &mut dep_cache, spec) {
                    pending.push((pos, msg));
                }
            }
            _ => {}
        }
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn selector_diagnostic(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    dep_cache: &mut DepDeprecatedCache,
    sel: &SelectorExpr,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Option<(u32, String)> {
    let info = pass.types_info()?;
    // Match go/types ObjectOf (Defs before Uses). For an embedded field
    // `pkg.T`, Defs maps T's Ident to the field Var while Uses maps it to the
    // TypeName — ObjectOf returns the Var, so embedding a deprecated type is
    // not reported (honnef SA1019 parity). Named fields / signatures still
    // resolve the type Ident via Uses → TypeName and are flagged.
    let obj = object_of(pass, &sel.sel)?;
    let pkg_path = object_pkg_path(pass, obj)?;
    if related_pkg_path(pass, &pkg_path) {
        return None;
    }
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    // Selections mark method picks; fall back to signature recv when the
    // selector wasn't recorded (some call shapes).
    // A *field* selection is recorded in `selections` exactly like a method
    // one, so "is there a selection" answers the wrong question: `a.Old` on a
    // struct field took the method branch, looked for `Options.Old` among the
    // methods, missed, and returned — which is why a deprecated field was
    // silent for every importer even once the scanner collected it.
    let sel_kind = info.selections.get(&sel.id).map(|s| s.kind());
    let is_field = sel_kind == Some(guff_types::selection::SelectionKind::FieldVal);
    let is_method = !is_field
        && (sel_kind.is_some() || func_has_receiver(&artifacts.types, &artifacts.objects, obj));
    let synthetic;
    let depr = if let Some(d) = deprs.objects.get(&obj) {
        d
    } else {
        // Lazy PARSE_COMMENTS reparse of dep sources. Do not scan every
        // same-module or third-party call site (prometheus cold wall). Allow:
        // - package already known-deprecated via facts,
        // - small third-party / nested-module allowlist,
        // - local replace / nested go.mod / testdata stubs.
        let obj_pkg = obj.pkg(&artifacts.objects)?;
        let allow_lazy = deprs.packages.contains_key(&obj_pkg)
            || worth_object_doc_scan(&pkg_path)
            || is_local_source_dep(pass, &pkg_path);
        if !allow_lazy {
            return None;
        }
        let name = obj.name(&artifacts.objects).to_string();
        let facts = dep_facts(pass, dep_cache, &pkg_path, true);
        let msg = if is_method {
            let recv = selection_recv_base_name(pass, sel).or_else(|| {
                method_recv_base_from_sig(&artifacts.types, &artifacts.objects, obj)
            })?;
            facts.methods.get(&method_fact_key(&recv, &name))
        } else if is_field {
            let recv = selection_recv_base_name(pass, sel)?;
            facts.fields.get(&method_fact_key(&recv, &name))
        } else {
            facts.objects.get(&name)
        }?
        .clone();
        synthetic = IsDeprecated { msg };
        &synthetic
    };
    // Stdlib table keyed by SelectorName; message uses source rendering (report.Render).
    let table_key = knowledge_selector_name(pass, sel);
    let display = render_expr(&Expr::SelectorExpr(sel.clone()));
    // Upstream passes the whole `*ast.SelectorExpr` to `report.Report`, so the
    // position is where `x` starts, not where the selected name does:
    // `lib.OldFunc` is reported at `lib`, `i.GetOld` at `i`.
    let pos = sel.x.pos().0 as u32;
    handle_deprecation(
        pass,
        deprs,
        depr,
        &table_key,
        &display,
        &pkg_path,
        pos,
        current_fn,
    )
    .map(|msg| (pos, msg))
}

fn struct_lit_diagnostics(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    dep_cache: &mut DepDeprecatedCache,
    lit: &CompositeLit,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Vec<(u32, String)> {
    let Some(typ_expr) = lit.ty.as_ref() else {
        return Vec::new();
    };
    let info = match pass.types_info() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let Some(tv) = info.types.get(&typ_expr.id()) else {
        return Vec::new();
    };
    if !matches!(
        artifacts.types.get(tv.typ.underlying(&artifacts.types)),
        TypeData::Struct(_)
    ) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = &*kv.key else {
            continue;
        };
        let sel = SelectorExpr {
            x: typ_expr.clone(),
            sel: key.clone(),
            id: 0,
        };
        if let Some(d) = selector_diagnostic(pass, deprs, dep_cache, &sel, current_fn) {
            out.push(d);
        }
    }
    out
}

fn import_diagnostic(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    dep_cache: &mut DepDeprecatedCache,
    spec: &ImportSpec,
) -> Option<(u32, String)> {
    let info = pass.types_info()?;
    // Explicit `import foo "path"` → defs[alias]; bare `import "path"` →
    // implicits keyed on the ImportSpec node (not the path BasicLit).
    let imp_obj = if let Some(name) = &spec.name {
        info.defs.get(&name.id).and_then(|o| *o)
    } else {
        info.implicits.get(&spec.id).copied()
    }?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::PkgName(pn) = artifacts.objects.get(imp_obj) else {
        return None;
    };
    let imported = artifacts.packages.get(pn.imported());
    let path = imported.path();
    if related_pkg_path(pass, path) {
        return None;
    }
    // Export-only stdlib deps never run `fact_deprecated`. Synthesize the
    // package fact from the knowledge table + frozen GOROOT package docs.
    // Third-party: package docs survive Mode::NONE, but fact remapping can
    // miss them — cheap Mode::NONE reparse of file docs only (not full
    // PARSE_COMMENTS object scan).
    let synthetic;
    let depr = if let Some(d) = deprs.packages.get(&pn.imported()) {
        d
    } else if let Some(msg) = stdlib_package_deprecation_msg(path) {
        if stdlib_deprecations().get(path).is_none() {
            return None;
        }
        synthetic = IsDeprecated {
            msg: msg.to_string(),
        };
        &synthetic
    } else if worth_package_doc_scan(path) || is_local_source_dep(pass, path) {
        // Cheap Mode::NONE package-doc reparse when fact remapping missed.
        // Gated: unrestricted third-party import scans dominate cold ./... wall.
        let Some(msg) = dep_facts(pass, dep_cache, path, false).package.clone() else {
            return None;
        };
        synthetic = IsDeprecated { msg };
        &synthetic
    } else {
        return None;
    };
    let p = spec.path.value.trim_matches('"');
    let pos = spec.path.value_pos.0 as u32;
    // Upstream reports the quoted import path via report.Render(spec.Path).
    let quoted = format!("\"{p}\"");
    handle_deprecation(pass, deprs, depr, p, &quoted, path, pos, None).map(|msg| (pos, msg))
}

fn sa1019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1019",
        doc: "using a deprecated function, variable, constant or field",
        url: "https://staticcheck.dev/docs/checks/#SA1019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![
            inspect::analyzer(),
            deprecated::analyzer(),
            generated::analyzer(),
        ],
        fact_types: vec![],
    }
}

/// SA1019 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
