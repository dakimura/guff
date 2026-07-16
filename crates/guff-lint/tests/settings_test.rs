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
            ..ErrcheckSettings::default()
        }
    );
}

#[test]
fn parse_v2_errcheck_exclude_functions_settings() {
    let contents =
        fs::read_to_string(testdata_config("v2_errcheck_exclude_functions.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.errcheck,
        ErrcheckSettings {
            check_blank: true,
            exclude_functions: vec![
                "io.Copy".into(),
                "io.WriteString".into(),
                "(net/http.ResponseWriter).Write".into(),
            ],
            ..ErrcheckSettings::default()
        }
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_errcheck::Options>("errcheck")
        .expect("errcheck options");
    assert!(opts.check_blank);
    assert!(!opts.disable_default_exclusions);
    assert_eq!(opts.exclude_functions.len(), 3);
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
            ..ErrcheckSettings::default()
        },
        ..LinterSettings::default()
    };
    let bag: Arc<SettingsBag> = settings.to_bag();
    let opts = bag
        .get::<guff_errcheck::Options>("errcheck")
        .expect("errcheck options");
    assert!(opts.check_blank);
    assert!(opts.check_asserts);
    assert!(!opts.disable_default_exclusions);
    assert!(opts.exclude_functions.is_empty());
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
fn parse_v2_revive_prometheus_style_rule_arguments() {
    let contents = fs::read_to_string(testdata_config("v2_revive_prometheus_args.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    let bag = settings.to_bag();
    let revive = bag
        .get::<guff_revive::Settings>("revive")
        .expect("revive settings");

    let ctx = revive.rule("context-as-argument").expect("context-as-argument");
    match ctx.arguments.first() {
        Some(guff_revive::RuleArgument::Map(map)) => {
            match map.get("allowTypesBefore") {
                Some(guff_revive::RuleArgument::String(s)) => {
                    assert_eq!(s, "*testing.T,testing.TB");
                }
                other => panic!("expected allowTypesBefore string, got {other:?}"),
            }
        }
        other => panic!("expected map argument, got {other:?}"),
    }

    for name in ["early-return", "indent-error-flow", "superfluous-else"] {
        let rule = revive.rule(name).unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            rule.arguments.iter().any(|a| matches!(
                a,
                guff_revive::RuleArgument::String(s) if s == "preserveScope"
            )),
            "{name} should carry preserveScope: {:?}",
            rule.arguments
        );
    }
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
    assert_eq!(settings.perfsprint.int_conversion, Some(false));
    assert_eq!(settings.perfsprint.err_error, Some(true));
    assert_eq!(settings.perfsprint.errorf, Some(false));
    assert_eq!(settings.perfsprint.concat_loop, Some(false));
    assert_eq!(settings.perfsprint.loop_other_ops, Some(true));
    assert_eq!(settings.goconst.min_occurrences, Some(10));
    assert_eq!(settings.goconst.numbers, Some(true));
    assert_eq!(settings.goconst.min, Some(2));
    assert_eq!(settings.goconst.max, Some(5));
    assert_eq!(settings.goconst.match_constant, Some(false));
    assert_eq!(settings.goconst.find_duplicates, Some(true));
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
        bag.get::<guff_style::GoconstOptions>("goconst")
            .unwrap()
            .find_duplicates
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
    assert!(
        !bag.get::<guff_style::PerfsprintOptions>("perfsprint")
            .unwrap()
            .int_conversion
    );
    assert!(
        bag.get::<guff_style::PerfsprintOptions>("perfsprint")
            .unwrap()
            .err_error
    );
    assert!(
        !bag.get::<guff_style::PerfsprintOptions>("perfsprint")
            .unwrap()
            .errorf
    );
    assert_eq!(settings.copyloopvar.check_alias, Some(true));
    assert_eq!(settings.usetesting.os_setenv, Some(true));
    assert_eq!(settings.usetesting.os_temp_dir, Some(true));
    assert_eq!(settings.usetesting.os_mkdir_temp, Some(false));
    assert_eq!(settings.usestdlibvars.http_method, Some(false));
    assert_eq!(settings.usestdlibvars.http_status_code, Some(false));
    assert!(
        bag.get::<guff_style::CopyloopvarOptions>("copyloopvar")
            .unwrap()
            .check_alias
    );
    assert!(
        bag.get::<guff_style::UsetestingOptions>("usetesting")
            .unwrap()
            .os_setenv
    );
    assert!(
        !bag.get::<guff_style::UsetestingOptions>("usetesting")
            .unwrap()
            .os_mkdir_temp
    );
    assert!(
        !bag.get::<guff_style::UsestdlibvarsOptions>("usestdlibvars")
            .unwrap()
            .http_method
    );
}

#[test]
fn parse_v2_usestdlibvars_optional_settings() {
    let contents =
        fs::read_to_string(testdata_config("v2_usestdlibvars_optional.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.usestdlibvars.http_method, Some(false));
    assert_eq!(settings.usestdlibvars.http_status_code, Some(false));
    assert_eq!(settings.usestdlibvars.time_weekday, Some(true));
    assert_eq!(settings.usestdlibvars.time_month, Some(true));
    assert_eq!(settings.usestdlibvars.time_layout, Some(true));
    assert_eq!(settings.usestdlibvars.crypto_hash, Some(true));
    assert_eq!(settings.usestdlibvars.default_rpc_path, Some(true));
    assert_eq!(settings.usestdlibvars.sql_isolation_level, Some(true));
    assert_eq!(settings.usestdlibvars.tls_signature_scheme, Some(true));
    assert_eq!(settings.usestdlibvars.constant_kind, Some(true));
    assert_eq!(settings.usestdlibvars.time_date_month, Some(true));
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::UsestdlibvarsOptions>("usestdlibvars")
        .unwrap();
    assert!(!opts.http_method);
    assert!(opts.time_weekday);
    assert!(opts.crypto_hash);
    assert!(opts.time_date_month);
}

#[test]
fn parse_v2_unconvert_settings() {
    let contents = fs::read_to_string(testdata_config("v2_unconvert_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.unconvert.fast_math, Some(true));
    assert_eq!(settings.unconvert.safe, Some(true));
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::UnconvertOptions>("unconvert")
        .expect("unconvert options");
    assert!(opts.fast_math);
    assert!(opts.safe);
}

#[test]
fn parse_v2_exhaustruct_settings() {
    let contents = fs::read_to_string(testdata_config("v2_exhaustruct_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.exhaustruct.include, vec![r".*\.Included$"]);
    assert_eq!(settings.exhaustruct.exclude, vec![r".*\.SkipMe$"]);
    assert_eq!(settings.exhaustruct.allow_empty, Some(true));
    assert_eq!(
        settings.exhaustruct.allow_empty_rx,
        vec![r".*\.OptEmpty$"]
    );
    assert_eq!(settings.exhaustruct.allow_empty_returns, Some(true));
    assert_eq!(settings.exhaustruct.allow_empty_declarations, Some(true));
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::ExhaustructOptions>("exhaustruct")
        .expect("exhaustruct options");
    assert!(opts.allow_empty);
    assert!(opts.allow_empty_returns);
    assert!(opts.allow_empty_declarations);
    assert_eq!(opts.include.len(), 1);
}

#[test]
fn parse_v2_exhaustive_settings() {
    let contents = fs::read_to_string(testdata_config("v2_exhaustive_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.exhaustive.check, vec!["switch"]);
    assert_eq!(settings.exhaustive.default_signifies_exhaustive, Some(true));
    assert_eq!(settings.exhaustive.default_case_required, Some(false));
    assert_eq!(
        settings.exhaustive.ignore_enum_members.as_deref(),
        Some(r"example\.com/exhaustive\.Skip.+")
    );
    assert_eq!(
        settings.exhaustive.ignore_enum_types.as_deref(),
        Some(r"example\.com/exhaustive\.IgnoreMe")
    );
    assert_eq!(settings.exhaustive.package_scope_only, Some(true));
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::ExhaustiveOptions>("exhaustive")
        .expect("exhaustive options");
    assert!(opts.check_switch);
    assert!(!opts.check_map);
    assert!(opts.default_signifies_exhaustive);
    assert!(!opts.default_case_required);
    assert!(opts.package_scope_only);
}

#[test]
fn parse_v2_musttag_settings() {
    let contents = fs::read_to_string(testdata_config("v2_musttag_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.musttag.functions.len(), 1);
    assert_eq!(
        settings.musttag.functions[0].name,
        "example.com/musttag.DecodeYAML"
    );
    assert_eq!(settings.musttag.functions[0].tag, "yaml");
    assert_eq!(settings.musttag.functions[0].arg_pos, 1);
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::MusttagOptions>("musttag")
        .expect("musttag options");
    assert_eq!(opts.functions.len(), 1);
    assert_eq!(opts.functions[0].tag, "yaml");
}

#[test]
fn parse_v2_loggercheck_settings() {
    let contents = fs::read_to_string(testdata_config("v2_loggercheck_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(settings.loggercheck.slog, Some(true));
    assert_eq!(settings.loggercheck.kitlog, Some(false));
    assert!(settings.loggercheck.require_string_key);
    assert!(settings.loggercheck.no_printf_like);
    assert_eq!(
        settings.loggercheck.rules,
        vec!["example.com/loggercheck.MyLog".to_string()]
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::LoggercheckOptions>("loggercheck")
        .expect("loggercheck options");
    assert!(opts.slog);
    assert!(!opts.kitlog);
    assert!(opts.require_string_key);
    assert!(opts.no_printf_like);
    assert_eq!(opts.rules, vec!["example.com/loggercheck.MyLog".to_string()]);
}

#[test]
fn parse_v2_sloglint_settings() {
    let contents = fs::read_to_string(testdata_config("v2_sloglint_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert!(settings.sloglint.no_mixed_args);
    assert!(settings.sloglint.attr_only);
    assert_eq!(settings.sloglint.no_global.as_deref(), Some("all"));
    assert_eq!(settings.sloglint.context.as_deref(), Some("scope"));
    assert!(settings.sloglint.static_msg);
    assert_eq!(settings.sloglint.msg_style.as_deref(), Some("lowercased"));
    assert!(settings.sloglint.no_raw_keys);
    assert_eq!(settings.sloglint.key_naming_case.as_deref(), Some("snake"));
    assert_eq!(settings.sloglint.allowed_keys, vec!["user_id".to_string()]);
    assert_eq!(
        settings.sloglint.forbidden_keys,
        vec!["time".to_string(), "level".to_string()]
    );
    assert!(settings.sloglint.args_on_sep_lines);
    assert_eq!(settings.sloglint.custom_funcs.len(), 1);
    assert_eq!(
        settings.sloglint.custom_funcs[0].name,
        "example.com/sloglint.MyLog"
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::SloglintOptions>("sloglint")
        .expect("sloglint options");
    assert!(opts.attr_only);
    assert_eq!(opts.no_global.as_deref(), Some("all"));
    assert_eq!(opts.custom_funcs.len(), 1);
    assert_eq!(opts.custom_funcs[0].msg_pos, 0);
}

#[test]
fn parse_v2_testifylint_settings() {
    let contents = fs::read_to_string(testdata_config("v2_testifylint_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert!(settings.testifylint.disable_all);
    assert!(!settings.testifylint.enable_all);
    assert_eq!(
        settings.testifylint.enable,
        vec![
            "bool-compare".to_string(),
            "empty".to_string(),
            "expected-actual".to_string(),
            "time-compare".to_string(),
            "formatter".to_string(),
            "suite-extra-assert-call".to_string(),
        ]
    );
    assert!(settings.testifylint.bool_compare.ignore_custom_types);
    assert_eq!(
        settings.testifylint.expected_actual.pattern.as_deref(),
        Some("^wanted$")
    );
    assert_eq!(
        settings
            .testifylint
            .time_compare
            .suppress_calls_pattern
            .as_deref(),
        Some("UTC|Round")
    );
    assert!(!settings.testifylint.formatter.check_format_string);
    assert!(settings.testifylint.formatter.require_f_funcs);
    assert!(!settings.testifylint.formatter.require_string_msg);
    assert_eq!(
        settings.testifylint.suite_extra_assert_call.mode.as_deref(),
        Some("require")
    );
    assert_eq!(
        settings.testifylint.require_error.fn_pattern.as_deref(),
        Some("^NoError$")
    );
    assert!(settings.testifylint.go_require.ignore_http_handlers);
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::TestifylintOptions>("testifylint")
        .expect("testifylint options");
    assert!(opts.disable_all);
    assert_eq!(
        opts.require_error_fn_pattern.as_deref(),
        Some("^NoError$")
    );
    assert!(opts.go_require_ignore_http_handlers);
    assert_eq!(
        opts.enable,
        vec![
            "bool-compare".to_string(),
            "empty".to_string(),
            "expected-actual".to_string(),
            "time-compare".to_string(),
            "formatter".to_string(),
            "suite-extra-assert-call".to_string(),
        ]
    );
    assert!(opts.bool_compare_ignore_custom_types);
    assert_eq!(opts.expected_actual_pattern.as_deref(), Some("^wanted$"));
    assert_eq!(
        opts.time_compare_suppress_calls_pattern.as_deref(),
        Some("UTC|Round")
    );
    assert!(!opts.formatter_check_format_string);
    assert!(opts.formatter_require_f_funcs);
    assert!(!opts.formatter_require_string_msg);
    assert_eq!(
        opts.suite_extra_assert_call_mode,
        guff_style::SuiteExtraAssertCallMode::Require
    );
}

#[test]
fn parse_v2_errchkjson_settings() {
    use guff_lint::ErrchkjsonSettings;

    let contents = fs::read_to_string(testdata_config("v2_errchkjson_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.errchkjson,
        ErrchkjsonSettings {
            check_error_free_encoding: true,
            report_no_exported: true,
        }
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_error::ErrchkjsonOptions>("errchkjson")
        .expect("errchkjson options");
    assert!(!opts.omit_safe);
    assert!(opts.report_no_exported);
}

#[test]
fn parse_v2_wrapcheck_settings() {
    use guff_lint::WrapcheckSettings;

    let contents = fs::read_to_string(testdata_config("v2_wrapcheck_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.wrapcheck,
        WrapcheckSettings {
            ignore_sigs: Some(vec![".Errorf(".into(), "errors.New(".into()]),
            extra_ignore_sigs: vec!["encoding/json.Marshal(".into()],
            ignore_sig_regexps: vec![r"\.New.*Error\(".into()],
            ignore_package_globs: vec!["encoding/*".into()],
            ignore_interface_regexps: vec!["Reader$".into()],
            report_internal_errors: true,
        }
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_error::WrapcheckOptions>("wrapcheck")
        .expect("wrapcheck options");
    assert_eq!(
        opts.ignore_sigs.as_ref().map(|v| v.len()),
        Some(2)
    );
    assert_eq!(opts.extra_ignore_sigs.len(), 1);
    assert_eq!(opts.ignore_package_globs, vec!["encoding/*".to_string()]);
    assert!(opts.report_internal_errors);
}

#[test]
fn parse_v2_comment_settings() {
    use guff_lint::{DupwordSettings, GodotSettings, GodoxSettings};

    let contents = fs::read_to_string(testdata_config("v2_comment_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.godot,
        GodotSettings {
            scope: Some("declarations".into()),
            exclude: vec!["^FIXME:".into(), "^TODO:".into()],
            period: Some(false),
            capital: Some(true),
        }
    );
    assert_eq!(
        settings.godox,
        GodoxSettings {
            keywords: vec!["NOTE".into(), "HACK".into()],
        }
    );
    assert_eq!(
        settings.dupword,
        DupwordSettings {
            keywords: vec!["the".into()],
            ignore: vec!["is".into()],
            comments_only: Some(true),
        }
    );
    let bag = settings.to_bag();
    let godot = bag
        .get::<guff_comment::GodotOptions>("godot")
        .expect("godot options");
    assert!(!godot.period);
    assert!(godot.capital);
    assert_eq!(godot.exclude.len(), 2);
    let godox = bag
        .get::<guff_comment::GodoxOptions>("godox")
        .expect("godox options");
    assert_eq!(godox.keywords, vec!["NOTE".to_string(), "HACK".to_string()]);
    let dupword = bag
        .get::<guff_comment::DupwordOptions>("dupword")
        .expect("dupword options");
    assert!(dupword.comments_only);
    assert_eq!(dupword.keywords, vec!["the".to_string()]);
    assert_eq!(dupword.ignore, vec!["is".to_string()]);
}

#[test]
fn parse_v2_import_settings() {
    use guff_lint::{DepguardSettings, GomoddirectivesSettings};

    let contents = fs::read_to_string(testdata_config("v2_import_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());

    assert_eq!(settings.depguard.rules.len(), 1);
    let main = settings.depguard.rules.get("Main").expect("Main rule");
    assert_eq!(main.list_mode.as_deref(), Some("lax"));
    assert_eq!(main.files, vec!["$all".to_string(), "!$test".to_string()]);
    assert_eq!(main.allow, vec!["$gostd".to_string()]);
    assert_eq!(main.deny.len(), 1);
    assert_eq!(main.deny[0].pkg, "github.com/sirupsen/logrus");
    assert_eq!(main.deny[0].desc, "use log/slog");

    assert_eq!(
        settings.gomoddirectives,
        GomoddirectivesSettings {
            replace_local: true,
            replace_allow_list: vec!["launchpad.net/gocheck".into()],
            retract_allow_no_explanation: true,
            exclude_forbidden: true,
            toolchain_forbidden: true,
            tool_forbidden: true,
            go_debug_forbidden: true,
        }
    );

    // v1 blocked logrus + v2 blocked pkg/errors, both set local_replace.
    assert!(settings.gomodguard.local_replace_directives);
    assert!(settings
        .gomodguard
        .blocked_modules
        .iter()
        .any(|(m, r)| m == "github.com/sirupsen/logrus" && r.contains("log/slog")));
    assert!(settings
        .gomodguard
        .blocked_modules
        .iter()
        .any(|(m, r)| m == "github.com/pkg/errors" && r.contains("std errors")));

    let bag = settings.to_bag();
    let dep = bag
        .get::<guff_import::DepguardOptions>("depguard")
        .expect("depguard options");
    assert_eq!(dep.rules.len(), 1);
    assert_eq!(dep.rules[0].list_mode, guff_import::ListMode::Lax);
    let gmd = bag
        .get::<guff_import::GomoddirectivesOptions>("gomoddirectives")
        .expect("gomoddirectives options");
    assert!(gmd.replace_local);
    assert!(gmd.exclude_forbidden);
    let gg = bag
        .get::<guff_import::GomodguardOptions>("gomodguard")
        .expect("gomodguard options");
    assert!(gg.local_replace_directives);
    assert_eq!(gg.blocked_modules.len(), 2);

    // Smoke: empty DepguardSettings round-trips to empty rules (analyzer default).
    let _ = DepguardSettings::default();
}

#[test]
fn parse_v2_modernize_settings() {
    let contents = fs::read_to_string(testdata_config("v2_modernize_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert_eq!(
        settings.modernize.disable,
        vec![
            "omitzero".to_string(),
            "newexpr".to_string(),
            "any".to_string()
        ]
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::ModernizeOptions>("modernize")
        .expect("modernize options");
    assert_eq!(opts.disable.len(), 3);
    assert!(opts.disable.iter().any(|d| d == "omitzero"));
    assert!(opts.disable.iter().any(|d| d == "newexpr"));
}

#[test]
fn parse_v2_gocritic_settings() {
    let contents = fs::read_to_string(testdata_config("v2_gocritic_settings.yml")).unwrap();
    let cfg = parse_config_str(&contents).unwrap();
    let settings = LinterSettings::from_yaml(cfg.linter_settings_raw());
    assert!(settings.gocritic.enable_all);
    assert_eq!(
        settings.gocritic.disabled_checks,
        vec![
            "appendAssign".to_string(),
            "ifElseChain".to_string(),
            "underef".to_string(),
        ]
    );
    let bag = settings.to_bag();
    let opts = bag
        .get::<guff_style::GocriticOptions>("gocritic")
        .expect("gocritic options");
    assert!(opts.enable_all);
    assert!(opts.disabled_checks.iter().any(|d| d == "appendAssign"));
}
