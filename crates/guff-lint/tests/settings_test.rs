//! R4: `linters.settings` changes analyzer behaviour / selection.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use guff_analysis::SettingsBag;
use guff_lint::{
    analyzers_for_linter_with_settings, parse_config_str, ErrcheckSettings, GovetSettings,
    IssueFilter, LintResult, LinterSettings, StaticcheckSettings,
};
use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
use guff_runner::{run_on_packages, RunnerOptions};
use guff_types::default_sizes;

fn testdata_config(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/config")
        .join(name)
}

fn fixture_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run")
        .join(name)
}

fn typecheck_fixture(dir: &PathBuf, id: &str, file: &str) -> Arc<Package> {
    let mut pkg = Package {
        id: id.into(),
        pkg_path: id.into(),
        dir: dir.clone(),
        compiled_go_files: vec![dir.join(file)],
        ..Package::default()
    };
    let fset = guff::position::FileSet::new();
    typecheck_package(
        &mut pkg,
        &fset,
        &HashMap::new(),
        &HashMap::new(),
        default_sizes(),
        &TypecheckEnv::default(),
        LoadMode::LOAD_SYNTAX,
    );
    Arc::new(pkg)
}

fn run_errcheck(pkg: Arc<Package>, settings: &LinterSettings) -> LintResult {
    let analyzers =
        analyzers_for_linter_with_settings("errcheck", settings).expect("errcheck");
    let run = run_on_packages(
        &analyzers,
        std::slice::from_ref(&pkg),
        &RunnerOptions {
            sequential: true,
            settings: settings.to_bag(),
            ..RunnerOptions::default()
        },
    )
    .expect("run errcheck");
    LintResult {
        packages: vec![pkg],
        run,
        filter: IssueFilter::default(),
        cached_issues: Vec::new(),
    }
}

#[test]
fn parse_v2_errcheck_check_blank_settings() {
    let contents = fs::read_to_string(testdata_config("v2_errcheck_check_blank.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.errcheck,
        ErrcheckSettings {
            check_blank: true,
            check_type_assertions: false,
        }
    );
}

#[test]
fn parse_v2_govet_and_staticcheck_settings() {
    let govet = fs::read_to_string(testdata_config("v2_govet_enable_printf.yml")).unwrap();
    let s = LinterSettings::from_yaml(parse_config_str(&govet).unwrap().linter_settings_raw());
    assert_eq!(
        s.govet,
        GovetSettings {
            disable_all: true,
            enable: vec!["printf".into()],
            ..GovetSettings::default()
        }
    );

    let sc = fs::read_to_string(testdata_config("v2_staticcheck_disable_sa1004.yml")).unwrap();
    let s = LinterSettings::from_yaml(parse_config_str(&sc).unwrap().linter_settings_raw());
    assert_eq!(
        s.staticcheck,
        StaticcheckSettings {
            checks: Some(vec!["all".into(), "-SA1004".into()]),
        }
    );
}

#[test]
fn errcheck_check_blank_true_flags_blank_assignment() {
    let dir = fixture_dir("errcheck_blank");
    let pkg = typecheck_fixture(&dir, "example.com/guff-test/errcheck_blank", "bad.go");
    assert!(!pkg.ill_typed, "{:?}", pkg.errors);

    let off = LinterSettings::default();
    let without = run_errcheck(Arc::clone(&pkg), &off);
    assert_eq!(
        without.raw_diagnostic_count(),
        0,
        "default errcheck should ignore `_ = returnsErr()`"
    );

    let on = LinterSettings {
        errcheck: ErrcheckSettings {
            check_blank: true,
            ..ErrcheckSettings::default()
        },
        ..LinterSettings::default()
    };
    let with = run_errcheck(pkg, &on);
    assert!(
        with.raw_diagnostic_count() > 0,
        "check-blank: true must flag `_ = returnsErr()`"
    );
}

#[test]
fn settings_bag_carries_errcheck_options() {
    let settings = LinterSettings {
        errcheck: ErrcheckSettings {
            check_blank: true,
            check_type_assertions: true,
        },
        ..LinterSettings::default()
    };
    let bag: Arc<SettingsBag> = settings.to_bag();
    let opts = bag
        .get::<guff_errcheck::Options>("errcheck")
        .expect("errcheck options");
    assert!(opts.check_blank);
    assert!(opts.check_asserts);
}
