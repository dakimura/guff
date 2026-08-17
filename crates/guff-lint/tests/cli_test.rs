//! R5: `guff version`, `guff linters`, `--timeout`, `-j/--concurrency`.

use std::process::Command;

use guff_lint::{
    format_linters_listing, guff_version, parse_go_duration, partition_linters, version_banner,
    LinterDefault, LinterSelection, DEFAULT_TIMEOUT, GOLANGCI_LINT_COMPAT,
};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_guff")
}

#[test]
fn version_banner_contains_crate_version() {
    let banner = version_banner();
    assert!(banner.contains(guff_version()));
    assert!(banner.contains(GOLANGCI_LINT_COMPAT));
    assert!(banner.starts_with("guff has version "));
    assert!(banner.contains("golangci-lint compat"));
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
    assert!(stdout.contains(GOLANGCI_LINT_COMPAT));
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

#[test]
fn cli_cache_status_and_clean() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cache_dir = tmp.path().join("guff-cache");
    std::fs::create_dir_all(&cache_dir).unwrap();
    std::fs::write(cache_dir.join("README"), b"x").unwrap();

    let status = Command::new(bin())
        .args(["cache", "status"])
        .env("GUFF_CACHE", &cache_dir)
        .output()
        .expect("spawn cache status");
    assert!(
        status.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&status.stderr)
    );
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(stdout.contains("Dir:"));
    assert!(stdout.contains(cache_dir.to_str().unwrap()));
    assert!(stdout.contains("Size:"));
    assert!(stdout.contains("GOCACHE:"));

    let clean = Command::new(bin())
        .args(["cache", "clean"])
        .env("GUFF_CACHE", &cache_dir)
        .output()
        .expect("spawn cache clean");
    assert!(
        clean.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&clean.stderr)
    );
    assert!(!cache_dir.exists());
}

#[test]
fn cli_fmt_rewrites_file() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("main.go");
    std::fs::write(&path, "package main\nfunc main(  ) {\nx:=1\n}\n").unwrap();

    let out = Command::new(bin())
        .args(["fmt", "--no-config", "-E", "gofmt"])
        .arg(&path)
        .output()
        .expect("spawn guff fmt");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(got.contains("func main() {"), "got:\n{got}");
    assert!(got.contains("x := 1"), "got:\n{got}");
}

#[test]
fn cli_fmt_diff_exits_one() {
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("main.go");
    let original = "package main\nfunc main(  ) {}\n";
    std::fs::write(&path, original).unwrap();

    let out = Command::new(bin())
        .args(["fmt", "--no-config", "-E", "gofmt", "-d"])
        .arg(&path)
        .output()
        .expect("spawn guff fmt -d");
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("func main()"), "diff:\n{stdout}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn cli_fmt_reads_gofmt_simplify_from_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - gofmt
  settings:
    gofmt:
      simplify: true
"#,
    )
    .unwrap();
    let path = tmp.path().join("p.go");
    std::fs::write(
        &path,
        "package p\n\nfunc f(s []int) []int {\n\treturn s[1:len(s)]\n}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt -c");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(got.contains("s[1:]"), "expected -s rewrite, got:\n{got}");
}

#[test]
fn cli_fmt_gofumpt_extra_rules_from_config() {
    if Command::new("gofumpt")
        .arg("-version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skip: gofumpt not on PATH");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - gofumpt
  settings:
    gofumpt:
      extra-rules: true
"#,
    )
    .unwrap();
    let path = tmp.path().join("p.go");
    std::fs::write(
        &path,
        "package p\n\nfunc f() (x int) {\n\tx = 1\n\treturn\n}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt gofumpt");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("return x"),
        "expected clothed return from -extra, got:\n{got}"
    );
}

#[test]
fn cli_fmt_goimports_local_prefixes_from_config() {
    if Command::new("goimports")
        .arg("-h")
        .output()
        .map(|o| !(o.status.success() || !o.stderr.is_empty()))
        .unwrap_or(true)
    {
        eprintln!("skip: goimports not on PATH");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - goimports
  settings:
    goimports:
      local-prefixes:
        - github.com/org/project
"#,
    )
    .unwrap();
    let path = tmp.path().join("p.go");
    std::fs::write(
        &path,
        "package p\n\nimport (\n\t\"github.com/org/project/pkg\"\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n\t_ = pkg.Y\n}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt goimports");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    let fmt_pos = got.find("\"fmt\"").expect("fmt");
    let bar_pos = got.find("\"github.com/foo/bar\"").expect("bar");
    let pkg_pos = got.find("\"github.com/org/project/pkg\"").expect("pkg");
    assert!(
        fmt_pos < bar_pos && bar_pos < pkg_pos,
        "expected stdlib < third-party < local, got:\n{got}"
    );
}

#[test]
fn cli_fmt_gci_sections_from_config() {
    if Command::new("gci")
        .args(["print", "--help"])
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skip: gci not on PATH");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - gci
  settings:
    gci:
      custom-order: true
      sections:
        - standard
        - default
        - prefix(github.com/org/project)
"#,
    )
    .unwrap();
    let path = tmp.path().join("p.go");
    std::fs::write(
        &path,
        "package p\n\nimport (\n\t\"github.com/org/project/pkg\"\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n\t_ = pkg.Y\n}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt gci");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    let fmt_pos = got.find("\"fmt\"").expect("fmt");
    let bar_pos = got.find("\"github.com/foo/bar\"").expect("bar");
    let pkg_pos = got.find("\"github.com/org/project/pkg\"").expect("pkg");
    assert!(
        fmt_pos < bar_pos && bar_pos < pkg_pos,
        "expected standard < default < prefix, got:\n{got}"
    );
}

#[test]
fn cli_fmt_golines_max_len_from_config() {
    if Command::new("golines")
        .arg("--version")
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(true)
    {
        eprintln!("skip: golines not on PATH");
        return;
    }
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - golines
  settings:
    golines:
      max-len: 60
      reformat-tags: false
"#,
    )
    .unwrap();
    let path = tmp.path().join("p.go");
    std::fs::write(
        &path,
        "package p\n\nfunc f() {\n\tfoo(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)\n}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt golines");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("foo(\n") || got.lines().count() > 5,
        "expected golines wrap from max-len: 60, got:\n{got}"
    );
}

#[test]
fn cli_fmt_diff_output_is_plain_without_tty() {
    // Piped stdout (non-TTY) must not contain ANSI escapes even without --no-color.
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("main.go");
    std::fs::write(&path, "package main\nfunc main(  ) {}\n").unwrap();

    let out = Command::new(bin())
        .args(["fmt", "--no-config", "-E", "gofmt", "-d"])
        .arg(&path)
        .output()
        .expect("spawn guff fmt -d");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains('\x1b'), "unexpected ANSI in:\n{stdout}");
    assert!(stdout.contains("func main()"), "diff:\n{stdout}");
}

#[test]
fn cli_fmt_swaggo_enable_is_accepted() {
    // swaggo is now a known/implemented formatter; enabling it must not error.
    // (If `swag` is not installed, the file is left unchanged and exit is 0.)
    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("main.go");
    std::fs::write(&path, "package main\n\nfunc main() {}\n").unwrap();

    let out = Command::new(bin())
        .args(["fmt", "--no-config", "-E", "swaggo"])
        .arg(&path)
        .output()
        .expect("spawn guff fmt -E swaggo");
    assert!(
        out.status.success(),
        "swaggo enable should not error; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn cli_fmt_generated_disable_from_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - gofmt
  exclusions:
    generated: disable
"#,
    )
    .unwrap();
    let path = tmp.path().join("gen.go");
    std::fs::write(
        &path,
        "// Code generated by tool. DO NOT EDIT.\npackage p\nfunc f(  ) {}\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt generated:disable");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("func f() {}"),
        "expected formatting of generated file when disable, got:\n{got}"
    );
}

#[test]
fn cli_fmt_generated_strict_formats_lax_only_marker() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        r#"
version: "2"
formatters:
  enable:
    - gofmt
  exclusions:
    generated: strict
"#,
    )
    .unwrap();
    let path = tmp.path().join("gen.go");
    // Lax-only marker — must still be formatted under strict.
    std::fs::write(&path, "// DO NOT EDIT\npackage p\nfunc f(  ) {}\n").unwrap();

    let out = Command::new(bin())
        .args(["fmt", "-c"])
        .arg(&cfg)
        .arg(&path)
        .output()
        .expect("spawn guff fmt generated:strict");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&path).unwrap();
    assert!(
        got.contains("func f() {}"),
        "expected format under strict for non-convention marker, got:\n{got}"
    );
}


/// A config that selects nothing must not quietly run the standard preset.
///
/// `linters.default: none` with no `enable`, and `default: standard` with every
/// standard linter under `disable`, both used to fall through to
/// `STANDARD_LINTER_NAMES` — so "disable everything" ran everything.
/// golangci-lint answers `Running error: no linters enabled` and exits 3.
#[test]
fn cli_run_with_nothing_selected_exits_three() {
    for cfg_body in [
        "version: \"2\"\nlinters:\n  default: none\n",
        "version: \"2\"\nlinters:\n  default: standard\n  disable: [errcheck, govet, ineffassign, staticcheck, unused]\n",
    ] {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("go.mod"), "module example.com\n\ngo 1.24\n").unwrap();
        std::fs::write(
            tmp.path().join("a.go"),
            "package a\n\nfunc unusedFn() int { return 1 }\n",
        )
        .unwrap();
        let cfg = tmp.path().join(".golangci.yml");
        std::fs::write(&cfg, cfg_body).unwrap();

        let out = Command::new(bin())
            .args(["run", "-c"])
            .arg(&cfg)
            .arg("./...")
            .current_dir(tmp.path())
            .output()
            .expect("spawn guff run");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(guff_lint::EXIT_NO_LINTERS),
            "config:\n{cfg_body}\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            stdout.trim().is_empty(),
            "nothing was enabled, so nothing should be reported:\n{stdout}"
        );
    }
}

/// …but a format-only config is legal, and `linters.default: none` is exactly
/// how you write one. Enabled formatters keep the run alive.
#[test]
fn cli_run_with_only_formatters_enabled_still_formats() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com\n\ngo 1.24\n").unwrap();
    std::fs::write(tmp.path().join("a.go"), "package a\n\nfunc f(  ) {}\n").unwrap();
    let cfg = tmp.path().join(".golangci.yml");
    std::fs::write(
        &cfg,
        "version: \"2\"\nlinters:\n  default: none\nformatters:\n  enable:\n    - gofmt\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["run", "-c"])
        .arg(&cfg)
        .arg("./...")
        .current_dir(tmp.path())
        .output()
        .expect("spawn guff run");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(1),
        "stdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("gofmt"), "stdout:\n{stdout}");
}

/// `-E` / `-D` are golangci-lint's shorthands for `--enable` / `--disable`, and
/// `guff fmt` already had `-E`. Without them on `run`, pasting a
/// `golangci-lint run -E gosec` command line at guff fails to parse.
#[test]
fn cli_run_accepts_short_enable_and_disable() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com\n\ngo 1.24\n").unwrap();
    std::fs::write(
        tmp.path().join("a.go"),
        "package a\n\nfunc unusedFn() int { return 1 }\n",
    )
    .unwrap();

    // `--default none -E errcheck` runs errcheck alone: nothing to report.
    let out = Command::new(bin())
        .args(["run", "--no-config", "--default", "none", "-E", "errcheck", "./..."])
        .current_dir(tmp.path())
        .output()
        .expect("spawn guff run -E");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // `-D unused` takes the one standard linter this file trips back out.
    let out = Command::new(bin())
        .args(["run", "--no-config", "-D", "unused", "./..."])
        .current_dir(tmp.path())
        .output()
        .expect("spawn guff run -D");
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// A linter name guff cannot resolve must stop the run, not warn and continue.
///
/// Regression test for the 2026-08-17 field report (issue G). guff used to
/// print `linter "x" is not available yet` and lint on with whatever was left,
/// so a typo — or a linter someone assumed was wired — turned into "silently
/// not checked": exit 0, no findings, indistinguishable from a clean tree.
/// golangci-lint validates the enable set first and exits 3
/// (`unknown linters: 'x', run 'golangci-lint help linters' …`).
///
/// The finding-set tiers cannot express this: a tool that skips a linter and a
/// tool that has nothing to report both print nothing.
#[test]
fn cli_run_rejects_unknown_linter_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com\n\ngo 1.24\n").unwrap();
    std::fs::write(tmp.path().join("a.go"), "package a\n").unwrap();

    let out = Command::new(bin())
        .args(["run", "--no-config", "-E", "nosuchlinter", "./..."])
        .current_dir(tmp.path())
        .output()
        .expect("spawn guff run -E nosuchlinter");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(guff_lint::EXIT_CONFIG_ERROR),
        "guff must refuse to start, like golangci-lint\nstdout={}\nstderr={stderr}",
        String::from_utf8_lossy(&out.stdout),
    );
    assert!(
        stderr.contains("unknown linters: 'nosuchlinter'"),
        "the message must name the offending linter; got:\n{stderr}"
    );
    assert!(
        stderr.contains("guff linters"),
        "the message must point at guff's own listing command; got:\n{stderr}"
    );
}

/// Same, from a config file rather than `-E` — `linters.enable` is where a
/// stale name actually shows up in the wild.
#[test]
fn cli_run_rejects_unknown_linter_from_config() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("go.mod"), "module example.com\n\ngo 1.24\n").unwrap();
    std::fs::write(tmp.path().join("a.go"), "package a\n").unwrap();
    std::fs::write(
        tmp.path().join(".golangci.yml"),
        "version: \"2\"\nlinters:\n  default: none\n  enable:\n    - errcheck\n    - nosuchlinter\n",
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["run", "./..."])
        .current_dir(tmp.path())
        .output()
        .expect("spawn guff run");
    assert_eq!(
        out.status.code(),
        Some(guff_lint::EXIT_CONFIG_ERROR),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
}

/// Failing closed is only safe if guff resolves every name golangci-lint
/// accepts. `compat/isolate/linters.txt` is the in-repo roster of those names
/// (it carries one isolate target per golangci-lint v2 linter), so anything
/// listed there must resolve to at least one analyzer — otherwise the check
/// above would lock users out of a linter upstream runs happily.
#[test]
fn every_isolate_linter_name_resolves() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compat/isolate/linters.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    let mut missing = Vec::new();
    let mut seen = 0;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let name = line.split_whitespace().next().expect("non-empty line");
        seen += 1;
        if guff_lint::is_meta_linter(name) {
            // nolintlint owns no analyzer of its own; the run wires it up from
            // the `//nolint` comments the issue filter already parses.
            continue;
        }
        if guff_fmt::is_formatter(name) {
            // `golines` / `swaggo` have isolate targets but are formatters:
            // `guff fmt` / `formatters.enable` own them, and upstream refuses
            // them under `linters.enable` too (compat/reject/cases/
            // formatter-under-linters).
            continue;
        }
        if guff_lint::analyzers_for_linter(name).is_none() {
            missing.push(name.to_string());
        }
    }

    assert!(seen >= 110, "isolate roster looks truncated: {seen} names");
    assert!(
        missing.is_empty(),
        "these linter names are in compat/isolate/linters.txt but resolve to \
         nothing, so `guff run` now refuses configs golangci-lint accepts: {missing:?}"
    );
}
