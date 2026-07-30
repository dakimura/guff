//! Walk the package graph from patterns using `guff-build` + module resolution.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use guff_build::{find_module_root, Context, ModFile};
use rustc_hash::{FxHashMap, FxHashSet};
use rayon::prelude::*;

use crate::bail::{Bail, BailReason};
use crate::modcache::ModCache;
use crate::modmeta::{self, ModMetaKey};
use crate::resolve::{resolve_import, ResolvedModule};
use crate::workspace::{load_workspace, Workspace};

/// Input for [`list_packages`] (mirrors the subset of `packages.Config` we need).
#[derive(Debug, Clone)]
pub struct ListConfig {
    pub dir: PathBuf,
    pub build_tags: Vec<String>,
    pub tests: bool,
    pub need_deps: bool,
    /// Override GOMODCACHE (tests). `None` → env default.
    pub gomodcache: Option<PathBuf>,
    /// Override GOROOT (tests). `None` → `Context` default.
    pub goroot: Option<PathBuf>,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            dir: std::env::current_dir().unwrap_or_default(),
            build_tags: Vec::new(),
            tests: false,
            need_deps: true,
            gomodcache: None,
            goroot: None,
        }
    }
}

/// Native-lister package metadata (converted to `packages.Package` by the caller).
#[derive(Debug, Clone)]
pub struct ListPackage {
    pub id: String,
    pub name: String,
    pub pkg_path: String,
    pub dir: PathBuf,
    pub go_files: Vec<PathBuf>,
    pub compiled_go_files: Vec<PathBuf>,
    pub ignored_files: Vec<PathBuf>,
    /// Direct imports: (source import path, resolved package id).
    pub imports: Vec<(String, String)>,
    /// Transitive dependency import paths (sorted), excluding self.
    pub deps: Vec<String>,
    pub module: Option<ListModule>,
    pub standard: bool,
    pub dep_only: bool,
    /// Package under test (`ForTest` in `go list` JSON). Empty when not a test variant.
    pub for_test: String,
}

#[derive(Debug, Clone)]
pub struct ListModule {
    pub path: String,
    pub version: String,
    pub main: bool,
    pub indirect: bool,
    pub dir: PathBuf,
    pub go_mod: PathBuf,
    pub go_version: String,
}

#[derive(Debug, Clone, Default)]
pub struct ListResponse {
    pub roots: Vec<String>,
    pub packages: Vec<ListPackage>,
    pub compiler: String,
    pub arch: String,
}

/// Lists packages for `patterns`, or [`Bail`] when the request is out of scope.
pub fn list_packages(cfg: &ListConfig, patterns: &[String]) -> Result<ListResponse, Bail> {
    check_bail_preconditions(cfg, patterns)?;

    let src_dir = abs_dir(&cfg.dir)?;
    let module_root = find_module_root(&src_dir).ok_or_else(|| {
        Bail::new(
            BailReason::NoGoMod,
            format!("no go.mod above {}", src_dir.display()),
        )
    })?;

    let workspace = load_workspace(&src_dir, &module_root)?;
    let active = workspace.module_containing(&src_dir).ok_or_else(|| {
        Bail::new(
            BailReason::GoWork,
            format!(
                "{} is not inside any go.work use module",
                src_dir.display()
            ),
        )
    })?;
    // Patterns / package identity are relative to the active (containing) module.
    let module_root = active.dir.clone();
    let mod_file = active.mod_file.clone();

    if module_root.join("vendor").is_dir() {
        return Err(Bail::new(
            BailReason::Vendor,
            "vendor/ present (native list v1 unsupported)",
        ));
    }

    // Version / exclude gates apply to the active module (the one we list from).
    // Other workspace modules are only used for path resolution.
    check_mod_file(&mod_file)?;

    let cache = match &cfg.gomodcache {
        Some(p) => ModCache::with_root(p.clone()),
        None => ModCache::from_env(),
    };

    let mut ctxt = Context::default();
    if let Some(root) = &cfg.goroot {
        ctxt.goroot = root.to_string_lossy().into_owned();
    }
    for tag in &cfg.build_tags {
        if !ctxt.build_tags.iter().any(|t| t == tag) {
            ctxt.build_tags.push(tag.clone());
        }
    }
    let goroot = PathBuf::from(&ctxt.goroot);
    let goroot_ver = modmeta::goroot_version(&goroot);

    let root_dirs = expand_patterns(
        &ctxt,
        &src_dir,
        &module_root,
        &mod_file,
        patterns,
    )?;

    let mut response = ListResponse {
        compiler: "gc".into(),
        arch: ctxt.goarch.clone(),
        ..ListResponse::default()
    };

    let mut seen: FxHashSet<String> = FxHashSet::default();
    let mut queue: VecDeque<(String, PathBuf, ResolvedModule, bool)> = VecDeque::new();
    let mut packages: FxHashMap<String, ListPackage> = FxHashMap::default();
    let mut direct_imports: FxHashMap<String, Vec<String>> = FxHashMap::default();

    for dir in &root_dirs {
        let (pkg_path, module) = package_identity(dir, &workspace)?;
        if !seen.insert(pkg_path.clone()) {
            continue;
        }
        queue.push_back((pkg_path, dir.clone(), module, true));
    }

    // Process the BFS queue in parallel batches: import_dir is syscall-bound
    // and independent across packages already in the queue.
    while !queue.is_empty() {
        let batch: Vec<(String, PathBuf, ResolvedModule, bool)> = queue.drain(..).collect();
        let scanned: Vec<_> = batch
            .par_iter()
            .map(|(pkg_path, dir, module, is_root)| {
                let key = ModMetaKey {
                    module_path: &module.path,
                    module_version: &module.version,
                    pkg_path,
                    goos: &ctxt.goos,
                    goarch: &ctxt.goarch,
                    build_tags: &ctxt.build_tags,
                    standard: module.standard,
                    goroot_version: &goroot_ver,
                };
                let result = modmeta::import_dir_cached(&ctxt, dir, &key);
                (pkg_path.clone(), dir.clone(), module.clone(), *is_root, result)
            })
            .collect();

        for (pkg_path, dir, module, is_root, build_result) in scanned {
            let build_pkg = match build_result {
                Ok(p) => p,
                Err(e) => {
                    // `./...` walk is a cheap `.go` name gate; dirs with no
                    // buildable files (build tags / empty) are not roots.
                    if is_root {
                        let _ = e;
                        continue;
                    }
                    packages.insert(
                        pkg_path.clone(),
                        ListPackage {
                            id: pkg_path.clone(),
                            name: String::new(),
                            pkg_path: pkg_path.clone(),
                            dir: dir.clone(),
                            go_files: Vec::new(),
                            compiled_go_files: Vec::new(),
                            ignored_files: Vec::new(),
                            imports: Vec::new(),
                            deps: Vec::new(),
                            module: Some(to_list_module(&module)),
                            standard: module.standard,
                            dep_only: true,
                            for_test: String::new(),
                        },
                    );
                    let _ = e;
                    continue;
                }
            };

            // ... rest of package processing continues below via helper
            process_listed_package(
                cfg,
                &workspace,
                &cache,
                &goroot,
                &mut response,
                &mut seen,
                &mut queue,
                &mut packages,
                &mut direct_imports,
                pkg_path,
                module,
                is_root,
                build_pkg,
            )?;
        }
    }

    // Packages that import the package-under-test (or another for-test variant)
    // must be recompiled as `Q [P.test]` — matching cmd/go list -test.
    if cfg.tests {
        emit_fortest_dep_variants(&mut packages, &mut direct_imports);
    }

    // Fill transitive deps from the direct-import graph.
    for id in packages.keys().cloned().collect::<Vec<_>>() {
        let deps = transitive_deps(&id, &direct_imports);
        if let Some(pkg) = packages.get_mut(&id) {
            pkg.deps = deps;
        }
    }

    let mut pkgs: Vec<ListPackage> = packages.into_values().collect();
    pkgs.sort_by(|a, b| a.id.cmp(&b.id));
    response.roots.sort();
    response.packages = pkgs;
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
fn process_listed_package(
    cfg: &ListConfig,
    workspace: &Workspace,
    cache: &ModCache,
    goroot: &Path,
    response: &mut ListResponse,
    mut seen: &mut FxHashSet<String>,
    mut queue: &mut VecDeque<(String, PathBuf, ResolvedModule, bool)>,
    mut packages: &mut FxHashMap<String, ListPackage>,
    direct_imports: &mut FxHashMap<String, Vec<String>>,
    pkg_path: String,
    module: ResolvedModule,
    is_root: bool,
    build_pkg: guff_build::Package,
) -> Result<(), Bail> {
        let go_files = abs_join(&build_pkg.dir, &build_pkg.go_files);
        // Match go list's Package.GoFiles which merges CgoFiles into GoFiles.
        let mut go_files_with_cgo = go_files.clone();
        go_files_with_cgo.extend(abs_join(&build_pkg.dir, &build_pkg.cgo_files));
        // Plain package stays production-only. Test files go on `P [P.test]`.
        let compiled = go_files_with_cgo.clone();
        let ignored = abs_join(&build_pkg.dir, &build_pkg.ignored_go_files);
        // Imports already extracted during import_dir (header scan) — do not
        // re-read sources.
        let imports = &build_pkg.imports;

        let mut import_list: Vec<(String, String)> = Vec::new();
        let mut direct_ids: Vec<String> = Vec::new();
        for import_path in imports {
            if import_path == "C" {
                // Pure-cgo packages still appear in Imports; skip like go/packages.
                continue;
            }
            if import_path == "unsafe" {
                import_list.push((import_path.clone(), "unsafe".into()));
                direct_ids.push("unsafe".into());
                ensure_unsafe(&mut packages, &mut seen);
                continue;
            }
            let (dep_dir, dep_mod) = resolve_import(
                import_path,
                &workspace,
                &cache,
                &goroot,
                module.standard,
            )?;
            let dep_id = dep_mod.pkg_id.clone();
            import_list.push((import_path.clone(), dep_id.clone()));
            direct_ids.push(dep_id.clone());

            if (is_root || cfg.need_deps) && !seen.contains(&dep_id) {
                if seen.insert(dep_id.clone()) {
                    queue.push_back((dep_id, dep_dir, dep_mod, false));
                }
            }
        }
        import_list.sort_by(|a, b| a.0.cmp(&b.0));
        import_list.dedup_by(|a, b| a.0 == b.0);
        // go list synthesizes a Deps entry on runtime/cgo for cgo packages
        // (not an Imports entry). Skip for runtime/cgo itself.
        if !build_pkg.cgo_files.is_empty() && pkg_path != "runtime/cgo" {
            let cgo = "runtime/cgo";
            if !direct_ids.iter().any(|d| d == cgo) {
                if let Ok((dep_dir, dep_mod)) =
                    resolve_import(cgo, &workspace, &cache, &goroot, false)
                {
                    let dep_id = dep_mod.pkg_id.clone();
                    // Deps only — do not add to import_list / Imports map.
                    direct_ids.push(dep_id.clone());
                    if (is_root || cfg.need_deps) && seen.insert(dep_id.clone()) {
                        queue.push_back((dep_id, dep_dir, dep_mod, false));
                    }
                }
            }
        }
        direct_ids.sort();
        direct_ids.dedup();
        direct_imports.insert(pkg_path.clone(), direct_ids);

        let list_mod = if module.standard {
            None
        } else {
            Some(to_list_module(&module))
        };

        packages.insert(
            pkg_path.clone(),
            ListPackage {
                id: pkg_path.clone(),
                name: build_pkg.name.clone(),
                pkg_path: pkg_path.clone(),
                dir: build_pkg.dir.clone(),
                go_files: go_files_with_cgo.clone(),
                compiled_go_files: compiled,
                ignored_files: ignored,
                imports: import_list,
                deps: Vec::new(), // filled below
                module: list_mod.clone(),
                standard: module.standard,
                dep_only: !is_root,
                for_test: String::new(),
            },
        );

        // `-test` variants matching `cmd/go list -test` / go/packages IDs:
        //   P [P.test]       — internal tests (prod + *_test.go, same package)
        //   P_test [P.test]  — external tests (package P_test)
        //   P.test           — synthetic testmain (stub; no generated source)
        if cfg.tests && is_root {
            let has_internal = !build_pkg.test_go_files.is_empty();
            let has_external = !build_pkg.xtest_go_files.is_empty();
            if has_internal || has_external {
                let test_bin = format!("{pkg_path}.test");
                let internal_id = format!("{pkg_path} [{test_bin}]");
                let mut variant_ids_for_testmain: Vec<String> = Vec::new();

                if has_internal {
                    let mut internal_files = go_files_with_cgo.clone();
                    internal_files.extend(abs_join(&build_pkg.dir, &build_pkg.test_go_files));
                    let mut internal_imps = build_pkg.imports.clone();
                    for imp in &build_pkg.test_imports {
                        if !internal_imps.iter().any(|i| i == imp) {
                            internal_imps.push(imp.clone());
                        }
                    }
                    let (pairs, directs) = resolve_import_paths(
                        &internal_imps,
                        &workspace,
                        &cache,
                        &goroot,
                        module.standard,
                        cfg.need_deps,
                        &mut packages,
                        &mut seen,
                        &mut queue,
                        None,
                    )?;
                    direct_imports.insert(internal_id.clone(), directs);
                    packages.insert(
                        internal_id.clone(),
                        ListPackage {
                            id: internal_id.clone(),
                            name: build_pkg.name.clone(),
                            pkg_path: pkg_path.clone(),
                            dir: build_pkg.dir.clone(),
                            go_files: internal_files.clone(),
                            compiled_go_files: internal_files,
                            ignored_files: Vec::new(),
                            imports: pairs,
                            deps: Vec::new(),
                            module: list_mod.clone(),
                            standard: module.standard,
                            dep_only: false,
                            for_test: pkg_path.clone(),
                        },
                    );
                    response.roots.push(internal_id.clone());
                    variant_ids_for_testmain.push(internal_id.clone());
                }

                if has_external {
                    let xtest_id = format!("{pkg_path}_test [{test_bin}]");
                    let xtest_files = abs_join(&build_pkg.dir, &build_pkg.xtest_go_files);
                    // Import of P resolves to the internal test variant when
                    // present (same as cmd/go); otherwise the plain package.
                    let p_target = if has_internal {
                        internal_id.clone()
                    } else {
                        pkg_path.clone()
                    };
                    let rewrite = [(pkg_path.as_str(), p_target.as_str())];
                    let (mut pairs, mut directs) = resolve_import_paths(
                        &build_pkg.xtest_imports,
                        &workspace,
                        &cache,
                        &goroot,
                        false,
                        cfg.need_deps,
                        &mut packages,
                        &mut seen,
                        &mut queue,
                        Some(&rewrite),
                    )?;
                    // Ensure the package under test is imported even if the
                    // xtest sources don't mention it (rare, but go list does).
                    if !pairs.iter().any(|(src, _)| src == &pkg_path) {
                        pairs.push((pkg_path.clone(), p_target.clone()));
                        directs.push(p_target.clone());
                    }
                    pairs.sort_by(|a, b| a.0.cmp(&b.0));
                    pairs.dedup_by(|a, b| a.0 == b.0);
                    directs.sort();
                    directs.dedup();
                    direct_imports.insert(xtest_id.clone(), directs);
                    packages.insert(
                        xtest_id.clone(),
                        ListPackage {
                            id: xtest_id.clone(),
                            name: format!("{}_test", build_pkg.name),
                            pkg_path: format!("{pkg_path}_test"),
                            dir: build_pkg.dir.clone(),
                            go_files: xtest_files.clone(),
                            compiled_go_files: xtest_files,
                            ignored_files: Vec::new(),
                            imports: pairs,
                            deps: Vec::new(),
                            module: list_mod.clone(),
                            standard: false,
                            dep_only: false,
                            for_test: pkg_path.clone(),
                        },
                    );
                    response.roots.push(xtest_id.clone());
                    variant_ids_for_testmain.push(xtest_id);
                }

                // Synthetic testmain. cmd/go emits a generated file under
                // GOCACHE; we stub an empty package so roots match. Analysis
                // skips empty compiled_go_files; verify ignores file diffs.
                if seen.insert(test_bin.clone()) {
                    let mut tm_imports: Vec<(String, String)> = Vec::new();
                    let mut tm_direct: Vec<String> = Vec::new();
                    for vid in &variant_ids_for_testmain {
                        // testmain imports variants by their IDs.
                        tm_imports.push((vid.clone(), vid.clone()));
                        tm_direct.push(vid.clone());
                    }
                    if !has_internal {
                        // xtest-only: go list also imports the plain package.
                        tm_imports.push((pkg_path.clone(), pkg_path.clone()));
                        tm_direct.push(pkg_path.clone());
                    }
                    for std in ["testing", "os", "reflect", "testing/internal/testdeps"] {
                        if let Ok((dep_dir, dep_mod)) =
                            resolve_import(std, &workspace, &cache, &goroot, false)
                        {
                            let dep_id = dep_mod.pkg_id.clone();
                            tm_imports.push((std.to_string(), dep_id.clone()));
                            tm_direct.push(dep_id.clone());
                            if cfg.need_deps && seen.insert(dep_id.clone()) {
                                queue.push_back((dep_id, dep_dir, dep_mod, false));
                            }
                        }
                    }
                    tm_imports.sort_by(|a, b| a.0.cmp(&b.0));
                    tm_direct.sort();
                    tm_direct.dedup();
                    direct_imports.insert(test_bin.clone(), tm_direct);
                    packages.insert(
                        test_bin.clone(),
                        ListPackage {
                            id: test_bin.clone(),
                            name: "main".into(),
                            pkg_path: test_bin.clone(),
                            dir: build_pkg.dir.clone(),
                            go_files: Vec::new(),
                            compiled_go_files: Vec::new(),
                            ignored_files: Vec::new(),
                            imports: tm_imports,
                            deps: Vec::new(),
                            module: list_mod,
                            standard: false,
                            dep_only: false,
                            for_test: String::new(),
                        },
                    );
                    response.roots.push(test_bin);
                }
            }
        }

        if is_root {
            response.roots.push(pkg_path);
        }
        Ok(())
}

/// Emit `Q [P.test]` for dependencies that import the package under test.
///
/// cmd/go recompiles any non-stdlib package in the test binary's link set that
/// imports `P` (or another for-test variant) so it sees `P [P.test]`. Those
/// packages are DepOnly and carry `ForTest=P`.
fn emit_fortest_dep_variants(
    packages: &mut FxHashMap<String, ListPackage>,
    direct_imports: &mut FxHashMap<String, Vec<String>>,
) {
    // Primary test packages: ForTest set, not DepOnly (P [P.test] / P_test [P.test]).
    let mut by_fortest: FxHashMap<String, Vec<String>> = FxHashMap::default();
    for pkg in packages.values() {
        if !pkg.for_test.is_empty() && !pkg.dep_only {
            by_fortest
                .entry(pkg.for_test.clone())
                .or_default()
                .push(pkg.id.clone());
        }
    }

    for (p, primary_ids) in by_fortest {
        let test_bin = format!("{p}.test");
        let p_variant = format!("{p} [{test_bin}]");
        // Only when an internal test variant exists does cmd/go recompile
        // dependents against `P [P.test]`. Xtest-only links plain `P`.
        if !packages.contains_key(&p_variant) {
            continue;
        }

        // plain import path → variant id for this test binary
        let mut var_map: FxHashMap<String, String> = FxHashMap::default();
        var_map.insert(p.clone(), p_variant.clone());

        // Reachable plain package ids from the test packages (+ testmain).
        let mut reachable: FxHashSet<String> = FxHashSet::default();
        let mut stack: Vec<String> = primary_ids.clone();
        let testmain = format!("{p}.test");
        if packages.contains_key(&testmain) {
            stack.push(testmain);
        }
        while let Some(id) = stack.pop() {
            let Some(deps) = direct_imports.get(&id) else {
                continue;
            };
            for dep in deps {
                let plain = plain_package_id(dep);
                if plain == "unsafe" || plain == "C" {
                    continue;
                }
                if reachable.insert(plain.clone()) {
                    // Continue walking via the plain package's imports.
                    stack.push(plain);
                }
            }
        }

        // Fixed-point: any reachable non-stdlib package that imports a shadowed
        // path gets a `Q [P.test]` variant.
        let mut changed = true;
        while changed {
            changed = false;
            let candidates: Vec<String> = reachable.iter().cloned().collect();
            for q in candidates {
                if var_map.contains_key(&q) {
                    continue;
                }
                let Some(qpkg) = packages.get(&q) else {
                    continue;
                };
                if qpkg.standard {
                    continue;
                }
                // Skip other packages' test variants / testmains.
                if q.contains(' ') || q.ends_with(".test") {
                    continue;
                }
                let imports_shadowed = qpkg.imports.iter().any(|(src, id)| {
                    var_map.contains_key(src)
                        || var_map.contains_key(&plain_package_id(id))
                        || var_map.values().any(|v| v == id)
                });
                if !imports_shadowed {
                    continue;
                }

                let q_var = format!("{q} [{test_bin}]");
                if packages.contains_key(&q_var) {
                    var_map.insert(q.clone(), q_var);
                    continue;
                }

                let mut new_pkg = qpkg.clone();
                new_pkg.id = q_var.clone();
                new_pkg.pkg_path = q.clone();
                new_pkg.for_test = p.clone();
                new_pkg.dep_only = true;
                // Remap imports onto variants we already know; a second pass
                // below finishes remapping once the fixed point settles.
                remap_imports_with_var_map(&mut new_pkg.imports, &var_map);
                let directs: Vec<String> = new_pkg.imports.iter().map(|(_, id)| id.clone()).collect();
                direct_imports.insert(q_var.clone(), directs);
                packages.insert(q_var.clone(), new_pkg);
                var_map.insert(q, q_var);
                changed = true;
            }
        }

        // Final remap: every package whose id ends with ` [P.test]` (including
        // primaries) should import via var_map.
        let suffix = format!(" [{test_bin}]");
        let ids: Vec<String> = packages.keys().cloned().collect();
        for id in ids {
            if !id.ends_with(&suffix) {
                continue;
            }
            let Some(pkg) = packages.get_mut(&id) else {
                continue;
            };
            remap_imports_with_var_map(&mut pkg.imports, &var_map);
            let directs: Vec<String> = pkg.imports.iter().map(|(_, tid)| tid.clone()).collect();
            direct_imports.insert(id, directs);
        }
    }
}

fn plain_package_id(id: &str) -> String {
    match id.find(' ') {
        Some(i) => id[..i].to_string(),
        None => id.to_string(),
    }
}

fn remap_imports_with_var_map(
    imports: &mut Vec<(String, String)>,
    var_map: &FxHashMap<String, String>,
) {
    for (src, id) in imports.iter_mut() {
        if let Some(v) = var_map.get(src) {
            *id = v.clone();
            continue;
        }
        let plain = plain_package_id(id);
        if let Some(v) = var_map.get(&plain) {
            *id = v.clone();
        }
    }
}

fn check_bail_preconditions(cfg: &ListConfig, patterns: &[String]) -> Result<(), Bail> {
    let _ = cfg;
    for pattern in patterns {
        if !pattern_supported(pattern) {
            return Err(Bail::new(
                BailReason::UnsupportedPattern,
                format!("unsupported pattern {pattern:?}"),
            ));
        }
    }
    Ok(())
}

fn pattern_supported(pattern: &str) -> bool {
    if pattern.is_empty() || pattern == "." || pattern == "./..." || pattern == "..." {
        return true;
    }
    if pattern.starts_with("./") || Path::new(pattern).is_absolute() {
        return true;
    }
    // Main-module import paths are checked later once go.mod is known; here we
    // only reject clearly-unsupported forms (std patterns like "std", "cmd",
    // "all", and bare non-path tokens with spaces).
    if pattern.contains(' ') || pattern == "all" || pattern == "std" || pattern == "cmd" {
        return false;
    }
    true
}

fn check_mod_file(mod_file: &ModFile) -> Result<(), Bail> {
    // `exclude` / `retract` do not block listing: requires already reflect the
    // selected versions, and go list succeeds with them present.
    let _ = (mod_file.has_exclude, mod_file.has_retract);
    let ver = mod_file.go_version.as_deref().unwrap_or("1.16");
    if go_version_less(ver, "1.17") {
        return Err(Bail::new(
            BailReason::GoVersionTooOld,
            format!("go {ver} < 1.17"),
        ));
    }
    Ok(())
}

/// Compares Go minor versions like `1.22` / `1.22.3`.
fn go_version_less(a: &str, b: &str) -> bool {
    parse_go_minor(a) < parse_go_minor(b)
}

fn parse_go_minor(v: &str) -> (u32, u32) {
    let mut parts = v.trim().split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor)
}

fn package_identity(dir: &Path, workspace: &Workspace) -> Result<(String, ResolvedModule), Bail> {
    let wm = workspace.module_containing(dir).ok_or_else(|| {
        Bail::new(
            BailReason::UnsupportedPattern,
            format!("{} is outside all workspace modules", dir.display()),
        )
    })?;
    let rel = dir.strip_prefix(&wm.dir).map_err(|_| {
        Bail::new(
            BailReason::UnsupportedPattern,
            format!("{} is outside module root", dir.display()),
        )
    })?;
    let rel = rel.to_string_lossy().replace('\\', "/");
    let pkg_path = if rel.is_empty() {
        wm.mod_file.module_path.clone()
    } else {
        format!("{}/{}", wm.mod_file.module_path, rel.trim_matches('/'))
    };
    Ok((
        pkg_path.clone(),
        ResolvedModule {
            path: wm.mod_file.module_path.clone(),
            version: String::new(),
            dir: wm.dir.clone(),
            go_mod: wm.dir.join("go.mod"),
            main: true,
            indirect: false,
            go_version: wm.mod_file.go_version.clone().unwrap_or_default(),
            standard: false,
            pkg_id: pkg_path,
        },
    ))
}

fn to_list_module(m: &ResolvedModule) -> ListModule {
    ListModule {
        path: m.path.clone(),
        version: m.version.clone(),
        main: m.main,
        indirect: m.indirect,
        dir: m.dir.clone(),
        go_mod: m.go_mod.clone(),
        go_version: m.go_version.clone(),
    }
}

fn ensure_unsafe(packages: &mut FxHashMap<String, ListPackage>, seen: &mut FxHashSet<String>) {
    if !seen.insert("unsafe".into()) {
        return;
    }
    packages.insert(
        "unsafe".into(),
        ListPackage {
            id: "unsafe".into(),
            name: "unsafe".into(),
            pkg_path: "unsafe".into(),
            dir: PathBuf::new(),
            go_files: Vec::new(),
            compiled_go_files: Vec::new(),
            ignored_files: Vec::new(),
            imports: Vec::new(),
            deps: Vec::new(),
            module: None,
            standard: true,
            dep_only: true,
            for_test: String::new(),
        },
    );
}

/// Resolve import paths, optionally rewriting source→id pairs.
///
/// `rewrite` maps source import path → already-known package id (used so
/// external tests import `P [P.test]` instead of plain `P`).
fn resolve_import_paths(
    imports: &[String],
    workspace: &Workspace,
    cache: &ModCache,
    goroot: &Path,
    from_stdlib: bool,
    need_deps: bool,
    packages: &mut FxHashMap<String, ListPackage>,
    seen: &mut FxHashSet<String>,
    queue: &mut VecDeque<(String, PathBuf, ResolvedModule, bool)>,
    rewrite: Option<&[(&str, &str)]>,
) -> Result<(Vec<(String, String)>, Vec<String>), Bail> {
    let mut import_list: Vec<(String, String)> = Vec::new();
    let mut direct_ids: Vec<String> = Vec::new();
    for import_path in imports {
        if import_path == "C" {
            continue;
        }
        if let Some(pairs) = rewrite {
            if let Some((_, target)) = pairs.iter().find(|(src, _)| *src == import_path) {
                import_list.push((import_path.clone(), (*target).to_string()));
                direct_ids.push((*target).to_string());
                continue;
            }
        }
        if import_path == "unsafe" {
            import_list.push((import_path.clone(), "unsafe".into()));
            direct_ids.push("unsafe".into());
            ensure_unsafe(packages, seen);
            continue;
        }
        let (dep_dir, dep_mod) =
            resolve_import(import_path, workspace, cache, goroot, from_stdlib)?;
        let dep_id = dep_mod.pkg_id.clone();
        import_list.push((import_path.clone(), dep_id.clone()));
        direct_ids.push(dep_id.clone());
        if need_deps && seen.insert(dep_id.clone()) {
            queue.push_back((dep_id, dep_dir, dep_mod, false));
        }
    }
    import_list.sort_by(|a, b| a.0.cmp(&b.0));
    import_list.dedup_by(|a, b| a.0 == b.0);
    direct_ids.sort();
    direct_ids.dedup();
    Ok((import_list, direct_ids))
}

fn expand_patterns(
    ctxt: &Context,
    src_dir: &Path,
    module_root: &Path,
    mod_file: &ModFile,
    patterns: &[String],
) -> Result<Vec<PathBuf>, Bail> {
    let patterns = if patterns.is_empty() {
        vec![".".to_string()]
    } else {
        patterns.to_vec()
    };
    let mut dirs = Vec::new();
    for pattern in &patterns {
        if pattern == "./..." || pattern == "..." {
            walk_packages(ctxt, module_root, &mut dirs)?;
            continue;
        }
        if let Some(prefix) = pattern.strip_suffix("/...") {
            let dir = resolve_pattern_dir(src_dir, module_root, mod_file, prefix)?;
            walk_packages(ctxt, &dir, &mut dirs)?;
            continue;
        }
        let dir = resolve_pattern_dir(src_dir, module_root, mod_file, pattern)?;
        dirs.push(dir);
    }
    dirs.sort();
    dirs.dedup();
    if dirs.is_empty() {
        return Err(Bail::new(
            BailReason::UnsupportedPattern,
            "no packages matched patterns",
        ));
    }
    Ok(dirs)
}

fn resolve_pattern_dir(
    src_dir: &Path,
    module_root: &Path,
    mod_file: &ModFile,
    pattern: &str,
) -> Result<PathBuf, Bail> {
    if pattern == "." || pattern.is_empty() {
        return Ok(src_dir.to_path_buf());
    }
    let p = Path::new(pattern);
    if pattern.starts_with('.') || p.is_absolute() {
        let dir = if p.is_absolute() {
            p.to_path_buf()
        } else {
            src_dir.join(p)
        };
        return dir.canonicalize().map_err(|e| {
            Bail::new(BailReason::Io, format!("pattern {pattern}: {e}"))
        });
    }
    if path_prefix_match(pattern, &mod_file.module_path) {
        if let Some(dir) = module_import_dir_checked(module_root, &mod_file.module_path, pattern) {
            return Ok(dir);
        }
    }
    Err(Bail::new(
        BailReason::UnsupportedPattern,
        format!("pattern {pattern:?} is outside the main module"),
    ))
}

fn module_import_dir_checked(root: &Path, module_path: &str, import_path: &str) -> Option<PathBuf> {
    let dir = guff_build::module_import_dir(root, module_path, import_path)?;
    dir.is_dir().then_some(dir)
}

fn path_prefix_match(import_path: &str, module_path: &str) -> bool {
    import_path == module_path
        || (import_path.starts_with(module_path)
            && import_path.as_bytes().get(module_path.len()) == Some(&b'/'))
}

fn walk_packages(ctxt: &Context, root: &Path, out: &mut Vec<PathBuf>) -> Result<(), Bail> {
    let _ = ctxt; // build tags applied later in import_dir
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
            if (name == "vendor" || name == "testdata" || name == "node_modules" || name.starts_with('.'))
                && dir != root
            {
                continue;
            }
        }
        // Nested modules (their own go.mod) are outside `./...` of the parent,
        // matching `go list` — and often live in go.work as sibling `use`s.
        if dir != root && dir.join("go.mod").is_file() {
            continue;
        }
        // Cheap gate: any `.go` name. Full import_dir (build tags / NoGo) runs
        // once in the main loop — do not scan every directory twice.
        if dir_has_go_file(&dir) {
            out.push(dir.clone());
        }
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(())
}

fn dir_has_go_file(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_name()
            .to_string_lossy()
            .ends_with(".go")
    })
}

fn abs_join(dir: &Path, files: &[String]) -> Vec<PathBuf> {
    files.iter().map(|f| dir.join(f)).collect()
}

fn abs_dir(dir: &Path) -> Result<PathBuf, Bail> {
    if dir.as_os_str().is_empty() {
        return std::env::current_dir().map_err(|e| Bail::new(BailReason::Io, e.to_string()));
    }
    dir.canonicalize()
        .or_else(|_| Ok(dir.to_path_buf()))
        .map_err(|e: std::io::Error| Bail::new(BailReason::Io, e.to_string()))
}

fn transitive_deps(id: &str, direct: &FxHashMap<String, Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = FxHashSet::default();
    let mut stack: Vec<String> = direct.get(id).cloned().unwrap_or_default();
    while let Some(dep) = stack.pop() {
        if dep == id || dep == "C" {
            continue;
        }
        if !seen.insert(dep.clone()) {
            continue;
        }
        out.push(dep.clone());
        if let Some(next) = direct.get(&dep) {
            stack.extend(next.iter().cloned());
        }
    }
    out.sort();
    out
}
