//! [`load`] orchestration.
//!
//! Port of `packages.Load` and the non-typecheck portion of `refine` from `packages.go`.

use crate::hash::HashMap;
use std::sync::Arc;

use crate::config::Config;
use crate::driver::{default_driver, Driver};
use crate::load_mode::LoadMode;
use crate::package::{DriverResponse, Package};
use crate::typecheck::{typecheck_packages, TypecheckEnv};

/// Errors from [`load`].
#[derive(Debug)]
pub enum LoadError {
    Driver(String),
    MissingRoot(String),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Driver(msg) => write!(f, "{msg}"),
            Self::MissingRoot(id) => write!(f, "root package {id} is missing"),
        }
    }
}

impl std::error::Error for LoadError {}

/// Loads Go packages named by `patterns`.
///
/// Equivalent to `packages.Load`. Type checking and syntax parsing are deferred
/// to Phase 4; this skeleton wires the driver, import graph, and field clearing.
pub fn load(cfg: &Config, patterns: &[String]) -> Result<Vec<Arc<Package>>, LoadError> {
    load_with_driver(cfg, patterns, &default_driver())
}

/// Like [`load`] but also returns every loaded package (roots and transitive
/// dependencies), not just the roots. The full set lets callers build a
/// complete dependency-hash registry for the issues cache.
pub fn load_graph(
    cfg: &Config,
    patterns: &[String],
) -> Result<(Vec<Arc<Package>>, Vec<Arc<Package>>), LoadError> {
    load_graph_with_driver(cfg, patterns, &default_driver())
}

/// Like [`load`] but accepts a custom [`Driver`] (for tests).
pub fn load_with_driver<D: Driver>(
    cfg: &Config,
    patterns: &[String],
    driver: &D,
) -> Result<Vec<Arc<Package>>, LoadError> {
    load_graph_with_driver(cfg, patterns, driver).map(|(roots, _all)| roots)
}

/// Like [`load_graph`] but accepts a custom [`Driver`] (for tests).
pub fn load_graph_with_driver<D: Driver>(
    cfg: &Config,
    patterns: &[String],
    driver: &D,
) -> Result<(Vec<Arc<Package>>, Vec<Arc<Package>>), LoadError> {
    let requested_mode = cfg.mode.normalize();
    let effective_cfg = Config {
        mode: requested_mode.implied(),
        ..cfg.clone()
    };

    let response = driver.load(&effective_cfg, patterns)?;
    refine(&effective_cfg, requested_mode, response)
}

fn refine(
    cfg: &Config,
    requested_mode: LoadMode,
    response: DriverResponse,
) -> Result<(Vec<Arc<Package>>, Vec<Arc<Package>>), LoadError> {
    let timing = crate::debug::enabled();
    let detail = crate::debug::detailed();
    let t_refine = std::time::Instant::now();
    let mut by_id: HashMap<String, Arc<Package>> = HashMap::default();
    for pkg in response.packages {
        by_id.insert(pkg.id.clone(), pkg);
    }

    if requested_mode.contains(LoadMode::NEED_IMPORTS)
        || requested_mode.contains(LoadMode::NEED_SYNTAX)
        || requested_mode.contains(LoadMode::NEED_TYPES)
        || requested_mode.contains(LoadMode::NEED_TYPES_INFO)
    {
        connect_imports(&mut by_id);
        if timing {
            eprintln!(
                "guff:   refine connect_imports {:.2}s ({} pkgs)",
                t_refine.elapsed().as_secs_f64(),
                by_id.len(),
            );
        }
    } else {
        for pkg in by_id.values_mut() {
            Arc::make_mut(pkg).imports.clear();
        }
    }

    if crate::typecheck::needs_typecheck(requested_mode) {
        let mut env = TypecheckEnv::from_env(&cfg.resolved_env(), &response.compiler);
        env.from_source = cfg.dep_source;
        typecheck_packages(&mut by_id, &response.roots, requested_mode, &env);
    }

    // Clear against the *implied* mode so fields fetched because of implication
    // (e.g. NEED_MODULE via NEED_TYPES) are retained — matching go/packages.
    clear_unrequested_fields(&mut by_id, requested_mode.implied());

    let mut roots = Vec::with_capacity(response.roots.len());
    for root in response.roots {
        match by_id.get(&root) {
            Some(pkg) => roots.push(Arc::clone(pkg)),
            None => return Err(LoadError::MissingRoot(root)),
        }
    }

    // All loaded packages, in a deterministic order (by id), for callers that
    // need the full dependency set (e.g. the issues-cache hash registry).
    let mut all: Vec<Arc<Package>> = by_id.into_values().collect();
    all.sort_by(|a, b| a.id.cmp(&b.id));

    if detail {
        // The level-1 line only covers `connect_imports`; this is the whole of
        // `refine` (index, connect, clear-unrequested, root lookup, id sort), so
        // the gap against `phase load_graph` is attributable.
        eprintln!(
            "guff:     refine total {:.2}s ({} pkgs, {} roots)",
            t_refine.elapsed().as_secs_f64(),
            all.len(),
            roots.len(),
        );
    }

    let _ = cfg;
    Ok((roots, all))
}

fn connect_imports(by_id: &mut HashMap<String, Arc<Package>>) {
    let ids: Vec<String> = by_id.keys().cloned().collect();
    for id in ids {
        let Some(pkg) = by_id.get(&id).cloned() else {
            continue;
        };
        let import_paths: Vec<String> = pkg.imports.keys().cloned().collect();
        for path in import_paths {
            let stub_id = pkg.imports.get(&path).map(|p| p.id.clone());
            let Some(stub_id) = stub_id else { continue };
            if let Some(resolved) = by_id.get(&stub_id).cloned() {
                Arc::make_mut(by_id.get_mut(&id).expect("pkg"))
                    .imports
                    .insert(path, resolved);
            }
        }
    }
}

fn clear_unrequested_fields(by_id: &mut HashMap<String, Arc<Package>>, mode: LoadMode) {
    for pkg in by_id.values_mut() {
        let pkg = Arc::make_mut(pkg);
        if !mode.contains(LoadMode::NEED_NAME) {
            pkg.name.clear();
            pkg.pkg_path.clear();
        }
        if !mode.contains(LoadMode::NEED_FILES) {
            pkg.go_files.clear();
            pkg.other_files.clear();
            pkg.ignored_files.clear();
        }
        if !mode.contains(LoadMode::NEED_EMBED_FILES) {
            pkg.embed_files.clear();
        }
        if !mode.contains(LoadMode::NEED_EMBED_PATTERNS) {
            pkg.embed_patterns.clear();
        }
        if !mode.contains(LoadMode::NEED_COMPILED_GO_FILES) {
            pkg.compiled_go_files.clear();
        }
        if !mode.contains(LoadMode::NEED_IMPORTS) {
            pkg.imports.clear();
        }
        if !mode.contains(LoadMode::NEED_EXPORT_FILE) {
            pkg.export_file = Default::default();
        }
        if !mode.contains(LoadMode::NEED_TYPES) {
            pkg.types = None;
            pkg.type_artifacts = None;
            pkg.ill_typed = false;
        }
        if !mode.contains(LoadMode::NEED_SYNTAX) {
            pkg.syntax.clear();
        }
        if !mode.contains(LoadMode::NEED_SYNTAX)
            && !mode.contains(LoadMode::NEED_TYPES)
            && !mode.contains(LoadMode::NEED_TYPES_INFO)
        {
            pkg.fset = None;
        }
        if !mode.contains(LoadMode::NEED_TYPES_INFO) {
            pkg.types_info = None;
        }
        if !mode.contains(LoadMode::NEED_TYPES_SIZES) {
            pkg.types_sizes = None;
        }
        if !mode.contains(LoadMode::NEED_MODULE) {
            pkg.module = None;
        }
        if !mode.contains(LoadMode::NEED_TARGET) {
            pkg.target = Default::default();
        }
        if !mode.contains(LoadMode::NEED_FOR_TEST) {
            pkg.for_test.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::Driver;
    use crate::package::DriverResponse;
    use std::collections::HashMap;

    struct FakeDriver {
        response: DriverResponse,
    }

    impl Driver for FakeDriver {
        fn load(&self, _cfg: &Config, _patterns: &[String]) -> Result<DriverResponse, LoadError> {
            Ok(self.response.clone())
        }
    }

    fn sample_response() -> DriverResponse {
        let a = Arc::new(Package {
            id: "example.com/a".into(),
            pkg_path: "example.com/a".into(),
            name: "a".into(),
            go_files: vec!["a.go".into()],
            ..Package::default()
        });
        let b = Arc::new(Package {
            id: "example.com/b".into(),
            pkg_path: "example.com/b".into(),
            name: "b".into(),
            go_files: vec!["b.go".into()],
            imports: HashMap::from([(
                "example.com/a".into(),
                Arc::new(Package {
                    id: "example.com/a".into(),
                    ..Package::default()
                }),
            )]),
            ..Package::default()
        });
        DriverResponse {
            roots: vec!["example.com/b".into()],
            packages: vec![a.clone(), b],
            ..DriverResponse::default()
        }
    }

    #[test]
    fn load_mode_union_from_two_linter_presets() {
        let ast_only = LoadMode::NEED_NAME | LoadMode::NEED_FILES | LoadMode::NEED_COMPILED_GO_FILES;
        let types =
            LoadMode::NEED_TYPES | LoadMode::NEED_TYPES_INFO | LoadMode::NEED_SYNTAX;
        let union = LoadMode::union_all(&[ast_only, types]);
        assert!(union.contains(LoadMode::NEED_NAME));
        assert!(union.contains(LoadMode::NEED_TYPES_INFO));
    }

    #[test]
    fn refine_connects_import_stubs() {
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            ..Config::default()
        };
        let roots = load_with_driver(
            &cfg,
            &[".".to_string()],
            &FakeDriver {
                response: sample_response(),
            },
        )
            .expect("load");
        assert_eq!(roots.len(), 1);
        let imp = roots[0]
            .imports
            .get("example.com/a")
            .expect("import");
        assert_eq!(imp.name, "a");
    }

    #[test]
    fn refine_clears_unrequested_name() {
        let cfg = Config {
            mode: LoadMode::NEED_FILES,
            ..Config::default()
        };
        let roots = load_with_driver(
            &cfg,
            &[".".to_string()],
            &FakeDriver {
                response: sample_response(),
            },
        )
            .expect("load");
        assert!(roots[0].name.is_empty());
        assert!(!roots[0].go_files.is_empty());
    }
}
