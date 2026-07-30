//! Walk the package graph from patterns using `guff-build` + module resolution.

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use guff_build::go_source::parse_go_file_info;
use guff_build::{find_module_root, Context, ModFile};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::bail::{Bail, BailReason};
use crate::modcache::ModCache;
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
        let build_pkg = ctxt.import_dir(dir).map_err(|e| {
            Bail::new(
                BailReason::Io,
                format!("import_dir {}: {e}", dir.display()),
            )
        })?;
        if !build_pkg.cgo_files.is_empty() {
            // C-3e: keep listing with GoFiles only; compiled cgo output is
            // attached later via go list when available. Do not bail the graph.
        }
        let (pkg_path, module) = package_identity(dir, &workspace)?;
        if !seen.insert(pkg_path.clone()) {
            continue;
        }
        queue.push_back((pkg_path, dir.clone(), module, true));
    }

    while let Some((pkg_path, dir, module, is_root)) = queue.pop_front() {
        let build_pkg = match ctxt.import_dir(&dir) {
            Ok(p) => p,
            Err(e) => {
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
                        dep_only: !is_root,
                    },
                );
                let _ = e;
                if is_root {
                    response.roots.push(pkg_path);
                }
                continue;
            }
        };
        if !build_pkg.cgo_files.is_empty() {
            // See root-path note: GoFiles-only listing is fine for the graph.
        }

        let go_files = abs_join(&build_pkg.dir, &build_pkg.go_files);
        // Match go list's Package.GoFiles which merges CgoFiles into GoFiles.
        let mut go_files_with_cgo = go_files.clone();
        go_files_with_cgo.extend(abs_join(&build_pkg.dir, &build_pkg.cgo_files));
        let mut compiled = go_files_with_cgo.clone();
        // Only root packages get test files merged (go list `-test` applies to
        // the query roots, not the whole dependency closure).
        if cfg.tests && is_root {
            compiled.extend(abs_join(&build_pkg.dir, &build_pkg.test_go_files));
        }
        let ignored = abs_join(&build_pkg.dir, &build_pkg.ignored_go_files);
        // Imports come from GoFiles + CgoFiles (and tests when applicable).
        let imports = collect_imports(if cfg.tests && is_root {
            &compiled
        } else {
            &go_files_with_cgo
        });

        let mut import_list: Vec<(String, String)> = Vec::new();
        let mut direct_ids: Vec<String> = Vec::new();
        for import_path in &imports {
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
                go_files: go_files_with_cgo,
                compiled_go_files: compiled,
                ignored_files: ignored,
                imports: import_list,
                deps: Vec::new(), // filled below
                module: list_mod,
                standard: module.standard,
                dep_only: !is_root,
            },
        );

        // External tests (`package foo_test`) as a separate package. Id matches
        // go list's non-variant form closely enough for analysis; full
        // `foo_test [foo.test]` variants remain v2.
        if cfg.tests && is_root && !build_pkg.xtest_go_files.is_empty() {
            let xtest_id = format!("{pkg_path}_test");
            if seen.insert(xtest_id.clone()) {
                let xtest_files = abs_join(&build_pkg.dir, &build_pkg.xtest_go_files);
                let mut xtest_imports = collect_imports(&xtest_files);
                if !xtest_imports.iter().any(|i| i == &pkg_path) {
                    xtest_imports.push(pkg_path.clone());
                }
                xtest_imports.sort();
                xtest_imports.dedup();
                let mut xtest_pairs: Vec<(String, String)> = Vec::new();
                let mut xtest_direct: Vec<String> = Vec::new();
                for import_path in &xtest_imports {
                    if import_path == "C" {
                        continue;
                    }
                    if import_path == "unsafe" {
                        xtest_pairs.push((import_path.clone(), "unsafe".into()));
                        xtest_direct.push("unsafe".into());
                        ensure_unsafe(&mut packages, &mut seen);
                        continue;
                    }
                    let (dep_dir, dep_mod) = resolve_import(
                        import_path,
                        &workspace,
                        &cache,
                        &goroot,
                        false,
                    )?;
                    let dep_id = dep_mod.pkg_id.clone();
                    xtest_pairs.push((import_path.clone(), dep_id.clone()));
                    xtest_direct.push(dep_id.clone());
                    if cfg.need_deps && !seen.contains(&dep_id) {
                        if seen.insert(dep_id.clone()) {
                            queue.push_back((dep_id, dep_dir, dep_mod, false));
                        }
                    }
                }
                xtest_pairs.sort_by(|a, b| a.0.cmp(&b.0));
                xtest_direct.sort();
                xtest_direct.dedup();
                direct_imports.insert(xtest_id.clone(), xtest_direct);
                packages.insert(
                    xtest_id.clone(),
                    ListPackage {
                        id: xtest_id.clone(),
                        name: format!("{}_test", build_pkg.name),
                        pkg_path: xtest_id.clone(),
                        dir: build_pkg.dir.clone(),
                        go_files: xtest_files.clone(),
                        compiled_go_files: xtest_files,
                        ignored_files: Vec::new(),
                        imports: xtest_pairs,
                        deps: Vec::new(),
                        module: if module.standard {
                            None
                        } else {
                            Some(to_list_module(&module))
                        },
                        standard: false,
                        dep_only: !is_root,
                    },
                );
                if is_root {
                    response.roots.push(xtest_id);
                }
            }
        }

        if is_root {
            response.roots.push(pkg_path);
        }
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
        },
    );
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
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
            if (name == "vendor" || name == "testdata" || name == "node_modules" || name.starts_with('.'))
                && dir != root
            {
                continue;
            }
        }
        if ctxt.import_dir(&dir).is_ok() {
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

fn collect_imports(go_files: &[PathBuf]) -> Vec<String> {
    let mut seen = FxHashSet::default();
    let mut out = Vec::new();
    for path in go_files {
        let Ok(content) = fs::read(path) else {
            continue;
        };
        let Ok(info) = parse_go_file_info(&content) else {
            continue;
        };
        for imp in info.imports {
            if seen.insert(imp.clone()) {
                out.push(imp);
            }
        }
    }
    out
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
