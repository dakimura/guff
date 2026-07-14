//! R6–R8: formatter abstraction + `--out-format` variants.

use std::process::Command;

use guff_lint::{
    print_issues, resolve_out_formats, CheckstyleFormatter, Formatter, GithubActionsFormatter,
    Issue, JsonFormatter, OutputFormatKind, SarifFormatter, TabFormatter, TextFormatter,
};
use guff_analysis::Diagnostic;
use serde_json::Value;

fn sample_issue() -> Issue {
    Issue {
        from_linter: "errcheck".into(),
        analyzer: "errcheck".into(),
        text: "Error return value is not checked".into(),
        severity: String::new(),
        filename: "/tmp/bad.go".into(),
        line: 3,
        column: 1,
        source_line: None,
        diagnostic: Diagnostic {
            message: "Error return value is not checked".into(),
            ..Diagnostic::default()
        },
    }
}

#[test]
fn out_format_text_matches_print_text() {
    let issues = vec![sample_issue()];
    let mut via_trait = Vec::new();
    TextFormatter::new()
        .print(&issues, &mut via_trait)
        .unwrap();

    let mut via_print_issues = Vec::new();
    print_issues(&[OutputFormatKind::Text], &issues, &mut via_print_issues).unwrap();

    assert_eq!(via_trait, via_print_issues);
    let s = String::from_utf8(via_trait).unwrap();
    assert_eq!(
        s,
        "/tmp/bad.go:3:1: Error return value is not checked (errcheck)\n"
    );
}

#[test]
fn out_format_json_keys_match_golangci() {
    let issues = vec![sample_issue()];
    let mut buf = Vec::new();
    JsonFormatter::new().print(&issues, &mut buf).unwrap();

    let v: Value = serde_json::from_slice(&buf).unwrap();
    assert!(v.get("Issues").unwrap().is_array());
    assert!(v.get("Report").unwrap().is_null());

    let issue = &v["Issues"][0];
    for key in [
        "FromLinter",
        "Text",
        "Severity",
        "SourceLines",
        "Pos",
        "ExpectNoLint",
        "ExpectedNoLintLinter",
    ] {
        assert!(issue.get(key).is_some(), "missing key {key}");
    }
    assert_eq!(issue["FromLinter"], "errcheck");
    assert_eq!(issue["Text"], "Error return value is not checked");
    assert!(issue["SourceLines"].is_null());
    assert_eq!(issue["Pos"]["Filename"], "/tmp/bad.go");
    assert_eq!(issue["Pos"]["Line"], 3);
    assert_eq!(issue["Pos"]["Column"], 1);
}

#[test]
fn out_format_checkstyle_is_valid_xml() {
    let issues = vec![sample_issue()];
    let mut buf = Vec::new();
    CheckstyleFormatter::new().print(&issues, &mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.starts_with("<?xml"));
    assert!(s.contains(r#"<checkstyle version="5.0">"#));
    assert!(s.contains(r#"source="errcheck""#));
    assert!(s.contains(r#"message="Error return value is not checked""#));
}

#[test]
fn out_format_sarif_schema() {
    let issues = vec![sample_issue()];
    let mut buf = Vec::new();
    SarifFormatter::new().print(&issues, &mut buf).unwrap();
    let v: Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "guff");
    assert_eq!(v["runs"][0]["results"][0]["ruleId"], "errcheck");
}

#[test]
fn out_format_github_actions_annotation() {
    let issues = vec![sample_issue()];
    let mut buf = Vec::new();
    GithubActionsFormatter::new()
        .print(&issues, &mut buf)
        .unwrap();
    assert_eq!(
        String::from_utf8(buf).unwrap(),
        "::error file=/tmp/bad.go,line=3,col=1::Error return value is not checked (errcheck)\n"
    );
}

#[test]
fn out_format_tab_columns() {
    let issues = vec![sample_issue()];
    let mut buf = Vec::new();
    TabFormatter::new().print(&issues, &mut buf).unwrap();
    let s = String::from_utf8(buf).unwrap();
    assert!(s.contains("errcheck"));
    assert!(s.contains("Error return value is not checked"));
    assert!(s.contains("/tmp/bad.go:3:1"));
}

#[test]
fn resolve_out_formats_defaults_to_text() {
    assert_eq!(
        resolve_out_formats(&[]).unwrap(),
        vec![OutputFormatKind::Text]
    );
    assert_eq!(
        resolve_out_formats(&["text".into()]).unwrap(),
        vec![OutputFormatKind::Text]
    );
    assert_eq!(
        resolve_out_formats(&["line-number".into()]).unwrap(),
        vec![OutputFormatKind::Text]
    );
    assert_eq!(
        resolve_out_formats(&["colored-line-number".into()]).unwrap(),
        vec![OutputFormatKind::Colored]
    );
    assert_eq!(
        resolve_out_formats(&["json".into()]).unwrap(),
        vec![OutputFormatKind::Json]
    );
    assert_eq!(
        resolve_out_formats(&["checkstyle".into()]).unwrap(),
        vec![OutputFormatKind::Checkstyle]
    );
    assert_eq!(
        resolve_out_formats(&["sarif".into()]).unwrap(),
        vec![OutputFormatKind::Sarif]
    );
    assert_eq!(
        resolve_out_formats(&["tab".into()]).unwrap(),
        vec![OutputFormatKind::Tab]
    );
    assert_eq!(
        resolve_out_formats(&["github-actions".into()]).unwrap(),
        vec![OutputFormatKind::GithubActions]
    );
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_guff")
}

fn fixture_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run/unchecked_error")
}

#[test]
fn cli_out_format_text_writes_same_style_as_default() {
    let dir = fixture_dir();

    let default = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn default");

    let with_flag = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            "--out-format",
            "text",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn --out-format text");

    assert!(
        default.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&default.stderr)
    );
    assert!(
        with_flag.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&with_flag.stderr)
    );

    let a = String::from_utf8_lossy(&default.stdout);
    let b = String::from_utf8_lossy(&with_flag.stdout);
    assert_eq!(a, b, "default vs --out-format text must match");
    assert!(a.contains("bad.go"), "expected path in output: {a}");
}

#[test]
fn cli_out_format_json_writes_golangci_schema() {
    let dir = fixture_dir();
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            "--out-format",
            "json",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn --out-format json");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(stdout.trim()).expect("valid JSON on stdout");
    assert!(v["Issues"].is_array(), "Issues array missing: {stdout}");
    assert!(!v["Issues"].as_array().unwrap().is_empty());
    assert!(v["Report"].is_null());

    let first = &v["Issues"][0];
    assert_eq!(first["FromLinter"], "errcheck");
    assert!(first["Text"].as_str().unwrap().contains("Error") || !first["Text"].as_str().unwrap().is_empty());
    assert!(first["Pos"]["Filename"]
        .as_str()
        .unwrap()
        .contains("bad.go"));
    assert!(first["Pos"]["Line"].as_i64().unwrap() > 0);
}

#[test]
fn cli_out_format_checkstyle_writes_xml() {
    let dir = fixture_dir();
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            "--out-format",
            "checkstyle",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn --out-format checkstyle");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains(r#"<checkstyle version="5.0">"#), "{stdout}");
    assert!(stdout.contains(r#"source="errcheck""#), "{stdout}");
}

#[test]
fn cli_out_format_sarif_writes_schema() {
    let dir = fixture_dir();
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            "--out-format",
            "sarif",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn --out-format sarif");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
    assert_eq!(v["version"], "2.1.0");
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "guff");
    assert!(!v["runs"][0]["results"].as_array().unwrap().is_empty());
}

#[test]
fn cli_out_format_github_actions_writes_annotations() {
    let dir = fixture_dir();
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--sequential",
            "--timeout",
            "0",
            "--issues-exit-code",
            "0",
            "--out-format",
            "github-actions",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn --out-format github-actions");

    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("::error file="), "{stdout}");
    assert!(stdout.contains("(errcheck)"), "{stdout}");
}

#[test]
fn cli_out_format_unknown_exits_2() {
    let dir = fixture_dir();
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--out-format",
            "not-a-format",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("unknown output format") || err.contains("not-a-format"),
        "stderr={err}"
    );
}
