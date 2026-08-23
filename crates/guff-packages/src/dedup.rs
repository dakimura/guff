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

/// Import-path → deps graph for hybrid seed ordering.
///
/// Seed waves / `dep_load_order` look up by **import path** (from `Package.deps`
/// and `imports`). Package ids are often `P [P.test]` after
/// [`filter_duplicate_packages`], so an id-keyed map misses those edges and
/// typechecks `P` before its imports — embedded field types become Invalid
/// (cli `api.HTTPError` / govet errorsas FPs under multi-root load).
///
/// Prefer plain `id == pkg_path` deps when both plain and test-variant exist —
/// except for the paths in [`paths_with_external_test_package`], where the seed
/// compiles the test variant's files and so needs the test variant's imports
/// (`testing`, testify, …) in the graph. A path whose seeded files and seeded
/// edges come from different variants gets its `_test.go` imports resolved by
/// the fallback importer or not at all.
pub fn import_path_dep_graph(by_id: &HashMap<String, Arc<Package>>) -> HashMap<String, Vec<String>> {
    let augmented = paths_with_external_test_package(by_id);
    let mut dep_graph = HashMap::default();
    for pkg in by_id.values() {
        // The values need normalizing as much as the keys do, and for longer:
        // an external test package's `deps` are **ids**, so a graph whose edges
        // point at `Q [P.test]` sends the seed off to type-check a package under
        // a name no `import` statement can ever spell. See `import_path_of_id`.
        let deps: Vec<String> = pkg
            .deps
            .iter()
            .map(|d| import_path_of_id(d).to_string())
            .collect();
        let key = pkg.pkg_path.clone();
        let authoritative = if augmented.contains(&key) {
            pkg.id == same_package_test_variant_id(&key)
        } else {
            pkg.id == key
        };
        if authoritative {
            dep_graph.insert(key, deps);
        } else {
            dep_graph.entry(key).or_insert(deps);
        }
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
