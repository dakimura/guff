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

#[test]
fn parse_v2_revive_severity_settings() {
    let contents = fs::read_to_string(testdata_config("v2_revive_severity.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.revive.severity.as_deref(), Some("warning"));
    let dot = settings
        .revive
        .rules
        .as_ref()
        .and_then(|rules| rules.iter().find(|r| r.name == "dot-imports"));
    assert_eq!(dot.and_then(|r| r.severity.as_deref()), Some("error"));
}

#[test]
fn parse_v2_revive_confidence_and_generated_header() {
    let contents = fs::read_to_string(testdata_config("v2_revive_confidence.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.revive.confidence, Some(0.9));
    assert!(settings.revive.ignore_generated_header);
    let bag = settings.to_bag();
    let revive = bag.get::<guff_revive::Settings>("revive").unwrap();
    assert_eq!(revive.confidence_threshold(), 0.9);
    assert!(revive.ignore_generated_header);
}

#[test]
fn parse_v2_dupl_threshold_settings() {
    let contents = fs::read_to_string(testdata_config("v2_dupl_threshold.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.dupl.threshold, Some(30));
    let bag = settings.to_bag();
    let opts = bag.get::<guff_dupl::Options>("dupl").unwrap();
    assert_eq!(opts.threshold, 30);
}

#[test]
fn parse_v2_misspell_settings() {
    let contents = fs::read_to_string(testdata_config("v2_misspell_restricted.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.misspell.locale.as_deref(), Some("US"));
    assert_eq!(settings.misspell.ignore_words, vec!["amercia"]);
    assert_eq!(settings.misspell.mode.as_deref(), Some("restricted"));
    let bag = settings.to_bag();
    let opts = bag.get::<guff_misspell::Options>("misspell").unwrap();
    assert_eq!(opts.locale, "US");
    assert!(opts.restricted());
    assert_eq!(opts.ignore_words, vec!["amercia"]);
    assert_eq!(opts.extra_words.len(), 1);
}

#[test]
fn parse_v2_style_linter_settings() {
    let contents = fs::read_to_string(testdata_config("v2_style_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.gocyclo.min_complexity, Some(50));
    assert_eq!(settings.dogsled.max_blank_identifiers, Some(4));
    assert_eq!(settings.funlen.statements, Some(50));
    assert_eq!(settings.cyclop.max_complexity, Some(20));
    assert_eq!(settings.lll.line_length, Some(200));
    assert_eq!(settings.nakedret.max_func_lines, Some(50));
    assert_eq!(settings.nlreturn.block_size, Some(10));
    let bag = settings.to_bag();
    assert_eq!(
        bag.get::<guff_style::GocycloOptions>("gocyclo")
            .unwrap()
            .min_complexity,
        50
    );
    assert_eq!(
        bag.get::<guff_style::DogsledOptions>("dogsled")
            .unwrap()
            .max_blank_identifiers,
        4
    );
    assert_eq!(
        bag.get::<guff_style::FunlenOptions>("funlen")
            .unwrap()
            .statements,
        50
    );
    assert_eq!(
        bag.get::<guff_style::CyclopOptions>("cyclop")
            .unwrap()
            .max_complexity,
        20
    );
    assert_eq!(
        bag.get::<guff_style::LllOptions>("lll").unwrap().line_length,
        200
    );
    assert_eq!(
        bag.get::<guff_style::NakedretOptions>("nakedret")
            .unwrap()
            .max_func_lines,
        50
    );
    assert_eq!(
        bag.get::<guff_style::NlreturnOptions>("nlreturn")
            .unwrap()
            .block_size,
        10
    );
}

#[test]
fn parse_v2_style_extended_linter_settings() {
    let contents =
        fs::read_to_string(testdata_config("v2_style_settings_extended.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.cyclop.package_average, Some(5.0));
    assert_eq!(settings.cyclop.skip_tests, Some(true));
    assert_eq!(settings.nakedret.skip_test_files, Some(true));
    assert_eq!(settings.predeclared.ignore, vec!["len".to_string()]);
    assert_eq!(settings.predeclared.qualified, Some(true));
    assert_eq!(settings.perfsprint.integer_format, Some(false));
    assert_eq!(settings.perfsprint.concat_loop, Some(false));
    assert_eq!(settings.perfsprint.loop_other_ops, Some(true));
    assert_eq!(settings.goconst.min_occurrences, Some(10));
    assert_eq!(settings.goconst.numbers, Some(true));
    assert_eq!(settings.goconst.min, Some(2));
    assert_eq!(settings.goconst.max, Some(5));
    assert_eq!(settings.goconst.match_constant, Some(false));
    assert_eq!(settings.mnd.checks.as_ref().map(|c| c.len()), Some(1));
    assert_eq!(settings.prealloc.range_loops, Some(false));
    assert_eq!(settings.tagalign.align, Some(false));
    assert_eq!(settings.wsl.strict_append, Some(false));
    let bag = settings.to_bag();
    assert_eq!(
        bag.get::<guff_style::CyclopOptions>("cyclop")
            .unwrap()
            .package_average,
        5.0
    );
    assert!(bag.get::<guff_style::NakedretOptions>("nakedret").unwrap().skip_test_files);
    assert_eq!(
        bag.get::<guff_style::GoconstOptions>("goconst")
            .unwrap()
            .min_occurrences,
        10
    );
    assert!(bag.get::<guff_style::GoconstOptions>("goconst").unwrap().numbers);
    assert_eq!(
        bag.get::<guff_style::GoconstOptions>("goconst")
            .unwrap()
            .number_min,
        2
    );
    assert_eq!(
        bag.get::<guff_style::GoconstOptions>("goconst")
            .unwrap()
            .number_max,
        5
    );
    assert!(
        !bag.get::<guff_style::GoconstOptions>("goconst")
            .unwrap()
            .match_constant
    );
    assert!(
        !bag.get::<guff_style::PerfsprintOptions>("perfsprint")
            .unwrap()
            .concat_loop
    );
    assert!(
        bag.get::<guff_style::PerfsprintOptions>("perfsprint")
            .unwrap()
            .loop_other_ops
    );
}
