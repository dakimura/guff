//! Offline / `go`-less package driver using `guff-build`.
//!
//! Port of the deferred Phase-2 fallback from PRE-LINTER-PLAN (PL02): when the
//! `go` binary is unavailable, resolve packages from `go.mod` + the filesystem
//! (module-local and GOROOT) without shelling out to `go list`.
//!
//! DEFERRED: external module deps from `go.mod` `require` (needs module cache /
//! `go mod download`); export-data population (needs a Go toolchain build).

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use guff_build::go_source::parse_go_file_info;
use guff_build::{find_module_root, parse_mod_file, Context, ImportMode, ModFile};

use crate::config::Config;
use crate::package::{DriverResponse, Error, ErrorKind, Module, Package};
use crate::typecheck::TypecheckEnv;
use crate::LoadError;

/// Package driver that never invokes the `go` binary.
#[derive(Debug, Default, Clone, Copy)]
pub struct OfflineDriver;

impl crate::driver::Driver for OfflineDriver {
    fn load(&self, cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
        offline_driver(cfg, patterns)
    }
}

/// Loads packages by walking the module filesystem via [`guff_build::Context`].
pub fn offline_driver(cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
    let ctxt = build_context(cfg);
    let src_dir = abs_cfg_dir(cfg)?;
    let module_root = find_module_root(&src_dir);
    let mod_file = module_root
        .as_ref()
        .and_then(|root| parse_mod_file(&root.join("go.mod")).ok());

    let roots = expand_patterns(&ctxt, &src_dir, module_root.as_deref(), mod_file.as_ref(), patterns)?;
    if roots.is_empty() {
        return Err(LoadError::Driver(
            "offline driver: no packages matched patterns".into(),
        ));
    }

    let env = cfg.resolved_env();
    let arch = TypecheckEnv::from_env(&env, "gc").arch;
    let mut response = DriverResponse {
        compiler: "gc".into(),
        arch,
        ..DriverResponse::default()
    };

    let mode = cfg.effective_mode();
    let need_imports = mode.contains(crate::load_mode::LoadMode::NEED_IMPORTS)
        || mode.contains(crate::load_mode::LoadMode::NEED_TYPES)
        || mode.contains(crate::load_mode::LoadMode::NEED_TYPES_INFO)
        || mode.contains(crate::load_mode::LoadMode::NEED_SYNTAX);
    // Recurse into imports' imports only when NeedDeps is set (stdlib walk is
    // otherwise huge without export data).
    let need_deps = mode.contains(crate::load_mode::LoadMode::NEED_DEPS);

    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<(String, PathBuf, bool)> = VecDeque::new();
    for dir in &roots {
        match ctxt.import_dir(dir) {
            Ok(build_pkg) => {
                let id = if build_pkg.import_path.is_empty() || build_pkg.import_path == "." {
                    dir.display().to_string()
                } else {
                    build_pkg.import_path.clone()
                };
                if seen.insert(id.clone()) {
                    queue.push_back((id, dir.clone(), true));
                }
            }
            Err(err) => {
                return Err(LoadError::Driver(format!(
                    "offline driver: {}: {err}",
                    dir.display()
                )));
            }
        }
    }

    let mut packages: Vec<Arc<Package>> = Vec::new();
    let mut root_ids = Vec::new();

    while let Some((id, dir, is_root)) = queue.pop_front() {
        let build_pkg = match ctxt.import_dir(&dir) {
            Ok(p) => p,
            Err(err) => {
                packages.push(Arc::new(Package {
                    id: id.clone(),
                    pkg_path: id.clone(),
                    dir: dir.clone(),
                    errors: vec![Error {
                        pos: dir.display().to_string(),
                        msg: err.to_string(),
                        kind: ErrorKind::List,
                    }],
                    ..Package::default()
                }));
                if is_root {
                    root_ids.push(id);
                }
                continue;
            }
        };

        let pkg_path = if build_pkg.import_path.is_empty() || build_pkg.import_path == "." {
            id.clone()
        } else {
            build_pkg.import_path.clone()
        };

        let go_files = abs_join(&build_pkg.dir, &build_pkg.go_files);
        let mut compiled = go_files.clone();
        if cfg.tests {
            compiled.extend(abs_join(&build_pkg.dir, &build_pkg.test_go_files));
        }

        let import_paths = if need_imports {
            collect_imports(&go_files)
        } else {
            Vec::new()
        };

        let mut imports = HashMap::new();
        let mut deps = Vec::new();
        for import_path in &import_paths {
            if import_path == "C" || import_path == "unsafe" {
                if import_path == "unsafe" {
                    deps.push(import_path.clone());
                    imports.insert(
                        import_path.clone(),
                        Arc::new(Package {
                            id: "unsafe".into(),
                            pkg_path: "unsafe".into(),
                            name: "unsafe".into(),
                            ..Package::default()
                        }),
                    );
                }
                continue;
            }
            deps.push(import_path.clone());
            imports.insert(
                import_path.clone(),
                Arc::new(Package {
                    id: import_path.clone(),
                    ..Package::default()
                }),
            );

            if (is_root || need_deps) && !seen.contains(import_path) {
                match ctxt.import(import_path, &build_pkg.dir, ImportMode::FIND_ONLY) {
                    Ok(dep) => {
                        if seen.insert(import_path.clone()) {
                            queue.push_back((import_path.clone(), dep.dir, false));
                        }
                    }
                    Err(_err) => {
                        // External module deps are DEFERRED (no module cache walk).
                    }
                }
            }
        }
        deps.sort();
        deps.dedup();

        let module = mod_file.as_ref().and_then(|m| {
            module_root.as_ref().map(|root| Module {
                path: m.module_path.clone(),
                version: String::new(),
                replace: None,
                main: pkg_path == m.module_path || pkg_path.starts_with(&format!("{}/", m.module_path)),
                indirect: false,
                dir: root.clone(),
                go_mod: root.join("go.mod"),
                go_version: m.go_version.clone().unwrap_or_default(),
                error: None,
            })
        });

        let pkg = Package {
            id: pkg_path.clone(),
            name: build_pkg.name.clone(),
            pkg_path: pkg_path.clone(),
            dir: build_pkg.dir.clone(),
            go_files,
            compiled_go_files: compiled,
            ignored_files: abs_join(&build_pkg.dir, &build_pkg.ignored_go_files),
            imports,
            deps,
            module,
            ..Package::default()
        };

        if is_root {
            root_ids.push(pkg.id.clone());
        }
        packages.push(Arc::new(pkg));
    }

    // Ensure unresolved import stubs that were never queued still appear so
    // refine() can leave them as stubs (external modules).
    response.roots = root_ids;
    response.packages = packages;
    Ok(response)
}

fn build_context(cfg: &Config) -> Context {
    let mut ctxt = Context::default();
    for flag in &cfg.build_flags {
        if let Some(tags) = flag.strip_prefix("-tags=") {
            for tag in tags.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() && !ctxt.build_tags.iter().any(|t| t == tag) {
                    ctxt.build_tags.push(tag.to_string());
                }
            }
        }
    }
    ctxt
}

fn abs_cfg_dir(cfg: &Config) -> Result<PathBuf, LoadError> {
    if cfg.dir.as_os_str().is_empty() {
        return std::env::current_dir().map_err(|e| LoadError::Driver(e.to_string()));
    }
    Ok(cfg
        .dir
        .canonicalize()
        .unwrap_or_else(|_| cfg.dir.clone()))
}

fn expand_patterns(
    ctxt: &Context,
    src_dir: &Path,
    module_root: Option<&Path>,
    mod_file: Option<&ModFile>,
    patterns: &[String],
) -> Result<Vec<PathBuf>, LoadError> {
    let patterns = if patterns.is_empty() {
        vec![".".to_string()]
    } else {
        patterns.to_vec()
    };

    let mut dirs = Vec::new();
    for pattern in &patterns {
        if pattern == "./..." || pattern == "..." {
            let root = module_root.unwrap_or(src_dir);
            walk_packages(ctxt, root, &mut dirs)?;
            continue;
        }
        if pattern.ends_with("/...") {
            let prefix = pattern.trim_end_matches("/...");
            let dir = resolve_pattern_dir(ctxt, src_dir, module_root, mod_file, prefix)?;
            walk_packages(ctxt, &dir, &mut dirs)?;
            continue;
        }
        let dir = resolve_pattern_dir(ctxt, src_dir, module_root, mod_file, pattern)?;
        dirs.push(dir);
    }

    dirs.sort();
    dirs.dedup();
    Ok(dirs)
}

fn resolve_pattern_dir(
    ctxt: &Context,
    src_dir: &Path,
    module_root: Option<&Path>,
    mod_file: Option<&ModFile>,
    pattern: &str,
) -> Result<PathBuf, LoadError> {
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
        return dir
            .canonicalize()
            .map_err(|e| LoadError::Driver(format!("offline driver: {pattern}: {e}")));
    }
    if let (Some(root), Some(m)) = (module_root, mod_file) {
        if pattern == m.module_path || pattern.starts_with(&format!("{}/", m.module_path)) {
            if let Ok(pkg) = ctxt.import(pattern, src_dir, ImportMode::FIND_ONLY) {
                return Ok(pkg.dir);
            }
            let rel = pattern
                .strip_prefix(&m.module_path)
                .unwrap_or(pattern)
                .trim_start_matches('/');
            let dir = if rel.is_empty() {
                root.to_path_buf()
            } else {
                root.join(rel)
            };
            if dir.is_dir() {
                return Ok(dir);
            }
        }
    }
    match ctxt.import(pattern, src_dir, ImportMode::FIND_ONLY) {
        Ok(pkg) => Ok(pkg.dir),
        Err(err) => Err(LoadError::Driver(format!(
            "offline driver: cannot resolve {pattern:?}: {err}"
        ))),
    }
}

fn walk_packages(ctxt: &Context, root: &Path, out: &mut Vec<PathBuf>) -> Result<(), LoadError> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        // Skip common non-package trees.
        if let Some(name) = dir.file_name().and_then(|s| s.to_str()) {
            if name == "vendor" || name == "testdata" || name == "node_modules" || name.starts_with('.')
            {
                if dir != root {
                    continue;
                }
            }
        }

        match ctxt.import_dir(&dir) {
            Ok(_) => out.push(dir.clone()),
            Err(_) => {
                // Directory may only contain subpackages.
            }
        }

        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
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
    let mut seen = HashSet::new();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Driver;
    use crate::load_mode::LoadMode;

    fn golist_testdata() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/golist")
    }

    #[test]
    fn offline_loads_mini_module_without_go() {
        let dir = golist_testdata();
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir: dir.clone(),
            ..Config::default()
        };
        let response = OfflineDriver.load(&cfg, &[".".to_string()]).expect("load");
        assert_eq!(response.roots.len(), 1);
        let pkg = response
            .packages
            .iter()
            .find(|p| p.id == "example.com/golist")
            .expect("main package");
        assert_eq!(pkg.name, "main");
        assert!(pkg.go_files.iter().any(|f| f.ends_with("main.go")));
        assert!(pkg.imports.contains_key("fmt"));
    }

    #[test]
    fn offline_resolves_fmt_from_goroot() {
        let dir = golist_testdata();
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir,
            ..Config::default()
        };
        let response = OfflineDriver.load(&cfg, &[".".to_string()]).expect("load");
        let fmt = response
            .packages
            .iter()
            .find(|p| p.pkg_path == "fmt")
            .expect("fmt package from GOROOT");
        assert_eq!(fmt.name, "fmt");
        assert!(!fmt.go_files.is_empty());
    }
}
