//! Type-checking loaded packages from source with export-data dependencies.
//!
//! Port of golangci-lint `loadFromSource` / `loadFromExportData` and the
//! `types.Config` wiring in `go/packages`.

use crate::hash::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;

use guff::parser::{parse_file, Mode, SKIP_FUNC_BODIES, SKIP_OBJECT_RESOLUTION};
use guff::position::FileSet;
use guff_exportdata::ExportImporter;
use guff_types::api::Config as TypeConfig;
use guff_types::default_sizes;
use guff_types::sizes_for;
use guff_types::{Checker, ExportSeed, WorkerOverlays};

use crate::load_mode::LoadMode;
use crate::package::{Error, ErrorKind, Package, TypecheckArtifacts};

/// Sub-phase accounting inside [`typecheck_package_with_seed`], summed across
/// rayon workers (so the totals are CPU, not wall — `PERF_TASKS.md` §1.6).
///
/// Only written when `GUFF_DEBUG_CACHE` is at level 2. The counters are global
/// and monotonic; callers snapshot them around the window they care about and
/// diff (see `typecheck_roots`), which also keeps repeated calls in one process
/// from mixing.
mod detail {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    pub(super) static READ_NS: AtomicU64 = AtomicU64::new(0);
    pub(super) static PARSE_NS: AtomicU64 = AtomicU64::new(0);
    pub(super) static SEED_NS: AtomicU64 = AtomicU64::new(0);
    pub(super) static CHECK_NS: AtomicU64 = AtomicU64::new(0);

    /// `Some(Instant)` only when level-2 accounting is on, so an unset
    /// `GUFF_DEBUG_CACHE` costs no clock read even on the per-file paths.
    pub(super) fn start(on: bool) -> Option<Instant> {
        on.then(Instant::now)
    }

    pub(super) fn add(counter: &AtomicU64, since: Option<Instant>) {
        if let Some(t) = since {
            counter.fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }
    }

    /// (read, parse, seed, check) totals so far.
    pub(super) fn snapshot() -> [Duration; 4] {
        [&READ_NS, &PARSE_NS, &SEED_NS, &CHECK_NS]
            .map(|c| Duration::from_nanos(c.load(Ordering::Relaxed)))
    }
}

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
    /// Skip AST object resolution ([`SKIP_OBJECT_RESOLUTION`]) when parsing
    /// target packages. Safe when no enabled analyzer reads `Ident.obj`
    /// (today: `ineffassign`, `maintidx`). The type checker uses stamped node
    /// ids + `Info` maps, not `Ident.obj`. Default `false` (resolve).
    pub skip_object_resolution: bool,
}

impl Default for TypecheckEnv {
    fn default() -> Self {
        Self {
            compiler: "gc".into(),
            arch: std::env::var("GOARCH").unwrap_or_else(|_| "amd64".into()),
            go_version: String::new(),
            from_source: true,
            parallel: true,
            skip_object_resolution: false,
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
                .filter(|s| !s.is_empty())
                .unwrap_or_else(crate::golist::detect_go_version_string),
            from_source: false,
            parallel: true,
            skip_object_resolution: false,
        }
    }

    pub fn sizes(&self) -> guff_types::Sizes {
        sizes_for(&self.compiler, &self.arch).unwrap_or_else(default_sizes)
    }
}

/// `GUFF_DEBUG_SEED_ERRORS=1`: print the first type error of each *dependency*
/// the source seed checks. Read once — `check_sources` runs per source dep, and
/// a dependency closure is hundreds of them.
fn seed_errors_enabled() -> bool {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os("GUFF_DEBUG_SEED_ERRORS").is_some());
    *ON
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
    let dep_graph = crate::dedup::import_path_dep_graph(by_id);

    let mut targets: Vec<String> = if mode.contains(LoadMode::NEED_DEPS) {
        by_id.keys().cloned().collect()
    } else {
        root_ids.to_vec()
    };
    // FxHashMap iteration order is deterministic but differs from SipHash;
    // keep sequential typecheck order stable for FileSet positions.
    targets.sort();

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
    typecheck_roots_with_prebuilt_seed(all, target_ids, mode, env, None)
}

/// Like [`typecheck_roots`], but reuses a speculative seed when `prebuilt` is
/// `Some` (PERF_TASKS_V2 C-7). The seed's [`FileSet`] is kept alive for import
/// positions; targets are parsed into that same set.
pub fn typecheck_roots_with_prebuilt_seed(
    all: &[Arc<Package>],
    target_ids: &[String],
    mode: LoadMode,
    env: &TypecheckEnv,
    prebuilt: Option<(Arc<ExportSeed>, Arc<FileSet>)>,
) -> Vec<Arc<Package>> {
    if target_ids.is_empty() || !needs_typecheck(mode) {
        return Vec::new();
    }

    let by_id: HashMap<String, Arc<Package>> =
        all.iter().map(|p| (p.id.clone(), Arc::clone(p))).collect();

    let (fset, prebuilt_seed) = match prebuilt {
        Some((seed, fset)) => (fset, Some(seed)),
        None => (FileSet::new(), None),
    };
    let sizes = env.sizes();
    let export_paths = collect_export_paths(&by_id);
    let dep_graph = crate::dedup::import_path_dep_graph(&by_id);

    let dbg = crate::debug::enabled();
    let acct = crate::debug::detailed();
    let tc_start;
    let acct_before;
    let mut checked: HashMap<String, Arc<Package>> = {
        let ts = std::time::Instant::now();
        let seed = if let Some(s) = prebuilt_seed {
            if dbg {
                eprintln!(
                    "guff:   typecheck_roots seed build {:.2}s (from_source={}, prebuilt)",
                    ts.elapsed().as_secs_f64(),
                    env.from_source,
                );
            }
            Some(s)
        } else if env.from_source {
            let s = build_source_seed(target_ids, &by_id, &export_paths, &dep_graph, &fset, env);
            if dbg {
                eprintln!(
                    "guff:   typecheck_roots seed build {:.2}s (from_source={})",
                    ts.elapsed().as_secs_f64(),
                    env.from_source,
                );
            }
            s
        } else {
            let s = build_export_seed(target_ids, &by_id, &export_paths, &dep_graph, &fset, env);
            if dbg {
                eprintln!(
                    "guff:   typecheck_roots seed build {:.2}s (from_source={})",
                    ts.elapsed().as_secs_f64(),
                    env.from_source,
                );
            }
            s
        };
        if acct {
            if let Some(ref s) = seed {
                let st = s.types().structural_dup_stats();
                eprintln!(
                    "guff:   type arena structural dups (seed): types={} structural={} unique={} \
                     dup_rate={:.1}%  ptr={}/{} slice={}/{} array={}/{} map={}/{} chan={}/{} sig={}/{}",
                    st.total_types,
                    st.structural,
                    st.unique_structural,
                    st.dup_rate() * 100.0,
                    st.pointer.0,
                    st.pointer.1,
                    st.slice.0,
                    st.slice.1,
                    st.array.0,
                    st.array.1,
                    st.map.0,
                    st.map.1,
                    st.chan.0,
                    st.chan.1,
                    st.signature.0,
                    st.signature.1,
                );
            }
        }
        // Taken after the seed build so the diff below covers only the target
        // packages. The seed builders drive `Checker` directly and do not touch
        // these counters, but `typecheck_packages` (via `refine`) does, so
        // starting from a snapshot rather than zero is what makes this correct.
        acct_before = detail::snapshot();
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
    if acct {
        let after = detail::snapshot();
        let [read, parse, seed, check] =
            std::array::from_fn(|i| (after[i] - acct_before[i]).as_secs_f64());
        eprintln!(
            "guff:     target check read {read:.2}s / parse {parse:.2}s / seed-clone {seed:.2}s \
             / check_files {check:.2}s (summed across workers)",
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
    let mut out = HashMap::default();
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
        // Ensure `save_to_cache` can persist an empty entry (it skips pkgs
        // without an fset). Empty packages must not perpetual-miss the cache.
        if pkg.fset.is_none() {
            pkg.fset = Some(FileSet::new());
        }
        return;
    }

    let acct = crate::debug::detailed();
    let mut syntax = Vec::new();
    let mut source_files: Vec<Arc<[u8]>> = Vec::new();
    for path in paths {
        let t_read = detail::start(acct);
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
        detail::add(&detail::READ_NS, t_read);
        // Prefer a stable path string for diagnostics (compat/R21 diffs on
        // file:line:linter). Fall back to the basename only when the path is
        // not valid UTF-8.
        let name = path.to_str().unwrap_or_else(|| {
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("file.go")
        });
        let t_parse = detail::start(acct);
        // Object resolution fills `Ident.obj`. Only ineffassign / maintidx read
        // it; when the caller opts out via `env.skip_object_resolution`, skip
        // the walk (P0-3). Type checking still stamps node ids below.
        let parse_mode = if env.skip_object_resolution {
            SKIP_OBJECT_RESOLUTION
        } else {
            Mode::NONE
        };
        let parsed = parse_file(fset, name, &src, parse_mode);
        detail::add(&detail::PARSE_NS, t_parse);
        match parsed {
            Ok(file) => {
                syntax.push(file);
                source_files.push(Arc::<[u8]>::from(src));
            }
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
    let t_seed = detail::start(acct);
    let mut check = if let Some(seed) = seed {
        Checker::from_seed(seed, conf)
    } else {
        Checker::new(conf)
    };
    // Checker allocates the package under check with an empty path; set the
    // real import path so `Object.Pkg().Path()` / `type_func_name` match go/types
    // (needed for cross-package facts like contextcheck).
    if !pkg.pkg_path.is_empty() {
        check.packages.get_mut(check.pkg).set_path(pkg.pkg_path.clone());
    }

    let mut importer = ExportImporter::with_fset(fset.clone());
    for (path, file) in export_paths {
        importer.set_path(path.clone(), file.clone());
    }
    check.set_importer(Box::new(importer));

    if seed.is_none() {
        let mut visiting = Vec::new();
        let mut done = HashSet::default();
        preload_exports(
            &mut check,
            &pkg.deps,
            dep_graph,
            export_paths,
            &mut visiting,
            &mut done,
        );
    }
    detail::add(&detail::SEED_NS, t_seed);

    let files = syntax;
    let t_check = detail::start(acct);
    check.check_files(files);
    detail::add(&detail::CHECK_NS, t_check);

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

    if mode.contains(LoadMode::NEED_TYPES) || mode.contains(LoadMode::NEED_TYPES_INFO) {
        // One Arc for both consumers: type_artifacts (SSA/buildir) and
        // types_info (analyzers). Avoids the former deep-clone of Info when
        // both flags are set (the common lint path).
        let info = std::sync::Arc::new(std::mem::take(&mut check.info));
        if mode.contains(LoadMode::NEED_TYPES) {
            pkg.types = Some(check.pkg);
            pkg.type_artifacts = Some(TypecheckArtifacts {
                type_pkg: check.pkg,
                types: check.types,
                objects: check.objects,
                scopes: check.scopes,
                packages: check.packages,
                info: std::sync::Arc::clone(&info),
            });
        }
        if mode.contains(LoadMode::NEED_TYPES_INFO) {
            pkg.types_info = Some(info);
        }
    }
    if mode.contains(LoadMode::NEED_SYNTAX) {
        pkg.syntax = std::mem::take(&mut check.files);
        // Keep bytes aligned with the syntax we handed the checker (same order
        // as successful parses). Length may differ from `compiled_go_files`
        // when some paths failed to parse.
        pkg.source_files = source_files;
        if pkg.source_files.len() > pkg.syntax.len() {
            pkg.source_files.truncate(pkg.syntax.len());
        }
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
    let mut seen = HashSet::default();
    for id in targets {
        let Some(pkg) = by_id.get(id) else {
            continue;
        };
        for dep in &pkg.deps {
            let dep = crate::dedup::import_path_of_id(dep).to_string();
            if seen.insert(dep.clone()) {
                needed.push(dep);
            }
        }
        // Direct imports may not always appear in deps (e.g. incomplete list).
        for path in pkg.imports.keys() {
            let path = crate::dedup::import_path_of_id(path).to_string();
            if seen.insert(path.clone()) {
                needed.push(path);
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
    let mut done = HashSet::default();
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

/// The loaded package whose files the seed compiles for import path `path`,
/// paired with whether its `_test.go` files are part of that compilation.
///
/// Production files only, with one exception. When `augment` holds `path` — the
/// load contains `path`'s *external* test package — the seed compiles the
/// same-package test variant `P [P.test]` instead: P's own files **plus** its
/// in-package `_test.go`. That is the package `import ".../p"` names inside P's
/// test binary, and widening it is the whole purpose of `export_test.go`, so a
/// production-only P leaves `package p_test` staring at `undefined:` and the
/// external test package goes ill-typed — which runs no analyzers at all.
///
/// Everything else stays production-only on purpose: `_test.go` across the
/// whole seed roughly doubles type-arena RSS on prometheus `./...`.
fn seed_package_for<'a>(
    by_id: &'a HashMap<String, Arc<Package>>,
    path: &str,
    augment: &HashSet<String>,
) -> Option<(&'a Arc<Package>, bool)> {
    if augment.contains(path) {
        if let Some(variant) = by_id.get(&crate::dedup::same_package_test_variant_id(path)) {
            if !variant.compiled_go_files.is_empty() {
                return Some((variant, true));
            }
        }
    }
    crate::dedup::package_for_import_path(by_id, path).map(|pkg| (pkg, false))
}

fn is_test_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_test.go"))
}

/// Whether [`seed_package_for`] leaves the seed anything to compile for `path`.
fn seed_has_source_files(
    by_id: &HashMap<String, Arc<Package>>,
    path: &str,
    augment: &HashSet<String>,
) -> bool {
    seed_package_for(by_id, path, augment).is_some_and(|(pkg, with_tests)| {
        pkg.compiled_go_files
            .iter()
            .any(|f| with_tests || !is_test_file(f))
    })
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
    build_source_seed_inner(targets, by_id, export_paths, dep_graph, fset, env)
}

/// Public entry for C-7 speculative prewarm ([`crate::speculate`]).
pub(crate) fn build_source_seed_for_speculate(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    fset: &Arc<FileSet>,
    env: &TypecheckEnv,
) -> Option<Arc<ExportSeed>> {
    build_source_seed_inner(targets, by_id, export_paths, dep_graph, fset, env)
}

fn build_source_seed_inner(
    targets: &[String],
    by_id: &HashMap<String, Arc<Package>>,
    export_paths: &HashMap<String, PathBuf>,
    dep_graph: &HashMap<String, Vec<String>>,
    fset: &Arc<FileSet>,
    env: &TypecheckEnv,
) -> Option<Arc<ExportSeed>> {
    // Which import paths the seed must compile *with* their in-package tests.
    // Derived from `by_id` rather than passed in, so every consumer of the seed
    // agrees with `import_path_dep_graph`, which reads the same set.
    let augment = crate::dedup::paths_with_external_test_package(by_id);

    // Transitive dependency closure of the targets (leaves included).
    let mut needed: Vec<String> = Vec::new();
    let mut seen = HashSet::default();
    let mut stack: Vec<String> = Vec::new();
    for id in targets {
        if let Some(pkg) = by_id.get(id) {
            // `deps` holds go list **ids**, and an external test package's are
            // bracketed (`Q [P.test]`). Everything downstream — the wave order,
            // the seed's package registry, the importer every dependency's own
            // `import` statement goes through — is keyed by import path, so a
            // bracketed id here seeds a package under a name no source file can
            // name. See `dedup::import_path_of_id`.
            stack.extend(pkg.deps.iter().map(|d| crate::dedup::import_path_of_id(d).to_string()));
            stack.extend(
                pkg.imports
                    .keys()
                    .map(|p| crate::dedup::import_path_of_id(p).to_string()),
            );
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
    // After filter_duplicate_packages, plain `P` is gone and only `P [P.test]`
    // remains — resolve by pkg_path so importers (e.g. consul flags→QF1012) see
    // real types. Which files that resolution yields is `seed_package_for`'s
    // call: production only, except for the packages whose external test
    // package is in this load.
    needed.retain(|p| export_paths.contains_key(p) || seed_has_source_files(by_id, p, &augment));
    needed.sort();
    if needed.is_empty() {
        return None;
    }

    let loadable: HashSet<String> = needed.iter().cloned().collect();
    let timing = crate::debug::enabled();
    // GUFF_DEBUG_CACHE=2 only: time each dep so the wave schedule can be scored
    // against the two bounds that matter — `sum/threads` (perfectly balanced,
    // no barriers) and the dependency critical path (the floor no schedule can
    // beat). Without both, "the barriers cost X" is a guess. One Instant pair
    // per dep, so it stays off the default path but costs nothing when on.
    let acct = crate::debug::detailed();
    let t_check_start = std::time::Instant::now();

    let make_conf = || TypeConfig {
        sizes: Some(env.sizes()),
        go_version: env.go_version.clone(),
        ..TypeConfig::default()
    };
    let make_importer = || {
        let mut importer = ExportImporter::with_fset(fset.clone());
        for (path, file) in export_paths {
            importer.set_path(path.clone(), file.clone());
        }
        importer
    };

    // Phase A (serial): decode all export-data dependencies (stdlib in the hybrid
    // path; the whole closure in export mode) into a base seed `S0`. Source deps
    // resolve these from the seed's import cache, so no source worker re-decodes
    // export data (which would duplicate a package's origin objects across
    // workers and break cross-package type identity).
    let mut check = Checker::new(make_conf());
    check.set_importer(Box::new(make_importer()));
    {
        let mut visiting = Vec::new();
        let mut done = HashSet::default();
        preload_exports(
            &mut check,
            &needed,
            dep_graph,
            export_paths,
            &mut visiting,
            &mut done,
        );
    }
    // Export-decode diagnostics are not seed output.
    check.errors.clear();
    check.first_err = None;
    let mut seed = check.capture_export_seed();

    // Snapshot S0 (export-data base) fingerprint inputs before any source merge.
    let s0_lens = seed.arena_lens();
    let s0_import_paths = seed.sorted_import_paths();

    // Group the *source* dependencies into topological waves. Export-data deps
    // are already in `S0` (treated as wave 0). Packages sharing a wave never
    // import one another, so they can be type-checked in parallel and merged
    // with a uniform id relocation (see `ExportSeed::merge_wave`).
    //
    // Waves are assigned **as late as possible**: `wave(P) = depth - height(P)`,
    // where `height(P)` is the longest chain of source packages that depends on
    // `P`. Assigning as *early* as possible (`1 + max(wave(dep))`) is equally
    // valid — both put every dependency in a strictly earlier wave — but far
    // worse balanced. A wave costs `max(total / threads, largest package)`, so a
    // wave holding one huge package idles every other core for its duration.
    // The expensive packages here are generated cloud SDKs that nothing else in
    // the dependency graph imports (only the targets do), i.e. graph leaves. As
    // early as possible scatters them across waves at their own dependency
    // depth, where each serializes alone; as late as possible collapses all of
    // them into the final wave so their costs overlap. On prometheus `./...`
    // (1455 source deps, 13.1s of type-check CPU) this takes the cold seed build
    // from 3.40s to 2.65s. Dropping the barriers entirely would only reach 2.2s
    // — see docs/PERF_TASKS.md §1.8 for why that is not worth its cost.
    let order = dep_load_order(&needed, dep_graph, &loadable);
    let source_set: HashSet<&str> = order
        .iter()
        .map(String::as_str)
        .filter(|p| {
            !export_paths.contains_key(*p) && seed_has_source_files(by_id, p, &augment)
        })
        .collect();

    // Pass 1 (leaves-first): dependency depth, so every source dep's deps are
    // already resolved when we reach it. Only `depth` (the deepest chain) is
    // needed afterwards, but the per-package values drive that maximum.
    let mut dep_depth: HashMap<&str, u32> = HashMap::default();
    let mut depth = 0u32;
    for p in &order {
        if !source_set.contains(p.as_str()) {
            continue;
        }
        let mut d_max = 0u32;
        if let Some(deps) = dep_graph.get(p) {
            for d in deps {
                if source_set.contains(d.as_str()) {
                    d_max = d_max.max(dep_depth.get(d.as_str()).copied().unwrap_or(0) + 1);
                }
            }
        }
        depth = depth.max(d_max);
        dep_depth.insert(p.as_str(), d_max);
    }

    // Pass 2 (consumers-first): `height(P)` = longest chain of source packages
    // that depends on `P`. Walking `order` in reverse visits every consumer of
    // `P` before `P` itself, so `height` is final by the time we read it — no
    // reverse graph needed.
    //
    // KNOWN BROKEN when `dep_graph` has a cycle, which bracket normalization can
    // manufacture out of an acyclic Go graph: prometheus' `util/teststorage`
    // test variant depends on `tsdb`, `tsdb`'s test variant depends on
    // `util/teststorage`, and after dedup drops both plain packages each key
    // takes its edges from a test variant. `dep_load_order`'s `visiting` guard
    // then drops an edge to finish the walk, `order` is no longer topological,
    // and the heights below come out inconsistent — 16 edges violating
    // `wave(dep) < wave(consumer)` on prometheus `./...`. See
    // docs/COMPAT-HARDENING.md §4 (2026-08-23, 続き 21) for why the fix belongs
    // in the loader (the seed compiles production files, so it wants production
    // edges) and not here: restricting these passes to the edges the DFS kept
    // reorders `promql` correctly and breaks `teststorage` instead.
    let mut height: HashMap<&str, u32> = HashMap::default();
    for p in order.iter().rev() {
        if !source_set.contains(p.as_str()) {
            continue;
        }
        let h = height.get(p.as_str()).copied().unwrap_or(0);
        height.insert(p.as_str(), h);
        if let Some(deps) = dep_graph.get(p) {
            for d in deps {
                if source_set.contains(d.as_str()) {
                    let e = height.entry(d.as_str()).or_insert(0);
                    *e = (*e).max(h + 1);
                }
            }
        }
    }

    let source_count = dep_depth.len();
    if source_count == 0 {
        if timing {
            eprintln!(
                "guff:     seed dep check {:.2}s (wave-parallel), 0 source deps, {} export deps",
                t_check_start.elapsed().as_secs_f64(),
                needed.len(),
            );
        }
        return Some(Arc::new(seed));
    }

    // Bucket by wave; sort each wave by path so the merge order — and therefore
    // the final arena layout and all downstream findings — is deterministic.
    // Empty waves are dropped: as-late-as-possible placement can leave gaps, and
    // merging nothing would only cost a pointless pass over the seed.
    let mut waves: Vec<Vec<&str>> = vec![Vec::new(); (depth + 1) as usize];
    for p in &order {
        if !source_set.contains(p.as_str()) {
            continue;
        }
        let h = height.get(p.as_str()).copied().unwrap_or(0);
        waves[(depth - h) as usize].push(p.as_str());
    }
    waves.retain(|w| !w.is_empty());
    for w in waves.iter_mut() {
        w.sort_unstable();
    }

    // Optional per-package overlay disk cache (PERF Task 4). Enabled by default;
    // disable with GUFF_SEED_PERSIST=0. Independent of the issues `--no-cache`
    // flag. Each source dep's exported-API overlay is written under
    // `${GUFF_CACHE}/seed`, keyed by (import path, content self-hash, seed-prefix
    // fingerprint); a later run whose deps are unchanged loads the overlay
    // instead of re-type-checking it.
    let persist = crate::seed_cache::seed_persist_enabled()
        .then(crate::seed_cache::seed_cache_dir)
        .flatten();

    // Background overlay writer: encode is cheap and stays on the wave workers,
    // but the disk syscalls (create + write + rename, ×N deps) are handed to a
    // dedicated thread so neither the wave merge nor the target type-check waits
    // on them. On an all-miss run (e.g. a fresh cache, as the regress harness
    // uses) this keeps persistence off the critical path entirely; writes drain
    // while later waves compute, and the handle is joined once before returning.
    let writer = persist.as_ref().map(|_| crate::seed_cache::OverlayWriter::spawn());

    // Phase B: check each wave in parallel on top of the accumulated seed, then
    // fold the wave's overlays back in. Each worker reads+parses its own source,
    // checks only its own package (deps resolve from the seed cache), and drops
    // its AST as soon as it captures its overlay — so resident dep-AST is bounded
    // to ~num_threads regardless of wave width. `ignore_func_bodies` keeps the
    // seed to exported API only (targets re-check with full bodies).
    //
    // When persistence is on, the worker hashes the source it just read and tries
    // to load a previously saved overlay keyed by (self_hash, base_fp) before
    // type-checking; base_fp fingerprints the seed prefix at wave start so
    // Remapper's foreign ids stay valid across runs. The source bytes are read
    // exactly once and reused for both the hash and (on a miss) the parser.
    let parallel = env.parallel;
    let check_sources = |path: &str, files: Vec<guff::ast::File>, seed: &ExportSeed| -> Option<WorkerOverlays> {
        if files.is_empty() {
            return None;
        }
        let mut check = Checker::from_seed(seed, make_conf());
        check.set_ignore_func_bodies(true);
        // Seed keeps only arena overlays (`into_worker_overlays`); Info maps
        // would be allocated and then dropped. Match Go's nil-Info path.
        check.set_record_info(false);
        // Fallback importer only; every dep should already be in the seed cache.
        check.set_importer(Box::new(make_importer()));
        check.add_dependency_source(path.to_string(), files);
        let pkg_id = check.preload_import(path)?;
        // Dependency diagnostics are not reported (they are not the user's
        // package), but "silently" is not the same as "unobservable": when a
        // seed dependency fails to type-check, every target that imports it
        // sees an incomplete type and the *target's* error names something else
        // entirely — `manager.Manager has no field or method GetCache` for a
        // failure in `pkg/cluster`. GUFF_DEBUG_SEED_ERRORS=1 is the only way to
        // read the first error rather than infer it.
        if seed_errors_enabled() && !check.errors.is_empty() {
            eprintln!(
                "guff:     seed dep {path} — {} error(s), first: {}",
                check.errors.len(),
                check
                    .errors
                    .first()
                    .map(|e| e.msg.clone())
                    .unwrap_or_default(),
            );
        }
        Some(check.into_worker_overlays(path.to_string(), pkg_id))
    };
    let mut merge_secs = 0f64;
    let mut widest = 0usize;
    let mut seed_hits = 0usize;
    let mut seed_misses = 0usize;
    let mut wave_walls: Vec<f64> = Vec::new();
    let mut dep_secs_by_path: HashMap<String, f64> = HashMap::default();
    // Running fingerprint of the seed prefix. Extended after each merged pkg
    // so wave N+1 does not re-hash the entire merged list (O(n²) SHA).
    let mut running_fp = persist.as_ref().map(|_| {
        crate::seed_cache::base_fingerprint(&crate::seed_cache::BaseFingerprintInput {
            go_version: &env.go_version,
            arch: &env.arch,
            s0_lens,
            s0_import_paths: &s0_import_paths,
            merged: &[],
        })
    });
    // (path, self_hash) in merge order — kept for debug / legacy callers.
    let mut merged: Vec<(String, String)> = Vec::new();

    // Under `GUFF_DEBUG_RSS`, sample the process every eighth wave. The
    // per-package attribution can only name what it can walk, so the shape of
    // this curve is what says whether the unnamed remainder is built here or
    // left behind here (PERF_TASKS_V6 §4.1).
    let rss_probe = crate::rss::enabled();
    if rss_probe {
        crate::rss::report_process("seed build start");
    }
    for (wave_idx, wave) in waves.iter().enumerate() {
        widest = widest.max(wave.len());
        let base_fp = running_fp.clone();

        // Resolve each path to (path, overlay, from_cache, self_hash). `path` and
        // `self_hash` are returned so `merged` bookkeeping stays aligned with
        // merge order after filter_map drops failures.
        let resolve_one = |path: &str| -> Option<(String, WorkerOverlays, bool, Option<String>, f64)> {
            let t_dep = acct.then(std::time::Instant::now);
            let dep_secs = |t: Option<std::time::Instant>| {
                t.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0)
            };
            let (pkg, with_tests) = seed_package_for(by_id, path, &augment)?;
            let seed_files: Vec<PathBuf> = pkg
                .compiled_go_files
                .iter()
                .filter(|f| with_tests || !is_test_file(f))
                .cloned()
                .collect();
            if seed_files.is_empty() {
                return None;
            }
            // Read each source file once; the bytes feed both the self-hash key
            // and (on a miss) the parser, so a miss never re-reads from disk.
            let sources = read_dep_sources(&seed_files);
            let self_hash = persist
                .as_ref()
                .map(|_| crate::seed_cache::pkg_self_hash_from_sources(&pkg.pkg_path, &sources));

            // Cache hit: load the persisted overlay and skip type-checking.
            if let (Some(dir), Some(fp), Some(h)) =
                (persist.as_ref(), base_fp.as_ref(), self_hash.as_ref())
            {
                if let Some(o) = crate::seed_cache::load_overlay(dir, path, h, fp) {
                    return Some((path.to_string(), o, true, self_hash, dep_secs(t_dep)));
                }
            }

            // Miss: parse the bytes we already read, then type-check.
            let files = parse_dep_sources(&sources, fset);
            let mut o = check_sources(path, files, &seed)?;
            if persist.is_some() {
                // Match the on-disk form so cold (all miss) and hot (all hit)
                // seeds are identical: FileSet-absolute positions do not survive
                // across runs.
                o.clear_source_positions();
                if let (Some(w), Some(dir), Some(fp), Some(h)) = (
                    writer.as_ref(),
                    persist.as_ref(),
                    base_fp.as_ref(),
                    self_hash.as_ref(),
                ) {
                    // Encode on the worker (cheap, parallel); the write itself
                    // runs on the background writer thread.
                    if let Ok(bytes) = o.encode() {
                        w.submit(crate::seed_cache::overlay_path(dir, path, h, fp), bytes);
                    }
                }
            }
            Some((path.to_string(), o, false, self_hash, dep_secs(t_dep)))
        };

        let t_wave = acct.then(std::time::Instant::now);
        let resolved: Vec<(String, WorkerOverlays, bool, Option<String>, f64)> = if parallel {
            wave.par_iter().filter_map(|p| resolve_one(p)).collect()
        } else {
            wave.iter().filter_map(|p| resolve_one(p)).collect()
        };
        if let Some(t) = t_wave {
            wave_walls.push(t.elapsed().as_secs_f64());
        }

        let mut overlays = Vec::with_capacity(resolved.len());
        for (path, overlay, from_cache, self_hash, secs) in resolved {
            if acct {
                dep_secs_by_path.insert(path.clone(), secs);
            }
            if from_cache {
                seed_hits += 1;
            } else {
                seed_misses += 1;
            }
            if let Some(h) = self_hash {
                if let Some(fp) = running_fp.as_mut() {
                    *fp = crate::seed_cache::base_fingerprint_extend(fp, &path, &h);
                }
                merged.push((path, h));
            }
            overlays.push(overlay);
        }

        let m = std::time::Instant::now();
        seed.merge_wave(overlays);
        merge_secs += m.elapsed().as_secs_f64();
        if rss_probe && wave_idx % 8 == 7 {
            crate::rss::report_process(&format!("seed wave {}", wave_idx + 1));
        }
    }
    if rss_probe {
        crate::rss::report_process("seed build done");
    }

    // Flush any in-flight overlay writes before returning so a persistent cache
    // is fully populated for the next run. On an all-miss run most writes have
    // already drained during later waves; this only waits on the tail.
    if let Some(w) = writer {
        w.finish();
    }

    if acct && !wave_walls.is_empty() {
        // Score the wave schedule against the only two numbers that bound it.
        //
        //   busy/threads  — every core saturated end to end, barriers gone.
        //   critical path — the longest chain of deps, each waiting for the one
        //                   below it. No schedule, barriered or not, beats this.
        //
        // The gap between the measured wall and `max(busy/threads, crit)` is the
        // whole prize for reworking the schedule. docs/PERF_TASKS.md §1.8 sized
        // that prize when the seed build was 2.65s; it is worth re-reading the
        // number before believing a barrier-removal estimate today.
        let threads = if parallel { rayon::current_num_threads() } else { 1 };
        let busy: f64 = dep_secs_by_path.values().sum();
        // Guard the occupancy division: a seed whose every dep came back inside
        // the timer's resolution would otherwise print `inf%`.
        let wave_wall: f64 = wave_walls.iter().sum::<f64>().max(f64::MIN_POSITIVE);
        // Longest dependency chain weighted by measured per-dep seconds.
        // `order` is a topological order, so each dep's own path is final.
        let mut cp: HashMap<&str, f64> = HashMap::default();
        let mut crit = 0f64;
        for p in &order {
            let Some(&own) = dep_secs_by_path.get(p.as_str()) else {
                continue;
            };
            let mut best = 0f64;
            if let Some(deps) = dep_graph.get(p) {
                for d in deps {
                    if let Some(&c) = cp.get(d.as_str()) {
                        best = best.max(c);
                    }
                }
            }
            let total = best + own;
            crit = crit.max(total);
            cp.insert(p.as_str(), total);
        }
        // Waves narrower than the thread count cannot fill the machine no
        // matter how fast each dep is; they are where a barrier actually bites.
        let narrow: usize = waves.iter().filter(|w| w.len() < threads).count();
        let narrow_wall: f64 = waves
            .iter()
            .zip(&wave_walls)
            .filter(|(w, _)| w.len() < threads)
            .map(|(_, s)| *s)
            .sum();
        eprintln!(
            "guff:     seed wave schedule: wall {wave_wall:.2}s vs busy/{threads} {:.2}s vs \
             critical path {crit:.2}s (busy {busy:.2}s, occupancy {:.0}%); \
             {narrow}/{} waves narrower than {threads} threads hold {narrow_wall:.2}s",
            busy / threads as f64,
            busy / threads as f64 / wave_wall * 100.0,
            waves.len(),
        );
    }
    if timing {
        if persist.is_some() {
            eprintln!(
                "guff:     seed dep check {:.2}s (wave-parallel; merge {:.2}s serial), {} source deps in {} waves (widest {}), {} export deps; seed cache hits={} misses={}",
                t_check_start.elapsed().as_secs_f64(),
                merge_secs,
                source_count,
                waves.len(),
                widest,
                needed.len() - source_count,
                seed_hits,
                seed_misses,
            );
        } else {
            eprintln!(
                "guff:     seed dep check {:.2}s (wave-parallel; merge {:.2}s serial), {} source deps in {} waves (widest {}), {} export deps",
                t_check_start.elapsed().as_secs_f64(),
                merge_secs,
                source_count,
                waves.len(),
                widest,
                needed.len() - source_count,
            );
        }
    }
    Some(Arc::new(seed))
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
    loadable: &HashSet<String>,
) -> Vec<String> {
    let mut order = Vec::new();
    let mut done = HashSet::default();
    let mut visiting: Vec<String> = Vec::new();
    for id in needed {
        dep_load_order_visit(id, dep_graph, loadable, &mut done, &mut visiting, &mut order);
    }
    order
}

fn dep_load_order_visit(
    path: &str,
    dep_graph: &HashMap<String, Vec<String>>,
    loadable: &HashSet<String>,
    done: &mut HashSet<String>,
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

/// Read a dependency's `compiled_go_files` once, in listed order. Files that
/// fail to read are skipped (dependency diagnostics are not reported on the
/// source-seed path). The returned bytes feed both the persisted-overlay
/// self-hash key and the parser, so a cache miss never reads source twice.
fn read_dep_sources(paths: &[PathBuf]) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        if let Ok(src) = fs::read(path) {
            out.push((path.clone(), src));
        }
    }
    out
}

/// Parse pre-read dependency sources into syntax trees sharing `fset`, in the
/// order given. Files that fail to parse are skipped (dependency diagnostics
/// are not reported on the source-seed path).
///
/// Function bodies are scanned but not built into statement trees
/// ([`SKIP_FUNC_BODIES`]): the seed only needs each dependency's exported API,
/// and `check_sources` type-checks it with `ignore_func_bodies`, so every
/// statement node built here would be dropped unread. Bodies are roughly half
/// of the parse cost of a dependency closure.
///
/// Object resolution is also skipped ([`SKIP_OBJECT_RESOLUTION`]): `resolve_file`
/// walks the tree filling each `Ident.obj` with a pointer to its declaration
/// (Go's deprecated `ast.Object` mechanism). The type checker does its own scope
/// resolution and never reads `Ident.obj`, and no analyzer runs on dependency
/// ASTs — the only `obj` readers (ineffassign, maintidx) run on *target*
/// packages, parsed separately. So on this path resolution is pure waste.
fn parse_dep_sources(sources: &[(PathBuf, Vec<u8>)], fset: &Arc<FileSet>) -> Vec<guff::ast::File> {
    let mut out = Vec::with_capacity(sources.len());
    for (path, src) in sources {
        let name = path.to_str().unwrap_or("file.go");
        if let Ok(file) = parse_file(fset, name, src, SKIP_FUNC_BODIES | SKIP_OBJECT_RESOLUTION) {
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
    done: &mut HashSet<String>,
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
    done: &mut HashSet<String>,
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
    use crate::hash::{HashMap, HashSet};
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
            &HashMap::default(),
            &HashMap::default(),
            default_sizes(),
            &TypecheckEnv::default(),
            mode,
        );
        assert!(!pkg.ill_typed, "errors: {:?}", pkg.errors);
        assert!(pkg.types.is_some());
        let info = pkg.types_info.as_deref().expect("types info");
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
            &HashMap::default(),
            &HashMap::default(),
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
            skip_object_resolution: false,
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

        let mut export_paths = HashMap::default();
        export_paths.insert(dep_id.clone(), dep_export);

        let mut dep_graph = HashMap::default();
        dep_graph.insert(dep_id.clone(), Vec::<String>::new());

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

        let mut export_paths = HashMap::default();
        export_paths.insert(dep_id.clone(), dep_export);
        let mut dep_graph = HashMap::default();
        dep_graph.insert(dep_id.clone(), Vec::<String>::new());

        let mut by_id = HashMap::default();
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
