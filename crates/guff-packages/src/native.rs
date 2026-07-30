//! Native package driver (PERF_TASKS_V2 §C-3c) wrapping `guff-golist`.
//!
//! Feature flag: `GUFF_NATIVE_LIST`
//! - unset / `0` / `off` / `false` — prefer `go list` when available (**default**).
//!   When `go` is missing, native is still tried (go-less).
//! - `1` / `on` / `true` — try native first; bail → `go list`
//! - `verify` — run both when native succeeds; print graph diffs; use `go list`
//! - `force` — native only (error on bail; for tests / go-less CI)
//!
//! Default stays off until native grows a warm list cache and full `-test`
//! variants (root counts still differ: go list ~294 vs native ~118 on
//! prometheus). Nested-module `./...` skipping and C-3d stdlib-from-source
//! make `force` / go-less usable today.

use std::path::PathBuf;
use std::sync::Arc;

use guff_golist::{list_packages, Bail, BailReason, ListConfig, ListModule, ListPackage, ListResponse};

use crate::config::Config;
use crate::golist::{go_available, go_list_driver};
use crate::load_mode::LoadMode;
use crate::package::{DriverResponse, Module, Package};
use crate::typecheck::TypecheckEnv;
use crate::LoadError;

/// How [`native_or_golist`] chooses the driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeListMode {
    /// Always `go list` when available.
    Off,
    /// Prefer native; fall back on [`Bail`].
    On,
    /// Run both; report diffs; keep `go list` result.
    Verify,
    /// Native only (tests / go-less CI).
    Force,
}

impl NativeListMode {
    pub fn from_env() -> Self {
        match std::env::var("GUFF_NATIVE_LIST") {
            Ok(v) => match v.trim().to_ascii_lowercase().as_str() {
                "" | "0" | "false" | "off" | "no" => Self::Off,
                "1" | "true" | "on" | "yes" => Self::On,
                "verify" => Self::Verify,
                "force" => Self::Force,
                other => {
                    eprintln!(
                        "guff: warning: unknown GUFF_NATIVE_LIST={other:?}; treating as off"
                    );
                    Self::Off
                }
            },
            // Default off: warm `go list` stdout cache stays critical-path.
            // Go-less still uses native via the Off branch when `go` is absent.
            Err(_) => Self::Off,
        }
    }
}

/// Loads packages via native list and/or `go list` according to [`NativeListMode`].
pub fn native_or_golist(cfg: &Config, patterns: &[String]) -> Result<DriverResponse, LoadError> {
    let mode = NativeListMode::from_env();
    match mode {
        NativeListMode::Off => {
            if go_available() {
                go_list_driver(cfg, patterns).map_err(LoadError::from)
            } else {
                // Go-less: native is the only option that can see GOMODCACHE.
                load_native(cfg, patterns).or_else(|bail| {
                    Err(LoadError::Driver(format!(
                        "native list failed and go is unavailable: {bail}"
                    )))
                })
            }
        }
        NativeListMode::On => match load_native(cfg, patterns) {
            Ok(native) => {
                if go_available() {
                    Ok(attach_hybrid_exports(cfg, native)?)
                } else {
                    Ok(native)
                }
            }
            Err(bail) => {
                if crate::debug::enabled() {
                    eprintln!("guff:   native list bail → go list ({bail})");
                }
                if go_available() {
                    go_list_driver(cfg, patterns).map_err(LoadError::from)
                } else {
                    Err(LoadError::Driver(format!(
                        "native list bail and go unavailable: {bail}"
                    )))
                }
            }
        },
        NativeListMode::Verify => {
            let go_resp = if go_available() {
                go_list_driver(cfg, patterns).map_err(LoadError::from)?
            } else {
                return Err(LoadError::Driver(
                    "GUFF_NATIVE_LIST=verify requires go on PATH".into(),
                ));
            };
            match load_native(cfg, patterns) {
                Ok(native) => {
                    let native = if cfg.dep_source {
                        attach_hybrid_exports(cfg, native)?
                    } else {
                        native
                    };
                    let diffs = diff_responses(&native, &go_resp);
                    if diffs.is_empty() {
                        if crate::debug::enabled() {
                            eprintln!(
                                "guff:   native list verify OK ({} pkgs)",
                                go_resp.packages.len()
                            );
                        }
                    } else {
                        eprintln!(
                            "guff: native list verify DIFF ({} issues); using go list",
                            diffs.len()
                        );
                        for d in diffs.iter().take(40) {
                            eprintln!("guff:   native≠golist: {d}");
                        }
                        if diffs.len() > 40 {
                            eprintln!("guff:   ... and {} more", diffs.len() - 40);
                        }
                    }
                    Ok(go_resp)
                }
                Err(bail) => {
                    eprintln!("guff: native list verify bail → go list ({bail})");
                    Ok(go_resp)
                }
            }
        }
        NativeListMode::Force => load_native(cfg, patterns).map_err(|bail| {
            LoadError::Driver(format!("GUFF_NATIVE_LIST=force: {bail}"))
        }),
    }
}

fn load_native(cfg: &Config, patterns: &[String]) -> Result<DriverResponse, Bail> {
    let list_cfg = list_config_from(cfg)?;
    let resp = list_packages(&list_cfg, patterns)?;
    Ok(to_driver_response(cfg, resp))
}

fn list_config_from(cfg: &Config) -> Result<ListConfig, Bail> {
    let mut build_tags = Vec::new();
    for flag in &cfg.build_flags {
        if let Some(tags) = flag.strip_prefix("-tags=") {
            for tag in tags.split(',') {
                let tag = tag.trim();
                if !tag.is_empty() {
                    build_tags.push(tag.to_string());
                }
            }
        } else if flag == "-tags" {
            return Err(Bail::new(
                BailReason::UnsupportedBuildFlags,
                "-tags must be -tags=...",
            ));
        } else if flag.starts_with('-') {
            return Err(Bail::new(
                BailReason::UnsupportedBuildFlags,
                format!("unsupported build flag {flag:?}"),
            ));
        }
    }
    let mode = cfg.effective_mode();
    let need_deps = mode.contains(LoadMode::NEED_DEPS)
        || mode.contains(LoadMode::NEED_TYPES)
        || mode.contains(LoadMode::NEED_TYPES_INFO);
    Ok(ListConfig {
        dir: cfg.dir.clone(),
        build_tags,
        tests: cfg.tests,
        need_deps,
        gomodcache: None,
        goroot: None,
    })
}

fn to_driver_response(cfg: &Config, resp: ListResponse) -> DriverResponse {
    let env = cfg.resolved_env();
    let arch = TypecheckEnv::from_env(&env, "gc").arch;
    DriverResponse {
        compiler: resp.compiler,
        arch,
        roots: resp.roots,
        packages: resp
            .packages
            .into_iter()
            .map(|p| Arc::new(to_package(p)))
            .collect(),
        ..DriverResponse::default()
    }
}

fn to_package(p: ListPackage) -> Package {
    let mut imports = std::collections::HashMap::new();
    for (src, id) in &p.imports {
        imports.insert(
            src.clone(),
            Arc::new(Package {
                id: id.clone(),
                pkg_path: id.clone(),
                ..Package::default()
            }),
        );
    }
    Package {
        id: p.id,
        name: p.name,
        pkg_path: p.pkg_path,
        dir: p.dir,
        go_files: p.go_files,
        compiled_go_files: p.compiled_go_files,
        ignored_files: p.ignored_files,
        imports,
        deps: p.deps,
        module: p.module.map(to_module),
        ..Package::default()
    }
}

fn to_module(m: ListModule) -> Module {
    Module {
        path: m.path,
        version: m.version,
        replace: None,
        main: m.main,
        indirect: m.indirect,
        dir: m.dir,
        go_mod: m.go_mod,
        go_version: m.go_version,
        error: None,
    }
}

/// Native graphs keep stdlib on the source path (C-3d). Cgo `CompiledGoFiles`
/// still come from `go list` when native bails; when native succeeds with cgo
/// packages present, `GoFiles` (which already include `*.go` from cgo) are used
/// until C-3e wires a dedicated compiled-files attach.
fn attach_hybrid_exports(
    cfg: &Config,
    response: DriverResponse,
) -> Result<DriverResponse, LoadError> {
    let _ = cfg;
    Ok(response)
}

/// Normalize + diff two driver responses for `GUFF_NATIVE_LIST=verify`.
pub fn diff_responses(native: &DriverResponse, golist: &DriverResponse) -> Vec<String> {
    let mut diffs = Vec::new();

    let mut native_roots = native.roots.clone();
    let mut go_roots = golist.roots.clone();
    native_roots.sort();
    go_roots.sort();
    if native_roots != go_roots {
        diffs.push(format!(
            "roots: native={native_roots:?} golist={go_roots:?}"
        ));
    }

    let native_by: std::collections::BTreeMap<&str, &Package> = native
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.as_ref()))
        .collect();
    let go_by: std::collections::BTreeMap<&str, &Package> = golist
        .packages
        .iter()
        .map(|p| (p.id.as_str(), p.as_ref()))
        .collect();

    for id in go_by.keys() {
        if !native_by.contains_key(id) {
            // go list includes more (e.g. incomplete packages); only flag
            // packages that matter for typecheck of roots' closure.
            diffs.push(format!("missing in native: {id}"));
        }
    }
    for id in native_by.keys() {
        if !go_by.contains_key(id) {
            diffs.push(format!("extra in native: {id}"));
        }
    }

    for (id, np) in &native_by {
        let Some(gp) = go_by.get(id) else {
            continue;
        };
        if np.name != gp.name {
            diffs.push(format!("{id}: name {:?} vs {:?}", np.name, gp.name));
        }
        if np.pkg_path != gp.pkg_path {
            diffs.push(format!(
                "{id}: pkg_path {:?} vs {:?}",
                np.pkg_path, gp.pkg_path
            ));
        }
        if norm_files(&np.compiled_go_files) != norm_files(&gp.compiled_go_files) {
            // Cgo / generated: ignore when GoFiles match (CompiledGoFiles may
            // include go tool output native listing does not see).
            let go_same = norm_files(&np.go_files) == norm_files(&gp.go_files);
            if !go_same
                && norm_files(&np.go_files) != norm_files(&gp.compiled_go_files)
                && norm_files(&np.compiled_go_files) != norm_files(&gp.go_files)
            {
                diffs.push(format!(
                    "{id}: compiled_go_files native={} golist={}",
                    np.compiled_go_files.len(),
                    gp.compiled_go_files.len()
                ));
            }
        }
        let ni: std::collections::BTreeSet<_> = np.imports.keys().collect();
        let gi: std::collections::BTreeSet<_> = gp.imports.keys().collect();
        if ni != gi {
            diffs.push(format!("{id}: imports native={ni:?} golist={gi:?}"));
        }
        let nd = norm_deps(&np.deps);
        let gd = norm_deps(&gp.deps);
        if nd != gd {
            // Transitive deps sets can differ in ordering only — already sorted.
            // Also ignore "unsafe" presence mismatches (both paths handle it).
            let nd2: std::collections::BTreeSet<_> =
                nd.iter().filter(|d| d.as_str() != "unsafe").cloned().collect();
            let gd2: std::collections::BTreeSet<_> =
                gd.iter().filter(|d| d.as_str() != "unsafe").cloned().collect();
            if nd2 != gd2 {
                diffs.push(format!(
                    "{id}: deps Δ native_only={:?} golist_only={:?}",
                    nd2.difference(&gd2).collect::<Vec<_>>(),
                    gd2.difference(&nd2).collect::<Vec<_>>()
                ));
            }
        }
        match (&np.module, &gp.module) {
            (Some(nm), Some(gm)) if nm.path != gm.path || nm.main != gm.main => {
                diffs.push(format!(
                    "{id}: module path/main {:?} vs {:?}",
                    (nm.path.as_str(), nm.main),
                    (gm.path.as_str(), gm.main)
                ));
            }
            (None, Some(gm)) if !gm.path.is_empty() && !is_stdlib_id(id) => {
                diffs.push(format!("{id}: module missing in native (golist {})", gm.path));
            }
            _ => {}
        }
        // export_file intentionally ignored (C-3d).
    }

    diffs
}

fn norm_files(files: &[PathBuf]) -> Vec<String> {
    let mut v: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    v.sort();
    v
}

fn norm_deps(deps: &[String]) -> Vec<String> {
    let mut v = deps.to_vec();
    v.sort();
    v.dedup();
    v
}

fn is_stdlib_id(id: &str) -> bool {
    !id.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_mode::LoadMode;

    fn golist_testdata() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/golist")
    }

    #[test]
    fn native_force_loads_mini_module() {
        let dir = golist_testdata();
        let cfg = Config {
            mode: LoadMode::NEED_NAME
                | LoadMode::NEED_FILES
                | LoadMode::NEED_COMPILED_GO_FILES
                | LoadMode::NEED_IMPORTS
                | LoadMode::NEED_DEPS
                | LoadMode::NEED_MODULE,
            dir: dir.clone(),
            ..Config::default()
        };
        let resp = load_native(&cfg, &[".".to_string()]).expect("native list");
        assert_eq!(resp.roots, vec!["example.com/golist".to_string()]);
        let pkg = resp
            .packages
            .iter()
            .find(|p| p.id == "example.com/golist")
            .expect("main");
        assert_eq!(pkg.name, "main");
        assert!(pkg.imports.contains_key("fmt"));
        assert!(
            resp.packages.iter().any(|p| p.id == "fmt"),
            "fmt should be loaded from GOROOT"
        );
    }

    #[test]
    fn native_driver_trait() {
        let dir = golist_testdata();
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            dir,
            ..Config::default()
        };
        // Force path via direct call (env not required).
        let resp = load_native(&cfg, &[".".to_string()]).unwrap();
        assert!(!resp.packages.is_empty());
    }
}
