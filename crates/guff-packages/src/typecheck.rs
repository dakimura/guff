//! Type-checking loaded packages from source with export-data dependencies.
//!
//! Port of golangci-lint `loadFromSource` / `loadFromExportData` and the
//! `types.Config` wiring in `go/packages`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_exportdata::ExportImporter;
use guff_types::api::Config as TypeConfig;
use guff_types::default_sizes;
use guff_types::sizes_for;
use guff_types::{Checker, ExportSeed};

use crate::load_mode::LoadMode;
use crate::package::{Error, ErrorKind, Package, TypecheckArtifacts};

/// Toolchain parameters used when type-checking loaded packages.
#[derive(Debug, Clone)]
pub struct TypecheckEnv {
    /// Compiler name passed to [`sizes_for`] (typically `"gc"`).
    pub compiler: String,
    /// Target GOARCH (e.g. `"amd64"`).
    pub arch: String,
    /// Accepted Go language version for the checker (e.g. `"go1.26"`).
    pub go_version: String,
    /// Resolve dependency type information by type-checking dependency **source**
    /// (via the built-in source importer) instead of decoding compiler export
    /// data (`.a`). Set on the cold path where `go list` runs without `-export`,
    /// so the expensive dependency compilation is avoided (see the cold-speedup
    /// work). Default `true` (hybrid); `false` forces the export-data seed.
    pub from_source: bool,
    /// Type-check target packages concurrently via rayon. Set `false` when the
    /// caller runs with `--sequential` / `-j 1` so dependency seeding and
    /// per-package checking stay on the main thread (avoids small default worker
    /// stacks overflowing on deep hybrid type-check trees).
    pub parallel: bool,
}

impl Default for TypecheckEnv {
    fn default() -> Self {
        Self {
            compiler: "gc".into(),
            arch: std::env::var("GOARCH").unwrap_or_else(|_| "amd64".into()),
            go_version: String::new(),
            from_source: true,
            parallel: true,
        }
    }
}

impl TypecheckEnv {
    /// Build from a subprocess environment slice (`KEY=value` strings).
    pub fn from_env(env: &[String], compiler: &str) -> Self {
        let lookup = |key: &str| -> Option<String> {
            env.iter().find_map(|entry| {
                let (k, v) = entry.split_once('=')?;
                (k == key).then(|| v.to_string())
            })
        };
        Self {
            compiler: if compiler.is_empty() {
                "gc".into()
            } else {
                compiler.to_string()
            },
            arch: lookup("GOARCH")
                .or_else(|| std::env::var("GOARCH").ok())
                .unwrap_or_else(|| "amd64".into()),
            go_version: lookup("GOVERSION")
                .or_else(|| std::env::var("GOVERSION").ok())
                .or_else(|| {
                    // `GOVERSION` is rarely exported in the process environment;
                    // fall back to `go env` so language-version gates (modernize,
                    // …) see the toolchain version when Module metadata is absent.
                    std::process::Command::new("go")
                        .args(["env", "GOVERSION"])
                        .output()
                        .ok()
                        .and_then(|o| {
                            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                            if s.is_empty() {
                                None
                            } else {
                                Some(s)
                            }
                        })
                })
                .unwrap_or_default(),
            from_source: false,
            parallel: true,
        }
    }

    pub fn sizes(&self) -> guff_types::Sizes {
        sizes_for(&self.compiler, &self.arch).unwrap_or_else(default_sizes)
    }
}

/// Returns true when the load mode requires parsing or type-checking.
pub fn needs_typecheck(mode: LoadMode) -> bool {
    mode.contains(LoadMode::NEED_TYPES)
        || mode.contains(LoadMode::NEED_SYNTAX)
        || mode.contains(LoadMode::NEED_TYPES_INFO)
}

/// Type-check packages in `by_id`, filling `Types`, `Syntax`, and related fields.
///
/// `root_ids` are the packages matched by the load patterns. When
/// [`LoadMode::NEED_DEPS`] is set, every package in `by_id` is type-checked;
/// otherwise only roots are checked from source (dependencies resolve via export
/// data through the configured importer).
pub fn typecheck_packages(
    by_id: &mut HashMap<String, Arc<Package>>,
    root_ids: &[String],
    mode: LoadMode,
    env: &TypecheckEnv,
) {
    if !needs_typecheck(mode) {
        return;
    }

    let fset = FileSet::new();
    let sizes = env.sizes();

    let export_paths = collect_export_paths(by_id);
    let dep_graph: HashMap<String, Vec<String>> = by_id
        .iter()
        .map(|(id, pkg)| (id.clone(), pkg.deps.clone()))
        .collect();

    let targets: Vec<String> = if mode.contains(LoadMode::NEED_DEPS) {
        by_id.keys().cloned().collect()
    } else {
        root_ids.to_vec()
    };

    // Build a shared dependency seed once (R24.3) so parallel checkers clone the
    // already-loaded stdlib/common deps instead of reloading per package. In
    // source mode the deps are type-checked from source; otherwise decoded from
    // export data.
    let seed = if env.from_source {
        build_source_seed(&targets, by_id, &export_paths, &dep_graph, &fset, env)
    } else {
        build_export_seed(&targets, by_id, &export_paths, &dep_graph, &fset, env)
    };

    // Type-check targets in parallel. Each package resolves its dependencies
    // from on-disk export data (`.a` files) via a private `Checker`/importer —
    // it never reads sibling packages out of `by_id` — so the targets are
    // independent and can be checked concurrently. The shared `FileSet` uses
    // interior locking, making concurrent parsing safe. Results are cloned out,
    // checked off-map, then written back, so the map is untouched during the
    // parallel phase.
    let checked: Vec<(String, Package)> = if env.parallel {
        targets
            .par_iter()
            .filter_map(|id| typecheck_one_target(id, by_id, &fset, &export_paths, &dep_graph, sizes, env, mode, seed.as_deref()))
            .collect()
    } else {
        targets
            .iter()
            .filter_map(|id| typecheck_one_target(id, by_id, &fset, &export_paths, &dep_graph, sizes, env, mode, seed.as_deref()))
            .collect()
    };

    for (id, pkg) in checked {
        by_id.insert(id, Arc::new(pkg));
    }
}

/// Type-check exactly `target_ids` from source, resolving dependencies from
/// on-disk export data. Unlike [`typecheck_packages`], this never expands to the
/// whole graph even when `mode` contains [`LoadMode::NEED_DEPS`] — it is the
/// lazy path used to type-check only the packages that missed the issues cache.
///
/// `all` must contain every loaded package (roots and transitive deps) so
/// export paths and the dependency graph are complete. Returns the type-checked
/// target packages, in `target_ids` order, each sharing one `FileSet`.
pub fn typecheck_roots(
    all: &[Arc<Package>],
    target_ids: &[String],
    mode: LoadMode,
    env: &TypecheckEnv,
) -> Vec<Arc<Package>> {
    if target_ids.is_empty() || !needs_typecheck(mode) {
        return Vec::new();
    }

    let by_id: HashMap<String, Arc<Package>> =
        all.iter().map(|p| (p.id.clone(), Arc::clone(p))).collect();

    let fset = FileSet::new();
    let sizes = env.sizes();
    let export_paths = collect_export_paths(&by_id);
    let dep_graph: HashMap<String, Vec<String>> = by_id
        .iter()
        .map(|(id, pkg)| (id.clone(), pkg.deps.clone()))
        .collect();

    let dbg = std::env::var_os("GUFF_DEBUG_CACHE").is_some();
    let tc_start;
    let mut checked: HashMap<String, Arc<Package>> = {
        let ts = std::time::Instant::now();
        let seed = if env.from_source {
            build_source_seed(target_ids, &by_id, &export_paths, &dep_graph, &fset, env)
        } else {
            build_export_seed(target_ids, &by_id, &export_paths, &dep_graph, &fset, env)
        };
        if dbg {
            eprintln!(
                "guff:   typecheck_roots seed build {:.2}s (from_source={})",
                ts.elapsed().as_secs_f64(),
                env.from_source,
            );
        }
        tc_start = std::time::Instant::now();
        if env.parallel {
            target_ids
                .par_iter()
                .filter_map(|id| {
                    typecheck_one_target_arc(
                        id,
                        &by_id,
                        &fset,
                        &export_paths,
                        &dep_graph,
                        sizes,
                        env,
                        mode,
                        seed.as_deref(),
                    )
                })
                .collect()
        } else {
            target_ids
                .iter()
                .filter_map(|id| {
                    typecheck_one_target_arc(
                        id,
                        &by_id,
                        &fset,
                        &export_paths,
                        &dep_graph,
                        sizes,
                        env,
                        mode,
                        seed.as_deref(),
                    )
                })
                .collect()
        }
    };
    if dbg {
        eprintln!(
            "guff:   typecheck_roots target check {:.2}s ({} targets)",
            tc_start.elapsed().as_secs_f64(),
            target_ids.len(),
        );
    }

    target_ids
        .iter()
        .filter_map(|id| checked.remove(id))
        .collect()
}

fn typecheck_one_target(
    id: &String,
    by_id: &HashMap<String, Arc<Package>>,
    fset: &Arc<FileSet>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    sizes: guff_types::Sizes,
    env: &TypecheckEnv,
    mode: LoadMode,
    seed: Option<&ExportSeed>,
) -> Option<(String, Package)> {
    let mut pkg = (**by_id.get(id)?).clone();
    typecheck_package_with_seed(
        &mut pkg,
        fset,
        export_paths,
        dep_graph,
        sizes,
        env,
        mode,
        seed,
    );
    Some((id.clone(), pkg))
}

fn typecheck_one_target_arc(
    id: &String,
    by_id: &HashMap<String, Arc<Package>>,
    fset: &Arc<FileSet>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    sizes: guff_types::Sizes,
    env: &TypecheckEnv,
    mode: LoadMode,
    seed: Option<&ExportSeed>,
) -> Option<(String, Arc<Package>)> {
    typecheck_one_target(
        id,
        by_id,
        fset,
        export_paths,
        dep_graph,
        sizes,
        env,
        mode,
        seed,
    )
    .map(|(id, pkg)| (id, Arc::new(pkg)))
}

fn collect_export_paths(by_id: &HashMap<String, Arc<Package>>) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for (id, pkg) in by_id {
        if pkg.export_file.as_os_str().is_empty() {
            continue;
        }
        if Path::new(&pkg.export_file).exists() {
            out.insert(id.clone(), pkg.export_file.clone());
        }
    }
    out
}

/// Type-check a single loaded package from its `compiled_go_files`.
///
/// Equivalent to [`typecheck_package_with_seed`] with no shared seed.
pub fn typecheck_package(
    pkg: &mut Package,
    fset: &Arc<FileSet>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    sizes: guff_types::Sizes,
    env: &TypecheckEnv,
    mode: LoadMode,
) {
    typecheck_package_with_seed(
        pkg,
        fset,
        export_paths,
        dep_graph,
        sizes,
        env,
        mode,
        None,
    );
}

/// Type-check a package, optionally cloning dependency arenas from `seed` (R24.3).
///
/// When `seed` is provided, dependency export data is cloned from the shared
/// seed instead of being re-decoded. An [`ExportImporter`] is still attached so
/// unexpected late imports can fall back to on-disk `.a` files.
///
/// DEFERRED(R24.2): per-file incremental typecheck. Checker is whole-package;
/// cross-file defs/methods/inits make partial `check_files` incorrect without a
/// dedicated incremental engine.
pub fn typecheck_package_with_seed(
    pkg: &mut Package,
    fset: &Arc<FileSet>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    sizes: guff_types::Sizes,
    env: &TypecheckEnv,
    mode: LoadMode,
    seed: Option<&ExportSeed>,
) {
    if pkg.pkg_path == "unsafe" {
        return;
    }

    let paths = &pkg.compiled_go_files;
    if paths.is_empty() {
        return;
    }

    let mut syntax = Vec::new();
    for path in paths {
        let src = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) => {
                pkg.errors.push(Error {
                    pos: path.display().to_string(),
                    msg: err.to_string(),
                    kind: ErrorKind::Parse,
                });
                pkg.ill_typed = true;
                continue;
            }
        };
        // Prefer a stable path string for diagnostics (compat/R21 diffs on
        // file:line:linter). Fall back to the basename only when the path is
        // not valid UTF-8.
        let name = path.to_str().unwrap_or_else(|| {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file.go")
        });
        match parse_file(fset, name, &src, Mode::NONE) {
            Ok(file) => syntax.push(file),
            Err(errs) => {
                pkg.ill_typed = true;
                for err in errs.iter() {
                    pkg.errors.push(Error {
                        pos: path.display().to_string(),
                        msg: err.to_string(),
                        kind: ErrorKind::Parse,
                    });
                }
            }
        }
    }

    if syntax.is_empty() {
        pkg.ill_typed = true;
        return;
    }

    let conf = TypeConfig {
        sizes: Some(sizes),
        go_version: env.go_version.clone(),
        ..TypeConfig::default()
    };
    let mut check = if let Some(seed) = seed {
        Checker::from_seed(seed, conf)
    } else {
        Checker::new(conf)
    };

    let mut importer = ExportImporter::with_fset(fset.clone());
    for (path, file) in export_paths {
        importer.set_path(path.clone(), file.clone());
    }
    check.set_importer(Box::new(importer));

    if seed.is_none() {
        let mut visiting = Vec::new();
        let mut done = std::collections::HashSet::new();
        preload_exports(
            &mut check,
            &pkg.deps,
            dep_graph,
            export_paths,
            &mut visiting,
            &mut done,
        );
    }

    let files = syntax;
    check.check_files(files);

    pkg.ill_typed = !check.errors.is_empty();
    for err in &check.errors {
        pkg.errors.push(Error {
            pos: if err.pos == 0 {
                String::new()
            } else {
                err.pos.to_string()
            },
            msg: err.msg.clone(),
            kind: ErrorKind::Type,
        });
    }

    if mode.contains(LoadMode::NEED_TYPES) {
        let info = if mode.contains(LoadMode::NEED_TYPES_INFO) {
            check.info.clone()
        } else {
            std::mem::take(&mut check.info)
        };
        pkg.types = Some(check.pkg);
        pkg.type_artifacts = Some(TypecheckArtifacts {
            type_pkg: check.pkg,
            types: check.types,
            objects: check.objects,
            scopes: check.scopes,
            packages: check.packages,
            info,
        });
    }
    if mode.contains(LoadMode::NEED_TYPES_INFO) {
        pkg.types_info = Some(check.info);
    }
    if mode.contains(LoadMode::NEED_SYNTAX) {
        pkg.syntax = std::mem::take(&mut check.files);
    }
    if mode.contains(LoadMode::NEED_TYPES)
        || mode.contains(LoadMode::NEED_SYNTAX)
        || mode.contains(LoadMode::NEED_TYPES_INFO)
    {
        pkg.fset = Some(fset.clone());
    }
    if mode.contains(LoadMode::NEED_TYPES_SIZES) {
        pkg.types_sizes = Some(sizes);
    }
}

/// Build a shared [`ExportSeed`] covering the union of `targets`' dependency
/// graphs. Returns `None` when there is nothing useful to preload.
fn build_export_seed(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    fset: &Arc<FileSet>,
    env: &TypecheckEnv,
) -> Option<Arc<ExportSeed>> {
    let mut needed: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for id in targets {
        let Some(pkg) = by_id.get(id) else {
            continue;
        };
        for dep in &pkg.deps {
            if seen.insert(dep.clone()) {
                needed.push(dep.clone());
            }
        }
        // Direct imports may not always appear in deps (e.g. incomplete list).
        for path in pkg.imports.keys() {
            if seen.insert(path.clone()) {
                needed.push(path.clone());
            }
        }
    }
    needed.retain(|p| p != "unsafe" && p != "C" && export_paths.contains_key(p));
    if needed.is_empty() {
        return None;
    }
    needed.sort();

    let conf = TypeConfig {
        sizes: Some(env.sizes()),
        go_version: env.go_version.clone(),
        ..TypeConfig::default()
    };
    let mut check = Checker::new(conf);
    let mut importer = ExportImporter::with_fset(fset.clone());
    for (path, file) in export_paths {
        importer.set_path(path.clone(), file.clone());
    }
    check.set_importer(Box::new(importer));
    let mut visiting = Vec::new();
    let mut done = std::collections::HashSet::new();
    preload_exports(
        &mut check,
        &needed,
        dep_graph,
        export_paths,
        &mut visiting,
        &mut done,
    );
    Some(Arc::new(check.capture_export_seed()))
}

/// Build a shared [`ExportSeed`] by type-checking the targets' dependency
/// closure **from source** (no export data), for the cold path where `go list`
/// runs without `-export`. Returns `None` when there is nothing to preload.
///
/// The resulting seed is the same type as [`build_export_seed`]'s, so every
/// downstream consumer ([`typecheck_package_with_seed`], the SSA layer, …) is
/// unchanged — it simply contains source-checked dependencies instead of
/// export-decoded ones. Diagnostics produced while checking the dependencies are
/// intentionally dropped ([`Checker::capture_export_seed`] does not capture
/// errors); only the target packages report issues.
fn build_source_seed(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    fset: &Arc<FileSet>,
    env: &TypecheckEnv,
) -> Option<Arc<ExportSeed>> {
    // Transitive dependency closure of the targets (leaves included).
    let mut needed: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut stack: Vec<String> = Vec::new();
    for id in targets {
        if let Some(pkg) = by_id.get(id) {
            stack.extend(pkg.deps.iter().cloned());
            stack.extend(pkg.imports.keys().cloned());
        }
    }
    while let Some(path) = stack.pop() {
        if path == "unsafe" || path == "C" {
            continue;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        needed.push(path.clone());
        if let Some(deps) = dep_graph.get(&path) {
            stack.extend(deps.iter().cloned());
        }
    }
    // Keep dependencies we can load: either from export data (stdlib, hybrid
    // mode) or from source (third-party, and everything in pure-source mode).
    needed.retain(|p| {
        export_paths.contains_key(p)
            || by_id
                .get(p)
                .is_some_and(|pk| !pk.compiled_go_files.is_empty())
    });
    needed.sort();
    if needed.is_empty() {
        return None;
    }

    let conf = TypeConfig {
        sizes: Some(env.sizes()),
        go_version: env.go_version.clone(),
        ..TypeConfig::default()
    };
    let mut check = Checker::new(conf);
    // Dependencies only need their exported API in the seed; importers never see
    // dep function bodies. Skipping body checks here is a large speedup and does
    // not change any target package's findings (targets are checked with full
    // bodies via a fresh `from_seed` checker). Dep diagnostics are dropped anyway.
    check.set_ignore_func_bodies(true);

    // Export-data importer for dependencies that keep export data (stdlib in the
    // hybrid path; empty in pure-source). The built-in source importer takes
    // precedence for any path also registered via `add_dependency_source`.
    let mut importer = ExportImporter::with_fset(fset.clone());
    for (path, file) in export_paths {
        importer.set_path(path.clone(), file.clone());
    }
    check.set_importer(Box::new(importer));

    // Load each dependency once, leaves-first. Type-checking a dependency into
    // the shared arenas is inherently serial, but *parsing* its source is not.
    // Compute the leaves-first order once, then process it in bounded chunks:
    // parse the source deps of a chunk in parallel, then type-check/decode the
    // chunk in order. Peak resident dep-AST is one chunk (SEED_PARSE_CHUNK), so
    // we never hold the whole closure at once (keeping the hybrid RSS budget)
    // while still overlapping the serial checker with parallel parsing.
    let loadable: std::collections::HashSet<String> = needed.iter().cloned().collect();
    let order = dep_load_order(&needed, dep_graph, &loadable);
    const SEED_PARSE_CHUNK: usize = 64;
    for chunk in order.chunks(SEED_PARSE_CHUNK) {
        // Parse (in parallel) every source dependency in this chunk. Export-data
        // deps have no source to parse and are skipped here.
        let mut parsed: HashMap<String, Vec<guff::ast::File>> = chunk
            .par_iter()
            .filter(|path| !export_paths.contains_key(*path))
            .filter_map(|path| {
                let pkg = by_id.get(path)?;
                if pkg.compiled_go_files.is_empty() {
                    return None;
                }
                Some((path.clone(), parse_dep_files(&pkg.compiled_go_files, fset)))
            })
            .collect();

        for path in chunk {
            if let Some(files) = parsed.remove(path) {
                if !files.is_empty() {
                    check.add_dependency_source(path.clone(), files);
                }
            }
            check.preload_import(path);
            // Seed path: dependency diagnostics are intentionally dropped; do not
            // let them accumulate in the checker while walking the closure.
            check.errors.clear();
            check.first_err = None;
        }
    }
    Some(Arc::new(check.capture_export_seed()))
}

/// Leaves-first (post-order) topological order over the loadable dependency
/// closure: a package's deps are always emitted before the package itself, and
/// `deps` are walked in sorted order for determinism (so the seed is built in a
/// stable order regardless of `HashMap` iteration). `unsafe`/`C` and
/// non-loadable paths are skipped. The
/// graph is a DAG (Go forbids import cycles); a stack guard keeps a malformed
/// graph from recursing forever.
fn dep_load_order(
    needed: &[String],
    dep_graph: &HashMap<String, Vec<String>>,
    loadable: &std::collections::HashSet<String>,
) -> Vec<String> {
    let mut order = Vec::new();
    let mut done = std::collections::HashSet::new();
    let mut visiting: Vec<String> = Vec::new();
    for id in needed {
        dep_load_order_visit(id, dep_graph, loadable, &mut done, &mut visiting, &mut order);
    }
    order
}

fn dep_load_order_visit(
    path: &str,
    dep_graph: &HashMap<String, Vec<String>>,
    loadable: &std::collections::HashSet<String>,
    done: &mut std::collections::HashSet<String>,
    visiting: &mut Vec<String>,
    order: &mut Vec<String>,
) {
    if path == "unsafe" || path == "C" || done.contains(path) || !loadable.contains(path) {
        return;
    }
    if visiting.iter().any(|p| p == path) {
        return;
    }
    visiting.push(path.to_string());
    if let Some(deps) = dep_graph.get(path) {
        let mut deps: Vec<&str> = deps.iter().map(String::as_str).collect();
        deps.sort_unstable();
        for dep in deps {
            dep_load_order_visit(dep, dep_graph, loadable, done, visiting, order);
        }
    }
    visiting.pop();
    if done.insert(path.to_string()) {
        order.push(path.to_string());
    }
}

/// Parse a dependency's `compiled_go_files` into syntax trees sharing `fset`.
/// Files that fail to read or parse are skipped (dependency diagnostics are not
/// reported on the source-seed path).
fn parse_dep_files(paths: &[PathBuf], fset: &Arc<FileSet>) -> Vec<guff::ast::File> {
    let mut out = Vec::new();
    for path in paths {
        let Ok(src) = fs::read(path) else { continue };
        let name = path.to_str().unwrap_or("file.go");
        if let Ok(file) = parse_file(fset, name, &src, Mode::NONE) {
            out.push(file);
        }
    }
    out
}


/// Depth-first preload of dependency export data so nested `read()` calls find
/// transitive packages in the importer cache (see PL09 deferral).
///
/// `done` memoizes packages whose entire subtree has already been preloaded.
/// Without it, a dependency graph with heavy fan-in (every package importing a
/// handful of common ones) is walked an exponential number of times — each
/// distinct root→leaf path re-descends shared subtrees. On large modules
/// (e.g. Prometheus) that alone pushed a run past its timeout. With the memo,
/// each node is visited once: O(V+E).
fn preload_exports(
    check: &mut Checker,
    deps: &[String],
    dep_graph: &HashMap<String, Vec<String>>,
    export_paths: &HashMap<String, PathBuf>,
    visiting: &mut Vec<String>,
    done: &mut std::collections::HashSet<String>,
) {
    for dep in deps {
        preload_export(check, dep, dep_graph, export_paths, visiting, done);
    }
}

fn preload_export(
    check: &mut Checker,
    path: &str,
    dep_graph: &HashMap<String, Vec<String>>,
    export_paths: &HashMap<String, PathBuf>,
    visiting: &mut Vec<String>,
    done: &mut std::collections::HashSet<String>,
) {
    if path == "unsafe" || path == "C" {
        return;
    }
    // Whole subtree already preloaded via another path — skip. This is what
    // collapses the exponential DAG walk to linear.
    if done.contains(path) {
        return;
    }
    if visiting.iter().any(|p| p == path) {
        return;
    }
    if !export_paths.contains_key(path) {
        return;
    }
    visiting.push(path.to_string());
    if let Some(deps) = dep_graph.get(path) {
        preload_exports(check, deps, dep_graph, export_paths, visiting, done);
    }
    check.preload_import(path);
    visiting.pop();
    done.insert(path.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use guff::ast::Decl;

    fn testdata(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/typecheck")
            .join(name)
    }

    fn package_from_dir(id: &str, dir: &Path) -> Package {
        let go_files: Vec<PathBuf> = fs::read_dir(dir)
            .expect("read dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("go"))
            .collect();
        Package {
            id: id.into(),
            pkg_path: id.into(),
            name: if id.contains("invalid") {
                "invalid".into()
            } else {
                "main".into()
            },
            dir: dir.to_path_buf(),
            compiled_go_files: go_files.clone(),
            go_files,
            ..Package::default()
        }
    }

    #[test]
    fn typecheck_valid_main_package() {
        let dir = testdata("valid");
        let mut pkg = package_from_dir("example.com/valid", &dir);
        let fset = FileSet::new();
        let mode = LoadMode::LOAD_SYNTAX;
        typecheck_package(
            &mut pkg,
            &fset,
            &HashMap::new(),
            &HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            mode,
        );
        assert!(!pkg.ill_typed, "errors: {:?}", pkg.errors);
        assert!(pkg.types.is_some());
        let info = pkg.types_info.as_ref().expect("types info");
        let file = pkg.syntax.first().expect("syntax");
        let main_id = file
            .decls
            .iter()
            .find_map(|d| match d {
                Decl::FuncDecl(fd) if fd.name.name == "main" => Some(fd.name.id),
                _ => None,
            })
            .expect("main func");
        assert!(
            info.defs.contains_key(&main_id),
            "main should appear in TypesInfo.defs"
        );
    }

    #[test]
    fn typecheck_invalid_package_is_ill_typed() {
        let dir = testdata("invalid");
        let mut pkg = package_from_dir("example.com/invalid", &dir);
        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &HashMap::new(),
            &HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        assert!(pkg.ill_typed);
        assert!(pkg.errors.iter().any(|e| e.kind == ErrorKind::Type));
    }

    #[test]
    fn typecheck_env_sizes_follows_goarch() {
        let env = TypecheckEnv {
            compiler: "gc".into(),
            arch: "386".into(),
            go_version: String::new(),
            from_source: false,
            parallel: true,
        };
        assert_eq!(env.sizes().word_size, 4);
    }

    #[test]
    fn typecheck_with_export_dependency() {
        let dep_export = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-exportdata/tests/testdata/export/simple/simple.a");

        let dir = testdata("withdep");
        let mut pkg = package_from_dir("example.com/withdep", &dir);
        let dep_id = "example.com/simple".to_string();
        pkg.deps = vec![dep_id.clone()];
        pkg.imports.insert(
            dep_id.clone(),
            Arc::new(Package {
                id: dep_id.clone(),
                pkg_path: dep_id.clone(),
                ..Package::default()
            }),
        );

        let mut export_paths = HashMap::new();
        export_paths.insert(dep_id.clone(), dep_export);

        let dep_graph = HashMap::from([(dep_id.clone(), Vec::<String>::new())]);

        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &export_paths,
            &dep_graph,
            default_sizes(),
            &TypecheckEnv {
                from_source: false,
                ..TypecheckEnv::default()
            },
            LoadMode::LOAD_SYNTAX,
        );
        assert!(
            !pkg.ill_typed,
            "typecheck with export dep failed: {:?}",
            pkg.errors
        );
    }

    #[test]
    fn export_seed_roundtrip_matches_unseeded() {
        let dep_export = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-exportdata/tests/testdata/export/simple/simple.a");
        let dir = testdata("withdep");
        let dep_id = "example.com/simple".to_string();

        let mut export_paths = HashMap::new();
        export_paths.insert(dep_id.clone(), dep_export);
        let dep_graph = HashMap::from([(dep_id.clone(), Vec::<String>::new())]);

        let mut by_id = HashMap::new();
        let mut pkg_a = package_from_dir("example.com/withdep_a", &dir);
        pkg_a.id = "example.com/withdep_a".into();
        pkg_a.pkg_path = "example.com/withdep_a".into();
        pkg_a.deps = vec![dep_id.clone()];
        by_id.insert(pkg_a.id.clone(), Arc::new(pkg_a));

        let fset = FileSet::new();
        let env = TypecheckEnv {
            from_source: false,
            ..TypecheckEnv::default()
        };
        let seed = build_export_seed(
            &["example.com/withdep_a".into()],
            &by_id,
            &export_paths,
            &dep_graph,
            &fset,
            &env,
        )
        .expect("seed");
        assert!(seed.cached_import_count() >= 1);

        let mut seeded = (**by_id.get("example.com/withdep_a").unwrap()).clone();
        typecheck_package_with_seed(
            &mut seeded,
            &fset,
            &export_paths,
            &dep_graph,
            default_sizes(),
            &env,
            LoadMode::LOAD_SYNTAX,
            Some(seed.as_ref()),
        );
        assert!(!seeded.ill_typed, "seeded: {:?}", seeded.errors);

        let mut plain = package_from_dir("example.com/withdep_b", &dir);
        plain.deps = vec![dep_id];
        typecheck_package(
            &mut plain,
            &fset,
            &export_paths,
            &dep_graph,
            default_sizes(),
            &env,
            LoadMode::LOAD_SYNTAX,
        );
        assert!(!plain.ill_typed, "plain: {:?}", plain.errors);
    }
}
