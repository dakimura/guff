//! Test-package deduplication (golangci-lint style).
//!
//! Port of `golangci-lint/pkg/lint/package.go` (`filterDuplicatePackages`).

use std::sync::Arc;

use crate::hash::{HashMap, HashSet};
use crate::package::Package;

/// True when `id` looks like `… [….test]` (any for-test / test-augmented id).
fn is_bracket_test_id(id: &str) -> bool {
    let Some(open) = id.find(" [") else {
        return false;
    };
    let Some(close) = id.rfind(".test]") else {
        return false;
    };
    close > open + 2
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
    let id = &pkg.id;
    let open = id.find(" [")?;
    let close = id.rfind(".test]")?;
    if close <= open + 2 {
        return None;
    }
    let plain = &id[..open];
    let under_test = &id[open + 2..close];
    if under_test == plain {
        Some(plain.to_string())
    } else {
        None
    }
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
}
