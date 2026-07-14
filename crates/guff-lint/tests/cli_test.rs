//! R5: `guff version`, `guff linters`, `--timeout`, `-j/--concurrency`.

use std::process::Command;

use guff_lint::{
    format_linters_listing, guff_version, parse_go_duration, partition_linters, version_banner,
    LinterDefault, LinterSelection, DEFAULT_TIMEOUT,
};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_guff")
}

#[test]
fn version_banner_contains_crate_version() {
    let banner = version_banner();
    assert!(banner.contains(guff_version()));
    assert!(banner.starts_with("guff has version "));
}

#[test]
fn cli_version_prints_banner() {
    let out = Command::new(bin())
        .args(["version"])
        .output()
        .expect("spawn guff version");
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("guff has version"),
        "unexpected stdout: {stdout}"
    );
    assert!(stdout.contains(guff_version()));
}

#[test]
fn cli_version_short() {
    let out = Command::new(bin())
        .args(["version", "--short"])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert_eq!(stdout, guff_version());
}

#[test]
fn partition_standard_enables_five() {
    let sel = LinterSelection::default();
    let (enabled, disabled) = partition_linters(&sel);
    for name in ["errcheck", "govet", "ineffassign", "staticcheck", "unused"] {
        assert!(enabled.iter().any(|e| e == name), "missing enabled {name}");
    }
    assert!(disabled.iter().any(|d| d == "nolintlint"));
}

#[test]
fn partition_none_with_enable() {
    let sel = LinterSelection {
        default: LinterDefault::None,
        enable: vec!["errcheck".into()],
        disable: vec![],
    };
    let (enabled, disabled) = partition_linters(&sel);
    assert_eq!(enabled, vec!["errcheck".to_string()]);
    assert!(disabled.contains(&"govet".to_string()));
    assert!(disabled.contains(&"staticcheck".to_string()));
}

#[test]
fn format_linters_listing_has_sections() {
    let mut buf = Vec::new();
    format_linters_listing(
        &["errcheck".into()],
        &["govet".into()],
        &mut buf,
    )
    .unwrap();
    let text = String::from_utf8(buf).unwrap();
    assert!(text.contains("Enabled by your configuration linters:"));
    assert!(text.contains("Disabled by your configuration linters:"));
    assert!(text.contains("errcheck"));
    assert!(text.contains("govet"));
}

#[test]
fn cli_linters_lists_enabled_and_disabled() {
    let out = Command::new(bin())
        .args(["linters", "--no-config"])
        .output()
        .expect("spawn guff linters");
    assert!(out.status.success(), "stderr={}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Enabled by your configuration linters:"));
    assert!(stdout.contains("Disabled by your configuration linters:"));
    assert!(stdout.contains("errcheck"));
    assert!(stdout.contains("staticcheck"));
    assert!(stdout.contains("nolintlint"));
}

#[test]
fn cli_linters_respects_preset_none_and_enable() {
    let out = Command::new(bin())
        .args([
            "linters",
            "--no-config",
            "--preset",
            "none",
            "--enable",
            "errcheck",
        ])
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Enabled section should mention errcheck before Disabled section.
    let enabled_pos = stdout.find("Enabled by your configuration linters:").unwrap();
    let disabled_pos = stdout.find("Disabled by your configuration linters:").unwrap();
    let enabled_block = &stdout[enabled_pos..disabled_pos];
    let disabled_block = &stdout[disabled_pos..];
    assert!(enabled_block.contains("errcheck"));
    assert!(!enabled_block.contains("staticcheck"));
    assert!(disabled_block.contains("staticcheck"));
}

#[test]
fn parse_default_timeout() {
    let d = parse_go_duration(DEFAULT_TIMEOUT).unwrap();
    assert_eq!(d.as_secs(), 60);
}

#[test]
fn cli_rejects_invalid_timeout() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run/unchecked_error");
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--timeout",
            "notaduration",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn");
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("timeout") || stderr.contains("invalid"),
        "stderr={stderr}"
    );
}

#[test]
fn cli_accepts_timeout_and_concurrency_flags() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/run/unchecked_error");
    let out = Command::new(bin())
        .args([
            "run",
            "--no-config",
            "--enable",
            "errcheck",
            "--timeout",
            "1m",
            "-j",
            "1",
            "--issues-exit-code",
            "0",
            ".",
        ])
        .current_dir(&dir)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "status={:?}\nstdout={}\nstderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!out.stdout.is_empty());
}
