//! Chunk-55 tests: `version.rs` — Go language-version handling
//! (`GoVersion` / `as_go_version` + `Checker::allow_version` /
//! `verify_versionf`), port of Go's `version.go`.

use guff_types::{
    as_go_version, go1_17, go1_18, go1_21, go_current, Checker, Config, GoVersion,
};
use guff_types_errors::Code;

#[test]
fn as_go_version_strips_release_number() {
    assert_eq!(as_go_version("go1.20.1").as_str(), "go1.20");
    assert_eq!(as_go_version("go1.21").as_str(), "go1.21");
}

#[test]
fn invalid_version_is_empty() {
    assert!(!as_go_version("not-a-version").is_valid());
    assert!(!as_go_version("").is_valid());
    assert_eq!(as_go_version("nonsense"), GoVersion::default());
}

#[test]
fn cmp_orders_versions() {
    assert!(go1_17().cmp(&go1_18()) < 0);
    assert!(go1_18().cmp(&go1_18()) == 0);
    assert!(go1_21().cmp(&go1_18()) > 0);
}

#[test]
fn go_current_is_valid_and_recent() {
    let cur = go_current();
    assert!(cur.is_valid());
    // The toolchain we target is well past generics.
    assert!(cur.cmp(&go1_18()) >= 0);
}

#[test]
fn allow_version_disabled_when_version_unset() {
    // Default Config has no go_version → version checks are disabled, so every
    // feature is allowed.
    let mut check = Checker::new(Config::default());
    check.env.version = String::new();
    assert!(check.allow_version(&go1_21()));
    assert!(check.allow_version(&go1_18()));
}

#[test]
fn allow_version_gates_on_effective_version() {
    let mut check = Checker::new(Config::default());
    check.env.version = "go1.18".to_string();
    assert!(
        check.allow_version(&go1_17()),
        "go1.18 allows a go1.17 feature"
    );
    assert!(
        check.allow_version(&go1_18()),
        "go1.18 allows a go1.18 feature"
    );
    assert!(
        !check.allow_version(&go1_21()),
        "go1.18 forbids a go1.21 feature"
    );
}

#[test]
fn verify_versionf_reports_error_when_too_old() {
    let mut check = Checker::new(Config::default());
    check.env.version = "go1.18".to_string();

    // Allowed feature: returns true, no error.
    assert!(check.verify_versionf(0, &go1_17(), "slice-to-array-pointer conversion"));
    assert!(check.errors.is_empty());

    // Disallowed feature: returns false, records an UnsupportedFeature error.
    assert!(!check.verify_versionf(7, &go1_21(), "min built-in"));
    assert_eq!(check.errors.len(), 1);
    let err = &check.errors[0];
    assert_eq!(err.code, Code::UnsupportedFeature);
    assert!(
        err.msg.contains("min built-in") && err.msg.contains("go1.21"),
        "unexpected message: {:?}",
        err.msg
    );
}

#[test]
fn verify_versionf_passes_when_disabled() {
    // No effective version → everything verifies, no errors.
    let mut check = Checker::new(Config::default());
    check.env.version = String::new();
    assert!(check.verify_versionf(0, &go1_21(), "min built-in"));
    assert!(check.errors.is_empty());
}
