//! `package p_test` must see `export_test.go`.
//!
//! Inside P's test binary the import path `.../p` names the test-augmented
//! variant `P [P.test]` — P's own files **plus** its in-package `_test.go`.
//! `export_test.go` exists to widen exactly that variant, so a shared seed
//! built from production files alone leaves every identifier it adds
//! `undefined:` and the external test package goes ill-typed. Nothing in a
//! compat diff says so: an ill-typed package runs no analyzers at all, so its
//! findings drop to zero in silence, and the tool that still reports them looks
//! like the one with the false positives.
//!
//! Hand-built `Package` values against on-disk fixtures, so no `go` toolchain
//! and no export data are involved — the source seed is the thing under test.

use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{
    external_test_package_under_test, import_path_dep_graph, typecheck_roots, FxHashMap, LoadMode,
    Package, TypecheckEnv,
};

const P: &str = "example.com/xt/p";
const P_VARIANT: &str = "example.com/xt/p [example.com/xt/p.test]";
const P_EXTERNAL: &str = "example.com/xt/p_test [example.com/xt/p.test]";
const Q: &str = "example.com/xt/q";
const R: &str = "example.com/xt/r";
const R_VARIANT: &str = "example.com/xt/r [example.com/xt/r.test]";
const USER: &str = "example.com/xt/user";

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/xtest")
        .join(rel)
}

fn pkg(id: &str, pkg_path: &str, files: &[&str], deps: &[&str]) -> Arc<Package> {
    let files: Vec<PathBuf> = files.iter().map(|f| fixture(f)).collect();
    Arc::new(Package {
        id: id.to_string(),
        pkg_path: pkg_path.to_string(),
        dir: files[0].parent().expect("fixture dir").to_path_buf(),
        compiled_go_files: files,
        deps: deps.iter().map(|d| d.to_string()).collect(),
        ..Package::default()
    })
}

/// The package set `go list -test` produces for the fixture, minus the plain
/// `P` / `R` that `filter_duplicate_packages` drops — the shape the lint path
/// actually type-checks (`guff-lint` loads metadata, dedups, then calls
/// `typecheck_roots` on the survivors).
fn deduped_packages() -> Vec<Arc<Package>> {
    vec![
        pkg(P_VARIANT, P, &["p/p.go", "p/export_test.go"], &[]),
        // `deps` of an external test package hold **ids**, not import paths.
        pkg(P_EXTERNAL, "example.com/xt/p_test", &["p/ext_test.go"], &[P_VARIANT, Q]),
        pkg(Q, Q, &["q/q.go"], &[P]),
        pkg(R_VARIANT, R, &["r/r.go", "r/hidden_test.go"], &[]),
        pkg(USER, USER, &["user/user.go"], &[R]),
    ]
}

/// The same load before dedup: plain `P` and `R` are still present and win the
/// exact-id lookup, which is the shape `load()` type-checks under NEED_TYPES.
fn full_packages() -> Vec<Arc<Package>> {
    let mut all = deduped_packages();
    all.push(pkg(P, P, &["p/p.go"], &[]));
    all.push(pkg(R, R, &["r/r.go"], &[]));
    all
}

fn check(all: &[Arc<Package>], target: &str) -> Arc<Package> {
    let env = TypecheckEnv {
        from_source: true,
        parallel: false,
        ..TypecheckEnv::default()
    };
    typecheck_roots(all, &[target.to_string()], LoadMode::LOAD_ALL_SYNTAX, &env)
        .into_iter()
        .next()
        .expect("one target package back")
}

fn errors(pkg: &Package) -> String {
    pkg.errors
        .iter()
        .map(|e| e.msg.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

/// The fix. Both shapes of the load must resolve `p.Reveal`, and — because
/// `ext_test.go` also hands its `p.T` to `q.Describe` — must resolve it to the
/// *same* `p` that the rest of the seed imports. Splitting `p` into a
/// production copy for `q` and a test-augmented copy for `p_test` would clear
/// the `undefined:` and fail here instead.
#[test]
fn external_test_package_sees_export_test_symbols() {
    for (label, all) in [("deduped", deduped_packages()), ("full", full_packages())] {
        let checked = check(&all, P_EXTERNAL);
        assert!(
            !checked.ill_typed,
            "{label}: external test package ill-typed: {}",
            errors(&checked)
        );
    }
}

/// The gate on that widening. `r` has in-package `_test.go` but **no** external
/// test package, so its seeded copy stays production-only and `r.Hidden` is
/// undefined in `user` — as in Go. Without this, the cheapest fix (seed every
/// package from whatever survived dedup) would pass the test above while
/// putting `_test.go` through the whole seed, which roughly doubles type-arena
/// RSS on prometheus `./...`.
#[test]
fn a_package_without_an_external_test_package_is_seeded_production_only() {
    for (label, all) in [("deduped", deduped_packages()), ("full", full_packages())] {
        let checked = check(&all, USER);
        assert!(
            checked.ill_typed,
            "{label}: r.Hidden leaked out of r's test variant"
        );
        assert!(
            errors(&checked).contains("Hidden"),
            "{label}: expected an undefined-Hidden error, got: {}",
            errors(&checked)
        );
    }
}

/// Ordinary importers of `p` are unaffected: `q` type-checks either way.
#[test]
fn a_production_importer_of_an_augmented_package_still_checks() {
    for (label, all) in [("deduped", deduped_packages()), ("full", full_packages())] {
        let checked = check(&all, Q);
        assert!(
            !checked.ill_typed,
            "{label}: q ill-typed: {}",
            errors(&checked)
        );
    }
}

/// The bracket alone does not name an external test package. `Q [P.test]` is
/// some other package recompiled into P's test binary, and `P [P.test]` is P's
/// own test variant; treating either as "P has an external test package" would
/// widen the seed for packages that never asked for it.
#[test]
fn only_p_test_brackets_p_names_an_external_test_package() {
    assert_eq!(external_test_package_under_test(P_EXTERNAL), Some(P));
    assert_eq!(external_test_package_under_test(P_VARIANT), None);
    assert_eq!(
        external_test_package_under_test("example.com/xt/q [example.com/xt/p.test]"),
        None
    );
    assert_eq!(external_test_package_under_test(P), None);
    // A package whose own name ends in `_test`, in its own test variant.
    assert_eq!(
        external_test_package_under_test("example.com/xt/p_test [example.com/xt/p_test.test]"),
        None
    );
}

/// The seed compiles the test variant's files for an augmented path, so the
/// graph must carry the test variant's edges — otherwise a `_test.go`-only
/// import reaches the seed check with nothing to resolve it. `p`'s edges must
/// come from `P [P.test]`; `r`'s, which is not augmented, from plain `r`.
#[test]
fn augmented_paths_take_their_edges_from_the_test_variant() {
    let mut by_id: FxHashMap<String, Arc<Package>> = FxHashMap::default();
    for p in full_packages() {
        by_id.insert(p.id.clone(), p);
    }
    // Give the two variants edges the plain packages do not have.
    for (id, extra) in [(P_VARIANT, "example.com/only/in/p/tests"), (R_VARIANT, "example.com/only/in/r/tests")] {
        let mut v = (*by_id[id]).clone();
        v.deps.push(extra.to_string());
        by_id.insert(id.to_string(), Arc::new(v));
    }

    let graph = import_path_dep_graph(&by_id);
    assert!(
        graph[P].contains(&"example.com/only/in/p/tests".to_string()),
        "p is augmented, so its edges are the test variant's: {:?}",
        graph[P]
    );
    assert!(
        !graph[R].contains(&"example.com/only/in/r/tests".to_string()),
        "r is not augmented, so plain r still wins: {:?}",
        graph[R]
    );
}
