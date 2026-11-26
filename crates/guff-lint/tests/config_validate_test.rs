//! Configs golangci-lint refuses to start on (COMPAT-HARDENING §4).
//!
//! The golden tier compares two finding sets and cannot express "upstream
//! exited before linting", so these rules are asserted here and, end to end
//! against the real golangci-lint, by `compat/reject/run.sh`. Every expected
//! message below was copied from a measured run of the pinned golangci-lint
//! 2.12.2 (`compat/reject/cases/*/expected.txt` holds those runs verbatim).

use guff_lint::{parse_config_str, validate_gocritic_options, ConfigError};

fn reject(yaml: &str) -> String {
    let cfg = parse_config_str(yaml).expect("config parses");
    match cfg.validate() {
        Ok(()) => panic!("expected validation to reject:\n{yaml}"),
        Err(e) => e.to_string(),
    }
}

fn accept(yaml: &str) {
    let cfg = parse_config_str(yaml).expect("config parses");
    if let Err(e) = cfg.validate() {
        panic!("expected validation to accept:\n{yaml}\ngot: {e}");
    }
}

const V2_HEAD: &str = "version: \"2\"\nlinters:\n  default: none\n  enable: [errcheck]\n";

#[test]
fn exclude_rule_with_one_condition_is_a_config_error() {
    let err = reject(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - linters: [errcheck]\n"
    ));
    assert_eq!(
        err,
        "can't load config: error in exclude rule #0: \
         at least 2 of (text, source, path[-except], linters) should be set"
    );
}

#[test]
fn exclude_rule_index_is_the_rules_position() {
    let err = reject(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - linters: [errcheck]\n        text: ok\n      - text: lonely\n"
    ));
    assert!(err.contains("exclude rule #1"), "{err}");
}

#[test]
fn path_and_path_except_together_is_a_config_error() {
    let err = reject(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - path: 'a'\n        path-except: 'b'\n        text: 'c'\n"
    ));
    assert_eq!(
        err,
        "can't load config: error in exclude rule #0: \
         path and path-except should not be set at the same time"
    );
}

#[test]
fn path_filtering_counts_as_one_condition_however_it_is_spelled() {
    // Upstream counts `path` and `path-except` as a single condition on
    // purpose, so a rule with both plus nothing else fails the count *and* the
    // conflict check — and the conflict is the one reported.
    let err = reject(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - path: 'a'\n        path-except: 'b'\n"
    ));
    assert!(err.contains("should not be set at the same time"), "{err}");
}

#[test]
fn two_conditions_are_enough() {
    accept(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - linters: [errcheck]\n        path: '_test\\.go'\n"
    ));
}

#[test]
fn unknown_exclusion_preset_is_a_config_error() {
    let err = reject(&format!("{V2_HEAD}  exclusions:\n    presets: [nope]\n"));
    assert_eq!(err, "can't load config: invalid preset: nope");
}

#[test]
fn camel_case_preset_names_are_refused_like_upstream() {
    // guff's own preset resolution accepts `stdErrorHandling`; golangci-lint
    // v2's vocabulary is kebab-case only, and a config spelling it the old way
    // is one upstream will not start on.
    let err = reject(&format!(
        "{V2_HEAD}  exclusions:\n    presets: [stdErrorHandling]\n"
    ));
    assert_eq!(err, "can't load config: invalid preset: stdErrorHandling");
    accept(&format!(
        "{V2_HEAD}  exclusions:\n    presets: [std-error-handling, comments, legacy, common-false-positives]\n"
    ));
}

#[test]
fn severity_rules_without_a_default_are_a_config_error() {
    let err = reject(&format!(
        "{V2_HEAD}severity:\n  rules:\n    - linters: [errcheck]\n      severity: error\n"
    ));
    assert_eq!(
        err,
        "can't load config: can't set severity rule option: no default severity defined"
    );
}

#[test]
fn v1_spelling_of_the_default_does_not_count_in_a_v2_config() {
    // `default-severity` is v1's key. A v2 file carrying it has no default at
    // all — the same config guff used to run with the v1 value applied.
    let err = reject(&format!(
        "{V2_HEAD}severity:\n  default-severity: warning\n  rules:\n    - linters: [errcheck]\n      severity: error\n"
    ));
    assert!(err.contains("no default severity defined"), "{err}");
    accept(&format!(
        "{V2_HEAD}severity:\n  default: warning\n  rules:\n    - linters: [errcheck]\n      severity: error\n"
    ));
}

#[test]
fn a_severity_rule_needs_its_severity() {
    let err = reject(&format!(
        "{V2_HEAD}severity:\n  default: warning\n  rules:\n    - linters: [errcheck]\n"
    ));
    assert_eq!(
        err,
        "can't load config: error in severity rule #0: severity should be set"
    );
}

#[test]
fn severity_rules_need_only_one_condition() {
    accept(&format!(
        "{V2_HEAD}severity:\n  default: warning\n  rules:\n    - linters: [errcheck]\n      severity: error\n"
    ));
    let err = reject(&format!(
        "{V2_HEAD}severity:\n  default: warning\n  rules:\n    - severity: error\n"
    ));
    assert_eq!(
        err,
        "can't load config: error in severity rule #0: \
         at least 1 of (text, source, path[-except], linters) should be set"
    );
}

#[test]
fn output_path_mode_rel_is_a_config_error() {
    let err = reject(&format!("{V2_HEAD}output:\n  path-mode: rel\n"));
    assert_eq!(err, "can't load config: unsupported output path mode \"rel\"");
    accept(&format!("{V2_HEAD}output:\n  path-mode: abs\n"));
    accept(&format!("{V2_HEAD}output:\n  path-mode: ''\n"));
    accept(V2_HEAD);
}

// --- gocritic option combinations ------------------------------------------

fn gocritic(
    enable_all: bool,
    disable_all: bool,
    enabled_tags: &[&str],
    enabled_checks: &[&str],
    disabled_tags: &[&str],
    disabled_checks: &[&str],
) -> Result<(), ConfigError> {
    let own = |v: &[&str]| v.iter().map(|s| (*s).to_string()).collect::<Vec<_>>();
    validate_gocritic_options(
        enable_all,
        disable_all,
        &own(enabled_tags),
        &own(enabled_checks),
        &own(disabled_tags),
        &own(disabled_checks),
    )
}

#[test]
fn gocritic_enable_all_with_enabled_tags_is_refused() {
    let err = gocritic(true, false, &["performance"], &[], &[], &[]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "gocritic: invalid settings: enable-all and enabled-tags options must not be combined"
    );
}

#[test]
fn gocritic_enable_all_with_enabled_checks_is_refused() {
    let err = gocritic(true, false, &[], &["appendAssign"], &[], &[]).unwrap_err();
    assert!(err.to_string().ends_with("enable-all and enabled-checks options must not be combined"));
}

#[test]
fn gocritic_disable_all_with_disabled_checks_is_refused() {
    let err = gocritic(false, true, &[], &["x"], &[], &["appendAssign"]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "gocritic: invalid settings: disable-all and disabled-checks options must not be combined"
    );
}

#[test]
fn gocritic_disable_all_with_disabled_tags_is_refused() {
    let err = gocritic(false, true, &[], &["x"], &["performance"], &[]).unwrap_err();
    assert!(err.to_string().ends_with("disable-all and disabled-tags options must not be combined"));
}

#[test]
fn gocritic_disable_all_enabling_nothing_is_refused() {
    let err = gocritic(false, true, &[], &[], &[], &[]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "gocritic: invalid settings: all checks were disabled, \
         but no one check was enabled: at least one must be enabled"
    );
}

#[test]
fn gocritic_enable_all_and_disable_all_is_refused() {
    let err = gocritic(true, true, &[], &[], &[], &[]).unwrap_err();
    assert!(err.to_string().ends_with("enable-all and disable-all options must not be combined"));
}

#[test]
fn gocritic_disable_all_with_something_enabled_is_fine() {
    gocritic(false, true, &["performance"], &[], &[], &[]).unwrap();
    gocritic(false, true, &[], &["appendAssign"], &[], &[]).unwrap();
    gocritic(false, false, &[], &[], &[], &["appendAssign"]).unwrap();
}

// --- what validation must NOT reject ---------------------------------------

#[test]
fn a_regex_go_accepts_and_rust_does_not_is_still_accepted() {
    // Upstream compiles these patterns with Go's regexp and rejects the config
    // if one fails. guff does not port that check: the dialects differ, and a
    // pattern that only Rust refuses would make guff reject a config
    // golangci-lint runs. `(?i)` is fine in both; a lookahead is Go-invalid and
    // Rust-invalid, so the neutral assertion is that we do not compile at all.
    accept(&format!(
        "{V2_HEAD}  exclusions:\n    rules:\n      - path: '(?P<x>a)(?=b'\n        text: 'y'\n"
    ));
}

#[test]
fn v1_configs_keep_their_own_two_rules() {
    let err = parse_config_str(
        "linters:\n  enable: [errcheck]\nissues:\n  exclude-rules:\n    - linters: [errcheck]\n",
    )
    .unwrap()
    .validate()
    .unwrap_err();
    assert!(err.to_string().contains("at least 2 of"), "{err}");

    // v1 spells the default `default-severity`, and there it counts.
    parse_config_str(
        "linters:\n  enable: [errcheck]\nseverity:\n  default-severity: warning\n  rules:\n    - linters: [errcheck]\n      severity: error\n",
    )
    .unwrap()
    .validate()
    .unwrap();
}
