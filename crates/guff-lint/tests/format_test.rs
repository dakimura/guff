//! R6: formatter abstraction + `--out-format text`.

use std::process::Command;

use guff_lint::{
    print_issues, resolve_out_formats, Issue, OutputFormatKind, TextFormatter, Formatter,
};
use guff_analysis::Diagnostic;

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
}

#[test]
fn resolve_out_formats_rejects_json() {
    let err = resolve_out_formats(&["json".into()]).unwrap_err();
    assert!(err.contains("json"), "{err}");
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
