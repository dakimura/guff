//! [`load`] orchestration.
//!
//! Port of `packages.Load` and the non-typecheck portion of `refine` from `packages.go`.

use crate::hash::{HashMap, HashSet};
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
    mut response: DriverResponse,
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

    let keep = surviving_ids(&by_id);
    crate::dedup::carry_production_deps(&mut by_id, &keep);
    by_id.retain(|id, _| keep.contains(id));
    response.roots.retain(|id| keep.contains(id));

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

/// Which package ids survive the load.
///
/// golangci-lint drops the plain `P` when `P [P.test]` exists (and drops
/// synthetic testmains). Without this, unused / ineffassign / friends report
/// false positives on the prod-only view of packages that have tests.
fn surviving_ids(by_id: &HashMap<String, Arc<Package>>) -> HashSet<String> {
    let all_pkgs: Vec<Arc<Package>> = by_id.values().cloned().collect();
    crate::dedup::filter_test_main_packages(crate::dedup::filter_duplicate_packages(all_pkgs))
        .into_iter()
        .map(|p| p.id.clone())
        .collect()
}

/// The shape [`refine`] would give this driver response — which packages exist
/// and which of them are roots — without type-checking anything.
///
/// C-7 speculation guesses the graph from a disk cache and starts building the
/// seed against it while the authoritative load runs; the seed is only usable
/// if the guess reproduces the real graph exactly
/// (`SpeculativeSeed::matches`). A raw driver response is not that graph:
/// prometheus `./...` lists 1792 packages and 293 root-ish entries, which
/// `refine` narrows to 1616 and 118. Speculating from the raw response
/// therefore missed on the target list every single time — measured on
/// 2026-08-15, on the `go list` path C-7 was written for, so this had never
/// hit for anyone.
///
/// Shares `connect_imports` and [`surviving_ids`] with `refine` so the two
/// cannot drift into disagreeing about what the graph is.
pub(crate) fn peeked_graph_shape(
    response: DriverResponse,
) -> (Vec<String>, Vec<Arc<Package>>) {
    let mut by_id: HashMap<String, Arc<Package>> = HashMap::default();
    for pkg in response.packages {
        by_id.insert(pkg.id.clone(), pkg);
    }
    connect_imports(&mut by_id);

    let keep = surviving_ids(&by_id);
    crate::dedup::carry_production_deps(&mut by_id, &keep);
    by_id.retain(|id, _| keep.contains(id));

    let mut roots: Vec<String> = response
        .roots
        .into_iter()
        .filter(|id| keep.contains(id))
        .collect();
    roots.sort();
    roots.dedup();

    let mut all: Vec<Arc<Package>> = by_id.into_values().collect();
    all.sort_by(|a, b| a.id.cmp(&b.id));
    (roots, all)
}

/// Resolve one package's import edge to the id it should point at, the way
/// [`connect_imports`] does. `None` when the response holds no such package.
fn resolved_import_id(
    by_id: &HashMap<String, Arc<Package>>,
    stub_id: &str,
    path: &str,
) -> Option<String> {
    if by_id.contains_key(stub_id) {
        return Some(stub_id.to_string());
    }
    // After filter_duplicate_packages, plain `P` may be gone while
    // `P [P.test]` remains — resolve stub id / import path via pkg_path.
    crate::dedup::package_for_import_path(by_id, stub_id)
        .or_else(|| crate::dedup::package_for_import_path(by_id, path))
        .map(|p| p.id.clone())
}

/// Ids in dependency-first order: a package appears after everything it
/// imports.
///
/// [`connect_imports`] rewrites one package at a time, and each rewrite stores
/// an `Arc` to the *current* version of the neighbour. Visiting `root` before
/// `dep` therefore freezes into `root.imports["dep"]` a copy of `dep` whose own
/// imports are still the id-only stubs `build_import_stubs` made — so the graph
/// is only reliably one level deep from wherever you start, and which packages
/// get the shallow copy is decided by `HashMap` iteration order.
///
/// That is not academic: SA1019 reconstructs a deprecation message by reading
/// the sources of the package that *declares* the symbol, which for a promoted
/// field is neither the receiver's package nor necessarily one the file
/// imports. Walking there through `Package::imports` found a fileless stub and
/// the finding went missing — and renaming the module was enough to flip the
/// answer (COMPAT-HARDENING 2026-09-04 続き 159).
///
/// Iterative post-order DFS: dependency chains in a large corpus are deep
/// enough that recursion is not worth the risk. Neighbours are visited in
/// sorted order so the result does not depend on the map's seed. A cycle —
/// which `go list` should never produce — is broken at the back edge, leaving
/// that one package connected shallowly rather than looping.
fn connect_order(by_id: &HashMap<String, Arc<Package>>) -> Vec<String> {
    let mut roots: Vec<&str> = by_id.keys().map(String::as_str).collect();
    roots.sort_unstable();

    let mut order: Vec<String> = Vec::with_capacity(by_id.len());
    let mut done: HashSet<&str> = HashSet::default();
    let mut on_stack: HashSet<&str> = HashSet::default();
    // (id, deps-already-pushed) — the flag marks the second visit, at which
    // point every dependency is finished and the id can be emitted.
    let mut stack: Vec<(&str, bool)> = Vec::new();

    for root in roots {
        if done.contains(root) {
            continue;
        }
        stack.push((root, false));
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                on_stack.remove(id);
                if done.insert(id) {
                    order.push(id.to_string());
                }
                continue;
            }
            if done.contains(id) || !on_stack.insert(id) {
                // Already emitted, or a back edge: leave it to the frame that
                // is still open for this id.
                continue;
            }
            stack.push((id, true));
            let Some(pkg) = by_id.get(id) else { continue };
            let mut deps: Vec<&str> = pkg
                .imports
                .iter()
                .filter_map(|(path, stub)| {
                    resolved_import_id(by_id, &stub.id, path)
                        .and_then(|resolved| by_id.get_key_value(&resolved).map(|(k, _)| k.as_str()))
                })
                .collect();
            deps.sort_unstable();
            deps.dedup();
            // Reversed: the stack pops the last one first, so pushing in
            // reverse keeps the sorted visit order.
            for dep in deps.into_iter().rev() {
                if !done.contains(dep) {
                    stack.push((dep, false));
                }
            }
        }
    }
    order
}

fn connect_imports(by_id: &mut HashMap<String, Arc<Package>>) {
    for id in connect_order(by_id) {
        let Some(pkg) = by_id.get(&id).cloned() else {
            continue;
        };
        let import_paths: Vec<String> = pkg.imports.keys().cloned().collect();
        for path in import_paths {
            let stub_id = pkg.imports.get(&path).map(|p| p.id.clone());
            let Some(stub_id) = stub_id else { continue };
            let resolved = resolved_import_id(by_id, &stub_id, &path)
                .and_then(|id| by_id.get(&id).cloned());
            if let Some(resolved) = resolved {
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

    /// Three packages, `root` -> `dep` -> `inner`, none of them a stub with
    /// files. Whether `root.imports["dep"]` carries `dep`'s *own* connected
    /// imports decided whether SA1019 could find the package that declares a
    /// promoted deprecated field (COMPAT-HARDENING 2026-09-04 続き 159): a
    /// single pass over a `HashMap` connects `root` before `dep` half the time,
    /// and then `root`'s copy of `dep` is the snapshot taken before `dep`'s own
    /// edges were resolved. The two module paths in that entry differ only in
    /// name and landed on opposite sides of the coin.
    ///
    /// The names here are chosen so that at least one `HashMap` seed puts
    /// `root` first; the walk has to be dependency-first for the assertion to
    /// hold for every seed.
    #[test]
    fn refine_connects_imports_of_imports() {
        let inner = Arc::new(Package {
            id: "example.com/inner".into(),
            pkg_path: "example.com/inner".into(),
            name: "inner".into(),
            go_files: vec!["inner.go".into()],
            ..Package::default()
        });
        let stub = |id: &str| {
            Arc::new(Package {
                id: id.into(),
                ..Package::default()
            })
        };
        let dep = Arc::new(Package {
            id: "example.com/dep".into(),
            pkg_path: "example.com/dep".into(),
            name: "dep".into(),
            go_files: vec!["dep.go".into()],
            imports: HashMap::from([("example.com/inner".into(), stub("example.com/inner"))]),
            ..Package::default()
        });
        let root = Arc::new(Package {
            id: "example.com/root".into(),
            pkg_path: "example.com/root".into(),
            name: "root".into(),
            go_files: vec!["root.go".into()],
            imports: HashMap::from([("example.com/dep".into(), stub("example.com/dep"))]),
            ..Package::default()
        });
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            ..Config::default()
        };
        let roots = load_with_driver(
            &cfg,
            &[".".to_string()],
            &FakeDriver {
                response: DriverResponse {
                    roots: vec!["example.com/root".into()],
                    packages: vec![inner, dep, root],
                    ..DriverResponse::default()
                },
            },
        )
        .expect("load");
        let dep = roots[0].imports.get("example.com/dep").expect("dep");
        let inner = dep
            .imports
            .get("example.com/inner")
            .expect("inner reachable through dep");
        assert_eq!(
            inner.go_files.len(),
            1,
            "root's copy of dep still holds the id-only stub for inner",
        );
    }

    fn pkg_with_imports(id: &str, imports: &[&str]) -> Arc<Package> {
        Arc::new(Package {
            id: id.into(),
            pkg_path: id.into(),
            name: id.rsplit('/').next().unwrap_or(id).into(),
            go_files: vec![format!("{}.go", id.rsplit('/').next().unwrap_or(id)).into()],
            imports: imports
                .iter()
                .map(|dep| {
                    (
                        (*dep).to_string(),
                        Arc::new(Package {
                            id: (*dep).into(),
                            ..Package::default()
                        }),
                    )
                })
                .collect(),
            ..Package::default()
        })
    }

    /// `connect_order` decides the order, so it is what the depth and cycle
    /// cases assert against — the walk is iterative precisely so a long chain
    /// cannot blow the stack, and that is not observable through `refine`.
    #[test]
    fn connect_order_is_dependency_first() {
        let mut by_id: crate::hash::HashMap<String, Arc<Package>> = crate::hash::HashMap::default();
        for (id, deps) in [
            ("example.com/root", &["example.com/a", "example.com/b"][..]),
            ("example.com/a", &["example.com/leaf"][..]),
            ("example.com/b", &["example.com/leaf"][..]),
            ("example.com/leaf", &[][..]),
        ] {
            by_id.insert(id.to_string(), pkg_with_imports(id, deps));
        }
        let order = connect_order(&by_id);
        assert_eq!(order.len(), 4, "{order:?}");
        let at = |id: &str| order.iter().position(|x| x == id).expect(id);
        assert!(at("example.com/leaf") < at("example.com/a"), "{order:?}");
        assert!(at("example.com/leaf") < at("example.com/b"), "{order:?}");
        assert!(at("example.com/a") < at("example.com/root"), "{order:?}");
        assert!(at("example.com/b") < at("example.com/root"), "{order:?}");
    }

    #[test]
    fn connect_order_terminates_on_a_cycle_and_emits_every_package() {
        // `go list` should never produce one; if it ever does, the walk must
        // break the back edge rather than spin.
        let mut by_id: crate::hash::HashMap<String, Arc<Package>> = crate::hash::HashMap::default();
        by_id.insert(
            "example.com/a".into(),
            pkg_with_imports("example.com/a", &["example.com/b"]),
        );
        by_id.insert(
            "example.com/b".into(),
            pkg_with_imports("example.com/b", &["example.com/a"]),
        );
        let order = connect_order(&by_id);
        assert_eq!(order.len(), 2, "{order:?}");
    }

    #[test]
    fn connect_order_handles_a_chain_too_deep_to_recurse() {
        // A real corpus's dependency chains are nowhere near this long, but a
        // recursive walk is a latent stack overflow and this pins the choice.
        const N: usize = 20_000;
        let mut by_id: crate::hash::HashMap<String, Arc<Package>> = crate::hash::HashMap::default();
        for i in 0..N {
            let id = format!("example.com/p{i}");
            let deps: Vec<String> = if i + 1 < N {
                vec![format!("example.com/p{}", i + 1)]
            } else {
                vec![]
            };
            let refs: Vec<&str> = deps.iter().map(String::as_str).collect();
            by_id.insert(id.clone(), pkg_with_imports(&id, &refs));
        }
        let order = connect_order(&by_id);
        assert_eq!(order.len(), N);
        assert_eq!(order[0], format!("example.com/p{}", N - 1));
    }

    /// An import path the response has no package for (a dropped duplicate, a
    /// package `go list` reported only as a stub) must not stop the walk or
    /// lose the importer.
    #[test]
    fn connect_order_keeps_a_package_whose_import_is_missing() {
        let mut by_id: crate::hash::HashMap<String, Arc<Package>> = crate::hash::HashMap::default();
        by_id.insert(
            "example.com/root".into(),
            pkg_with_imports("example.com/root", &["example.com/gone"]),
        );
        let order = connect_order(&by_id);
        assert_eq!(order, vec!["example.com/root".to_string()]);
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

    /// C-7 speculation guesses the graph from a disk cache, and the guess is
    /// only useful if it reproduces what `refine` produces. The raw driver
    /// response does not: it still holds the prod-only `P` that `P [P.test]`
    /// replaces. Speculating from the raw response is what made the target
    /// list disagree — 293 guessed against 118 real on prometheus `./...` —
    /// so every speculation missed.
    #[test]
    fn peeked_graph_shape_drops_what_refine_drops() {
        let prod = Arc::new(Package {
            id: "example.com/a".into(),
            pkg_path: "example.com/a".into(),
            name: "a".into(),
            go_files: vec!["a.go".into()],
            ..Package::default()
        });
        let test_variant = Arc::new(Package {
            id: "example.com/a [example.com/a.test]".into(),
            pkg_path: "example.com/a".into(),
            name: "a".into(),
            go_files: vec!["a.go".into(), "a_test.go".into()],
            ..Package::default()
        });
        let response = DriverResponse {
            roots: vec![
                "example.com/a".into(),
                "example.com/a [example.com/a.test]".into(),
            ],
            packages: vec![prod, test_variant],
            ..DriverResponse::default()
        };

        let (roots, all) = peeked_graph_shape(response);
        assert_eq!(roots, vec!["example.com/a [example.com/a.test]".to_string()]);
        assert_eq!(all.len(), 1);

        // And it agrees with what the real load would produce from the same
        // response — the property that makes a guess usable at all.
        let cfg = Config {
            mode: LoadMode::LOAD_IMPORTS,
            ..Config::default()
        };
        let refined_roots = load_with_driver(
            &cfg,
            &[".".to_string()],
            &FakeDriver {
                response: DriverResponse {
                    roots: vec![
                        "example.com/a".into(),
                        "example.com/a [example.com/a.test]".into(),
                    ],
                    packages: vec![
                        Arc::new(Package {
                            id: "example.com/a".into(),
                            pkg_path: "example.com/a".into(),
                            name: "a".into(),
                            go_files: vec!["a.go".into()],
                            ..Package::default()
                        }),
                        Arc::new(Package {
                            id: "example.com/a [example.com/a.test]".into(),
                            pkg_path: "example.com/a".into(),
                            name: "a".into(),
                            go_files: vec!["a.go".into(), "a_test.go".into()],
                            ..Package::default()
                        }),
                    ],
                    ..DriverResponse::default()
                },
            },
        )
        .expect("load");
        let refined: Vec<String> = refined_roots.iter().map(|p| p.id.clone()).collect();
        assert_eq!(roots, refined);
    }
}
