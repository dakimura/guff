//! Test-package deduplication (golangci-lint style).
//!
//! Port of `golangci-lint/pkg/lint/package.go` (`filterDuplicatePackages`).

use std::sync::Arc;

use crate::hash::{HashMap, HashSet};
use crate::package::Package;

/// A `go list -test` bracket id split into its two halves: `P [U.test]` →
/// `("P", "U")`. `None` when `id` is a plain import path.
fn split_bracket_test_id(id: &str) -> Option<(&str, &str)> {
    let open = id.find(" [")?;
    let close = id.rfind(".test]")?;
    if close <= open + 2 {
        return None;
    }
    Some((&id[..open], &id[open + 2..close]))
}

/// True when `id` looks like `… [….test]` (any for-test / test-augmented id).
fn is_bracket_test_id(id: &str) -> bool {
    split_bracket_test_id(id).is_some()
}

/// The import path a `go list -test` **id** names: `Q [P.test]` → `Q`.
///
/// The distinction matters because two different things are spelled the same
/// way. `Package.deps` of an *external* test package holds ids, not paths:
/// `pkg/cache_test [pkg/cache.test]` lists
/// `pkg/cluster [pkg/cache.test]` — the recompiled-for-the-test-binary copy of
/// `pkg/cluster`. Anything that treats that string as an import path registers a
/// second, differently-named copy of `pkg/cluster` in the seed, and then
/// `pkg/manager`, whose source says `import ".../pkg/cluster"`, cannot find one.
///
/// Collapsing the two is right *for the seed*, which compiles a dependency's
/// production files, so `Q [P.test]` and `Q` are the same bytes. The one
/// exception is [`paths_with_external_test_package`], where the seed compiles
/// `P [P.test]` and the two are *not* the same bytes — but it still registers
/// that under the plain path, because that is the name `import` statements
/// spell. Collapsing would not be right for analysis, where the test variant is
/// a genuinely different package.
pub fn import_path_of_id(id: &str) -> &str {
    match split_bracket_test_id(id) {
        Some((plain, _)) => plain,
        None => id,
    }
}

/// Same-package test variant only: `P [P.test]` (not for-test dep `Q [P.test]`).
///
/// Regex intent of golangci's `^(.*) \[(.*)\.test\]` plus the comment constraint
/// that the bracket path equals the plain package — i.e. `a.go`+`a_test.go` →
/// keep `a [a.test]`, drop plain `a`. Native list also emits `Q [P.test]` for
/// deps recompiled into P's test binary; those must **not** suppress plain `Q`
/// when `Q` is itself a pattern root (consul: services/resource +
/// internal/resource), or write.go never gets analyzed.
fn try_parse_same_package_test_variant(pkg: &Package) -> Option<String> {
    let (plain, under_test) = split_bracket_test_id(&pkg.id)?;
    (under_test == plain).then(|| plain.to_string())
}

/// The id of `P`'s same-package test variant, `P [P.test]`.
///
/// [`filter_duplicate_packages`] drops the plain `P` whenever that variant is in
/// the same load, so any *other* package's `Imports["P"]` then names an id that
/// is no longer there. The variant is P's own files plus P's `_test.go` files in
/// the same package, so for anything keyed on declarations — facts, above all —
/// it stands in for P exactly.
pub fn same_package_test_variant_id(id: &str) -> String {
    format!("{id} [{id}.test]")
}

/// The package under test when `id` names an **external** test package.
///
/// `go list -test` spells that one `P_test [P.test]`: the `package p_test`
/// files, compiled as a package of their own that imports P. The bracket alone
/// does not identify it — a for-test dep `Q [P.test]` wears the same brackets
/// and is some *other* package recompiled into P's test binary. What separates
/// them is that the external test package's own path is P's plus `_test`.
pub fn external_test_package_under_test(id: &str) -> Option<&str> {
    let (plain, under_test) = split_bracket_test_id(id)?;
    let stem = plain.strip_suffix("_test")?;
    (stem == under_test).then_some(under_test)
}

/// Import paths whose seeded copy must include their own in-package `_test.go`
/// files, because this load holds their external test package.
///
/// Inside P's test binary, `import ".../p"` names the test-augmented variant
/// `P [P.test]` — P's files **plus** its in-package `_test.go`. Widening P is
/// the entire job of `export_test.go`, so a production-only copy of P leaves
/// every identifier it adds `undefined:` for `package p_test`. That is not a
/// visible diff: the external test package goes ill-typed, and an ill-typed
/// package runs no analyzers at all, so its findings drop to zero in silence.
pub fn paths_with_external_test_package(
    by_id: &HashMap<String, Arc<Package>>,
) -> HashSet<String> {
    by_id
        .keys()
        .filter_map(|id| external_test_package_under_test(id))
        .map(str::to_string)
        .collect()
}

/// Removes non-test package entries when a same-package test-augmented variant
/// exists (`P [P.test]`).
///
/// When `go/packages` loads tests, it returns both `pkg` and `pkg [pkg.test]`.
/// Linters should analyze only the latter to avoid false unused-code warnings.
pub fn filter_duplicate_packages(pkgs: Vec<Arc<Package>>) -> Vec<Arc<Package>> {
    let mut packages_with_tests = HashSet::default();
    for pkg in &pkgs {
        if let Some(name) = try_parse_same_package_test_variant(pkg) {
            packages_with_tests.insert(name);
        }
    }

    pkgs
        .into_iter()
        .filter(|pkg| {
            // Keep every bracket test id (including for-test deps `Q [P.test]`).
            if is_bracket_test_id(&pkg.id) {
                return true;
            }
            !packages_with_tests.contains(&pkg.pkg_path)
        })
        .collect()
}

/// Removes implicit `testmain` packages (`package main` with `.test` suffix).
pub fn filter_test_main_packages(pkgs: Vec<Arc<Package>>) -> Vec<Arc<Package>> {
    pkgs.into_iter()
        .filter(|pkg| !(pkg.name == "main" && pkg.pkg_path.ends_with(".test")))
        .collect()
}

/// Resolve an import path to a loaded package.
///
/// Prefers the plain package id (`path == id`). After refine, production `P`
/// may be absent while `P [P.test]` remains — fall back to `pkg_path`.
pub fn package_for_import_path<'a>(
    by_id: &'a HashMap<String, Arc<Package>>,
    path: &str,
) -> Option<&'a Arc<Package>> {
    if let Some(p) = by_id.get(path) {
        return Some(p);
    }
    by_id.values().find(|p| p.pkg_path == path)
}

/// Copy each dropped plain `P`'s `deps` onto the `P [P.test]` that replaced it.
///
/// [`filter_duplicate_packages`] leaves the load with no production copy of any
/// package that has tests, and `Package.deps` is the only record of what that
/// copy imported. The seed still compiles production files for those paths (see
/// [`seed_variant_rank`]), so it still needs production edges; without this the
/// only edges left to order them by are the test variant's, which reach back
/// into the repo and can close a cycle the Go graph does not have.
///
/// Call it with the id set [`filter_duplicate_packages`] kept, *before*
/// narrowing the map to that set — afterwards the plain package is gone and its
/// `deps` with it.
pub fn carry_production_deps(by_id: &mut HashMap<String, Arc<Package>>, keep: &HashSet<String>) {
    let carried: Vec<(String, Vec<String>)> = by_id
        .values()
        .filter(|pkg| pkg.id == pkg.pkg_path && !keep.contains(&pkg.id))
        .filter_map(|pkg| {
            let variant = same_package_test_variant_id(&pkg.id);
            keep.contains(&variant).then(|| (variant, pkg.deps.clone()))
        })
        .collect();
    for (variant, deps) in carried {
        if let Some(pkg) = by_id.get_mut(&variant) {
            Arc::make_mut(pkg).production_deps = Some(deps);
        }
    }
}

/// The dependency edges of the files the seed compiles for `path`.
///
/// [`Package::production_deps`] is the plain package's own `deps`, recorded
/// before it was dropped; it is exactly right for a production-only build and
/// exactly wrong for an augmented one, where the seed *does* compile the tests.
fn seed_edges<'a>(pkg: &'a Package, path: &str, augmented: &HashSet<String>) -> &'a [String] {
    match pkg.production_deps.as_deref() {
        Some(prod) if !augmented.contains(path) => prod,
        _ => &pkg.deps,
    }
}

/// How well a loaded package stands in for the import path `path` when the
/// seed needs *one* variant of it. Lower is better; ties break on the id, so
/// nothing here depends on `HashMap` iteration order.
///
/// `go list -test` spells three different things with the same brackets, and
/// they do not hold the same files:
///
/// | id | files |
/// |---|---|
/// | `P` | production |
/// | `P [Q.test]` | production, recompiled into Q's test binary |
/// | `P [P.test]` | production **plus** P's in-package `_test.go` |
///
/// The seed compiles production files for every path but the ones in
/// [`paths_with_external_test_package`], where it compiles `P [P.test]`. Its
/// edges have to come from a variant holding *those* files: `P [P.test]`'s
/// `deps` carry the imports of P's tests (`testing`, testify, whatever the
/// tests reach for), and those are not edges of the package the seed builds.
///
/// That distinction is not cosmetic. [`filter_duplicate_packages`] drops plain
/// `P` whenever `P [P.test]` is in the load, so on a `./...` run over a repo
/// with tests almost every path is left choosing between bracketed variants —
/// and test imports reach *back* into the repo. On prometheus `./...`,
/// `tsdb [tsdb.test]` imports `util/teststorage` and
/// `util/teststorage [util/teststorage.test]` imports `tsdb`: a cycle that does
/// not exist in the Go graph, manufactured entirely out of test edges. A cycle
/// makes `dep_load_order` drop an edge to finish its walk, which costs the
/// topological order, which costs the wave assignment — 39 edges scheduling a
/// dependency no earlier than its consumer, and the two packages on the far end
/// of them (`promql/promqltest`, `tsdb`) type-checked against an `invalid` type
/// they should have seen whole.
fn seed_variant_rank(pkg: &Package, path: &str, augmented: &HashSet<String>) -> (u8, u8) {
    // A copy with nothing to compile is no copy at all: `go list` emits such
    // entries for packages resolved from export data, and for a test variant
    // whose files never made it into the response. Rank it behind every variant
    // that has files, in both branches.
    let empty = u8::from(pkg.compiled_go_files.is_empty());
    (empty, seed_variant_kind(&pkg.id, path, augmented))
}

fn seed_variant_kind(id: &str, path: &str, augmented: &HashSet<String>) -> u8 {
    let augmented_variant = id == same_package_test_variant_id(path);
    if augmented.contains(path) {
        // The seed compiles P's in-package tests here, so their imports *are*
        // edges of what it builds. Take them from the variant that has them.
        if augmented_variant {
            0
        } else if id == path {
            1
        } else {
            2
        }
    } else if id == path {
        0
    } else if !augmented_variant {
        // `P [Q.test]` — production files, so production edges.
        1
    } else {
        // The only copy left is the test-augmented one. Its files are still
        // filtered down to production, and its edges come from
        // [`Package::production_deps`] — the plain package's own `deps`, kept
        // by [`carry_production_deps`] before it was dropped.
        2
    }
}

/// The single variant of `path` that the seed's files *and* edges both come
/// from. `None` when the load holds no package for `path` at all.
///
/// Ties inside a rank break on the id so the choice does not ride on
/// `HashMap` iteration order — the previous `values().find()` let two runs of
/// the same load disagree about which `_test.go`-carrying copy of a package the
/// graph described.
pub fn seed_variant_for<'a>(
    by_id: &'a HashMap<String, Arc<Package>>,
    path: &str,
    augmented: &HashSet<String>,
) -> Option<&'a Arc<Package>> {
    if let Some(pkg) = by_id.get(path) {
        // Fast path: for anything but an augmented path the plain id is the
        // best rank there is, so the scan below cannot beat it.
        if !augmented.contains(path) && !pkg.compiled_go_files.is_empty() {
            return Some(pkg);
        }
    }
    let mut best: Option<((u8, u8), &Arc<Package>)> = None;
    for pkg in by_id.values() {
        if pkg.pkg_path != path {
            continue;
        }
        let rank = seed_variant_rank(pkg, path, augmented);
        let better = match best {
            None => true,
            Some((r, cur)) => (rank, &pkg.id) < (r, &cur.id),
        };
        if better {
            best = Some((rank, pkg));
        }
    }
    best.map(|(_, pkg)| pkg)
}

/// Import-path → deps graph for hybrid seed ordering.
///
/// Seed waves / `dep_load_order` look up by **import path** (from `Package.deps`
/// and `imports`). Package ids are often `P [P.test]` after
/// [`filter_duplicate_packages`], so an id-keyed map misses those edges and
/// typechecks `P` before its imports — embedded field types become Invalid
/// (cli `api.HTTPError` / govet errorsas FPs under multi-root load).
///
/// One entry per import path, taken from the variant [`seed_variant_rank`]
/// picks — the same one `seed_package_for` compiles, so a path's seeded files
/// and its seeded edges can never come from different variants.
pub fn import_path_dep_graph(by_id: &HashMap<String, Arc<Package>>) -> HashMap<String, Vec<String>> {
    let augmented = paths_with_external_test_package(by_id);
    let mut chosen: HashMap<&str, ((u8, u8), &str)> = HashMap::default();
    for pkg in by_id.values() {
        let path = pkg.pkg_path.as_str();
        let rank = seed_variant_rank(pkg, path, &augmented);
        let id = pkg.id.as_str();
        match chosen.get(path) {
            Some(&(r, cur)) if (r, cur) <= (rank, id) => {}
            _ => {
                chosen.insert(path, (rank, id));
            }
        }
    }

    let mut dep_graph = HashMap::default();
    for (path, (_, id)) in chosen {
        // The values need normalizing as much as the keys do, and for longer:
        // an external test package's `deps` are **ids**, so a graph whose edges
        // point at `Q [P.test]` sends the seed off to type-check a package under
        // a name no `import` statement can ever spell. See `import_path_of_id`.
        let deps: Vec<String> = seed_edges(&by_id[id], path, &augmented)
            .iter()
            .map(|d| import_path_of_id(d).to_string())
            .collect();
        dep_graph.insert(path.to_string(), deps);
    }
    dep_graph
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, pkg_path: &str) -> Arc<Package> {
        Arc::new(Package {
            id: id.to_string(),
            pkg_path: pkg_path.to_string(),
            ..Package::default()
        })
    }

    #[test]
    fn dedup_keeps_test_variant_only() {
        let pkgs = vec![
            pkg("example.com/foo", "example.com/foo"),
            pkg("example.com/foo [example.com/foo.test]", "example.com/foo"),
        ];
        let out = filter_duplicate_packages(pkgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "example.com/foo [example.com/foo.test]");
    }

    #[test]
    fn dedup_keeps_non_test_when_no_variant() {
        let pkgs = vec![pkg("example.com/bar", "example.com/bar")];
        let out = filter_duplicate_packages(pkgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pkg_path, "example.com/bar");
    }

    #[test]
    fn dedup_keeps_plain_when_only_fortest_dep_variant() {
        // Q imports P; P's tests force Q [P.test]. Plain Q is still a pattern
        // root and must be analyzed (consul services/resource + internal/resource).
        let pkgs = vec![
            pkg("example.com/q", "example.com/q"),
            pkg("example.com/q [example.com/p.test]", "example.com/q"),
            pkg("example.com/p [example.com/p.test]", "example.com/p"),
        ];
        let out = filter_duplicate_packages(pkgs);
        let ids: Vec<_> = out.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"example.com/q"), "{ids:?}");
        assert!(ids.contains(&"example.com/q [example.com/p.test]"), "{ids:?}");
        assert!(ids.contains(&"example.com/p [example.com/p.test]"), "{ids:?}");
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn try_parse_same_package_test_variant_matches_golangci_intent() {
        let p = pkg("fmt [fmt.test]", "fmt");
        assert_eq!(
            try_parse_same_package_test_variant(&p),
            Some("fmt".to_string())
        );
        let fortest = pkg("other [fmt.test]", "other");
        assert_eq!(try_parse_same_package_test_variant(&fortest), None);
        let plain = pkg("fmt", "fmt");
        assert_eq!(try_parse_same_package_test_variant(&plain), None);
    }

    #[test]
    fn filter_test_main_removes_implicit_main() {
        let pkgs = vec![
            Arc::new(Package {
                id: "example.com/foo.test".into(),
                pkg_path: "example.com/foo.test".into(),
                name: "main".into(),
                ..Package::default()
            }),
            pkg("example.com/foo", "example.com/foo"),
        ];
        let out = filter_test_main_packages(pkgs);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pkg_path, "example.com/foo");
    }

    #[test]
    fn import_path_of_id_strips_the_for_test_bracket() {
        assert_eq!(import_path_of_id("example.com/q"), "example.com/q");
        assert_eq!(
            import_path_of_id("example.com/q [example.com/p.test]"),
            "example.com/q"
        );
        assert_eq!(
            import_path_of_id("example.com/p [example.com/p.test]"),
            "example.com/p"
        );
        // A space with no `.test]` after it is not a go list id at all.
        assert_eq!(import_path_of_id("example.com/q [x]"), "example.com/q [x]");
    }

    #[test]
    fn dep_graph_edges_are_import_paths_not_ids() {
        // An *external* test package's `deps` hold ids: `pkg/cache_test
        // [pkg/cache.test]` depends on `pkg/cluster [pkg/cache.test]`, the copy
        // of pkg/cluster recompiled into the test binary. Left as-is, that id
        // reaches the dependency seed as if it were an import path, so
        // pkg/cluster is type-checked and registered under a name no `import`
        // statement can spell — and pkg/manager, which really does import
        // ".../pkg/cluster", then fails with `undefined: cluster` and every
        // interface embedding cluster.Cluster loses its methods.
        //
        // Measured on controller-runtime before this normalization: 48 reports
        // of `manager.Manager has no field or method Get*` under `./pkg/...`,
        // 0 under `./pkg/metrics/filters/...` — the same bytes and the same
        // config, differing only in which packages were asked for.
        let mut by_id = HashMap::default();
        let ext_test = Arc::new(Package {
            id: "example.com/cache_test [example.com/cache.test]".into(),
            pkg_path: "example.com/cache_test".into(),
            deps: vec![
                "example.com/cluster [example.com/cache.test]".into(),
                "example.com/plain".into(),
            ],
            ..Package::default()
        });
        by_id.insert(ext_test.id.clone(), ext_test);

        let graph = import_path_dep_graph(&by_id);
        assert_eq!(
            graph.get("example.com/cache_test").map(Vec::as_slice),
            Some(["example.com/cluster".to_string(), "example.com/plain".to_string()].as_slice()),
        );
    }

    #[test]
    fn package_for_import_path_finds_for_test_survivor() {
        let mut by_id = HashMap::default();
        let survivor = pkg(
            "example.com/foo [example.com/foo.test]",
            "example.com/foo",
        );
        by_id.insert(survivor.id.clone(), Arc::clone(&survivor));
        let got = package_for_import_path(&by_id, "example.com/foo").unwrap();
        assert_eq!(got.id, survivor.id);
    }

    #[test]
    fn import_path_dep_graph_resolves_test_variant_survivor() {
        let lib = Arc::new(Package {
            id: "example.com/lib [example.com/lib.test]".into(),
            pkg_path: "example.com/lib".into(),
            deps: vec!["example.com/ext".into()],
            ..Package::default()
        });
        let mut by_id = HashMap::default();
        by_id.insert(lib.id.clone(), Arc::clone(&lib));

        let graph = import_path_dep_graph(&by_id);
        assert_eq!(
            graph.get("example.com/lib"),
            Some(&vec!["example.com/ext".to_string()]),
            "seed must see lib→ext after plain lib was dropped for lib [lib.test]"
        );
        assert!(graph.get(&lib.id).is_none());
    }

    fn with_deps(id: &str, pkg_path: &str, deps: &[&str]) -> Arc<Package> {
        Arc::new(Package {
            id: id.to_string(),
            pkg_path: pkg_path.to_string(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
            ..Package::default()
        })
    }

    fn by_id_of(pkgs: Vec<Arc<Package>>) -> HashMap<String, Arc<Package>> {
        let mut by_id = HashMap::default();
        for p in pkgs {
            by_id.insert(p.id.clone(), p);
        }
        by_id
    }

    /// prometheus, reduced: `tsdb`'s in-package tests import `util/teststorage`
    /// and `util/teststorage`'s in-package tests import `tsdb`. Neither
    /// *production* package imports the other, so Go is happy — each test
    /// binary sees the other side production-only. `filter_duplicate_packages`
    /// then drops both plain packages, and if the graph takes what is left, the
    /// two test variants point at each other and the seed has a cycle that does
    /// not exist in any Go build.
    fn mutually_test_importing_pair() -> Vec<Arc<Package>> {
        vec![
            with_deps(
                "example.com/m/tsdb [example.com/m/tsdb.test]",
                "example.com/m/tsdb",
                &["example.com/m/util/teststorage"],
            ),
            // The copy of tsdb recompiled into teststorage's test binary:
            // production files, production edges.
            with_deps(
                "example.com/m/tsdb [example.com/m/util/teststorage.test]",
                "example.com/m/tsdb",
                &["example.com/m/tsdb/index"],
            ),
            with_deps(
                "example.com/m/util/teststorage [example.com/m/util/teststorage.test]",
                "example.com/m/util/teststorage",
                &["example.com/m/tsdb"],
            ),
            with_deps(
                "example.com/m/util/teststorage [example.com/m/tsdb.test]",
                "example.com/m/util/teststorage",
                &[],
            ),
        ]
    }

    /// The seed compiles production files for a path with no external test
    /// package, so it has to be ordered by production edges. `P [Q.test]` is
    /// that same production build, recompiled for someone else's test binary.
    #[test]
    fn a_for_test_dep_copy_outranks_the_test_augmented_one() {
        let graph = import_path_dep_graph(&by_id_of(mutually_test_importing_pair()));
        assert_eq!(
            graph.get("example.com/m/tsdb"),
            Some(&vec!["example.com/m/tsdb/index".to_string()]),
            "tsdb's edges must come from the production copy, not its test variant",
        );
        assert_eq!(
            graph.get("example.com/m/util/teststorage"),
            Some(&vec![]),
            "and so must teststorage's — otherwise the two point at each other",
        );
    }

    /// The invariant the whole selection exists for: whatever variant supplies
    /// a path's *files* must be the one that supplies its *edges*. They were
    /// picked by two different functions — `package_for_import_path`, which
    /// takes whatever `HashMap` iteration hands it first, and a rank that
    /// preferred a plain package no longer in the load — so a path could be
    /// compiled from one variant and ordered by another's imports.
    #[test]
    fn files_and_edges_are_read_off_the_same_variant() {
        let by_id = by_id_of(mutually_test_importing_pair());
        let augmented = paths_with_external_test_package(&by_id);
        for path in ["example.com/m/tsdb", "example.com/m/util/teststorage"] {
            let files_from = seed_variant_for(&by_id, path, &augmented).expect("a variant");
            let edges = import_path_dep_graph(&by_id).remove(path).expect("edges");
            let want: Vec<String> = seed_edges(files_from, path, &augmented)
                .iter()
                .map(|d| import_path_of_id(d).to_string())
                .collect();
            assert_eq!(edges, want, "{path}: edges are not {}'s", files_from.id);
        }
    }

    /// When the only copy left is `P [P.test]` — nobody else's test binary
    /// recompiled P — the production edges come from the plain package
    /// `filter_duplicate_packages` dropped, carried over before it went.
    #[test]
    fn carried_production_deps_stand_in_for_the_dropped_plain_package() {
        let variant = "example.com/m/local [example.com/m/local.test]";
        let mut by_id = by_id_of(vec![
            with_deps("example.com/m/local", "example.com/m/local", &["example.com/m/fs"]),
            with_deps(variant, "example.com/m/local", &["example.com/m/fs", "example.com/m/fstest"]),
        ]);
        let keep: HashSet<String> = filter_duplicate_packages(by_id.values().cloned().collect())
            .into_iter()
            .map(|p| p.id.clone())
            .collect();
        assert_eq!(keep, HashSet::from_iter([variant.to_string()]));

        carry_production_deps(&mut by_id, &keep);
        by_id.retain(|id, _| keep.contains(id));

        let graph = import_path_dep_graph(&by_id);
        assert_eq!(
            graph.get("example.com/m/local"),
            Some(&vec!["example.com/m/fs".to_string()]),
            "fstest is a test-only import and not an edge of what the seed builds",
        );
    }

    /// …but not for a path whose *external* test package is in the load: there
    /// the seed really does compile the in-package `_test.go`, so their imports
    /// really are edges. Regression guard for the `export_test.go` fix.
    #[test]
    fn an_augmented_path_keeps_its_test_variant_edges_after_a_carry() {
        let variant = "example.com/m/promql [example.com/m/promql.test]";
        let mut by_id = by_id_of(vec![
            with_deps("example.com/m/promql", "example.com/m/promql", &["example.com/m/parser"]),
            with_deps(variant, "example.com/m/promql", &["example.com/m/parser", "example.com/m/testutil"]),
            with_deps(
                "example.com/m/promql_test [example.com/m/promql.test]",
                "example.com/m/promql_test",
                &[variant],
            ),
        ]);
        let keep: HashSet<String> = by_id
            .keys()
            .filter(|id| *id != "example.com/m/promql")
            .cloned()
            .collect();
        carry_production_deps(&mut by_id, &keep);
        by_id.retain(|id, _| keep.contains(id));

        let graph = import_path_dep_graph(&by_id);
        assert!(
            graph["example.com/m/promql"].contains(&"example.com/m/testutil".to_string()),
            "augmented: the seed compiles the tests, so their imports are edges: {:?}",
            graph["example.com/m/promql"],
        );
    }

    /// `carry_production_deps` must not fire when the plain package survived —
    /// its own `deps` already are the production ones, and a stale copy on some
    /// other variant would outlive an edit.
    #[test]
    fn nothing_is_carried_when_the_plain_package_survives() {
        let mut by_id = by_id_of(vec![
            with_deps("example.com/m/q", "example.com/m/q", &["example.com/m/a"]),
            with_deps(
                "example.com/m/q [example.com/m/p.test]",
                "example.com/m/q",
                &["example.com/m/a"],
            ),
        ]);
        let keep: HashSet<String> = by_id.keys().cloned().collect();
        carry_production_deps(&mut by_id, &keep);
        assert!(by_id.values().all(|p| p.production_deps.is_none()));
    }

    #[test]
    fn import_path_dep_graph_prefers_plain_over_fortest_variant() {
        let plain = Arc::new(Package {
            id: "example.com/q".into(),
            pkg_path: "example.com/q".into(),
            deps: vec!["example.com/a".into()],
            ..Package::default()
        });
        let fortest = Arc::new(Package {
            id: "example.com/q [example.com/p.test]".into(),
            pkg_path: "example.com/q".into(),
            deps: vec!["example.com/b".into()],
            ..Package::default()
        });
        let mut by_id = HashMap::default();
        by_id.insert(plain.id.clone(), Arc::clone(&plain));
        by_id.insert(fortest.id.clone(), Arc::clone(&fortest));

        let graph = import_path_dep_graph(&by_id);
        assert_eq!(
            graph.get("example.com/q"),
            Some(&vec!["example.com/a".to_string()]),
        );
    }
}
