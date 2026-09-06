//! Two packages whose in-package tests import each other must both type-check.
//!
//! `session`'s `_test.go` imports `vault` and `vault`'s `_test.go` imports
//! `session`. That is legal Go — each test binary links the *production* copy of
//! the other package, so no production import cycle exists — but guff's seed
//! keeps one node per import path, so the two test edges meet as a cycle.
//! `dep_load_order` declines the edge that closes it.
//!
//! The bug this guards: the declined edge stayed in `dep_graph`, and the wave
//! passes read it back. A declined edge always runs from a package late in the
//! load order to one earlier in it, so `height` raised the earlier package
//! *after* its own height was read as final — and it landed in the same wave as
//! a package it imports, type-checked in parallel with it, seeing nothing of it.
//!
//! On boundary that cost 11 ill-typed packages, all with the same symptom:
//! `*vault.CredentialStore has no field or method GetPublicId`, because the
//! seeded `vault` was built with its own `store` dependency out of scope and the
//! embedded field went invalid. `tool` below is the package that gets stranded;
//! in the Go original it is `testing`.
//!
//! Hand-built `Package` values against on-disk fixtures, so no `go` toolchain is
//! involved — the source seed is the thing under test.

use std::path::PathBuf;
use std::sync::Arc;

use guff_packages::{typecheck_roots, LoadMode, Package, TypecheckEnv};

const STORE: &str = "example.com/mutual/store";
const TOOL: &str = "example.com/mutual/tool";
const VAULT: &str = "example.com/mutual/vault";
const VAULT_VARIANT: &str = "example.com/mutual/vault [example.com/mutual/vault.test]";
const VAULT_EXTERNAL: &str =
    "example.com/mutual/vault_test [example.com/mutual/vault.test]";
const SESSION: &str = "example.com/mutual/session";
const SESSION_VARIANT: &str = "example.com/mutual/session [example.com/mutual/session.test]";
const SESSION_EXTERNAL: &str =
    "example.com/mutual/session_test [example.com/mutual/session.test]";

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/mutualtest")
        .join(rel)
}

fn pkg(id: &str, pkg_path: &str, files: &[&str], deps: &[&str]) -> Arc<Package> {
    build(id, pkg_path, files, deps, None)
}

/// A package whose plain copy dedup dropped. `production_deps` is what
/// `carry_production_deps` saves off that copy before it goes, and it is the
/// only way anything downstream can tell a test edge from a production one — so
/// a fixture that leaves it `None` marks *no* edge as test-only, declines
/// nothing, and quietly exercises none of this.
fn augmented(
    id: &str,
    pkg_path: &str,
    files: &[&str],
    deps: &[&str],
    production_deps: &[&str],
) -> Arc<Package> {
    build(
        id,
        pkg_path,
        files,
        deps,
        Some(production_deps.iter().map(|d| d.to_string()).collect()),
    )
}

fn build(
    id: &str,
    pkg_path: &str,
    files: &[&str],
    deps: &[&str],
    production_deps: Option<Vec<String>>,
) -> Arc<Package> {
    let files: Vec<PathBuf> = files.iter().map(|f| fixture(f)).collect();
    Arc::new(Package {
        id: id.to_string(),
        pkg_path: pkg_path.to_string(),
        dir: files[0].parent().expect("fixture dir").to_path_buf(),
        compiled_go_files: files,
        deps: deps.iter().map(|d| d.to_string()).collect(),
        production_deps,
        ..Package::default()
    })
}

/// What `go list -test ./...` produces for the fixture, minus the plain `vault`
/// and `session` that `filter_duplicate_packages` drops once their test
/// variants are in the load — the shape the lint path actually type-checks.
fn packages() -> Vec<Arc<Package>> {
    vec![
        pkg(STORE, STORE, &["store/store.go"], &[]),
        pkg(TOOL, TOOL, &["tool/tool.go"], &[]),
        // vault's production files import only `store`; `session` arrives with
        // vault_internal_test.go, so it is a test-only edge.
        augmented(
            VAULT_VARIANT,
            VAULT,
            &["vault/vault.go", "vault/vault_internal_test.go"],
            &[STORE, SESSION],
            &[STORE],
        ),
        pkg(
            VAULT_EXTERNAL,
            "example.com/mutual/vault_test",
            &["vault/vault_external_test.go"],
            &[VAULT_VARIANT],
        ),
        // session's production files import nothing: both `tool` and `vault`
        // are test-only edges, and `vault` is the one that closes the cycle.
        augmented(
            SESSION_VARIANT,
            SESSION,
            &["session/session.go", "session/session_internal_test.go"],
            &[TOOL, VAULT],
            &[],
        ),
        pkg(
            SESSION_EXTERNAL,
            "example.com/mutual/session_test",
            &["session/session_external_test.go"],
            &[SESSION_VARIANT],
        ),
    ]
}

/// Every package in the load is a root, as on a `./...` run — the shape that
/// matters, because it is what makes `session` both a root *and* a dependency
/// of `vault`. Checking one root at a time hides this bug entirely: the seed
/// then holds only that root's closure, and the package on the declining side
/// never gets built as a dependency at all.
fn check_all() -> Vec<Arc<Package>> {
    let env = TypecheckEnv {
        from_source: true,
        parallel: false,
        ..TypecheckEnv::default()
    };
    let all = packages();
    let roots: Vec<String> = all.iter().map(|p| p.id.clone()).collect();
    typecheck_roots(&all, &roots, LoadMode::LOAD_ALL_SYNTAX, &env)
}

fn check(target: &str) -> Arc<Package> {
    check_all()
        .into_iter()
        .find(|p| p.id == target)
        .unwrap_or_else(|| panic!("{target} not in the checked set"))
}

fn errors(pkg: &Package) -> String {
    pkg.errors
        .iter()
        .map(|e| e.msg.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

/// boundary's exact signature, in four packages.
///
/// `session.Helper` embeds `tool.T`, so a `session` built with `tool` out of
/// scope loses the promoted `Use` rather than the import — which is why the real
/// thing read as `*vault.CredentialStore has no field or method GetPublicId` and
/// was mistaken for a missing package variant rather than a scheduling bug.
///
/// Without the fix this reports exactly that:
/// `session.Helper.Use undefined (type session.Helper has no field or method Use)`.
#[test]
fn a_promoted_method_survives_the_declined_edge() {
    let checked = check(VAULT_VARIANT);
    assert!(
        !errors(&checked).contains("Use"),
        "vault lost a method promoted through session's stranded dependency: {}",
        errors(&checked)
    );
    assert!(!checked.ill_typed, "vault ill-typed: {}", errors(&checked));
}

/// Both sides of the cycle and the packages they can strand. Nothing here is
/// ill-typed: the Go this fixture is built from compiles and vets clean, so any
/// error is guff's.
#[test]
fn every_package_in_a_mutual_test_edge_load_is_well_typed() {
    for target in [
        STORE,
        TOOL,
        VAULT_VARIANT,
        VAULT_EXTERNAL,
        SESSION_VARIANT,
        SESSION_EXTERNAL,
    ] {
        let checked = check(target);
        assert!(!checked.ill_typed, "{target} ill-typed: {}", errors(&checked));
    }
}
