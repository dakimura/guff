//! Issue output formatters (golangci `pkg/printers` equivalent).
//!
//! R6: [`Formatter`] + text. R7: JSON. R8: colored / checkstyle / sarif / tab /
//! github-actions. `format:path` / config `path` write to files (golangci
//! `createWriter`).

mod checkstyle;
mod color;
mod github;
mod json;
mod sarif;
mod severity;
mod tab;
mod text;

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::exclude::Issue;

pub use checkstyle::CheckstyleFormatter;
pub use github::GithubActionsFormatter;
pub use json::{JsonFormatter, JsonReport, JsonWarning};
pub use sarif::SarifFormatter;
pub use tab::TabFormatter;
pub use text::{format_diagnostic_text, format_issue_text, TextFormatter};

/// Prints a slice of issues to a writer.
pub trait Formatter {
    fn name(&self) -> &'static str;
    fn print(&self, issues: &[Issue], w: &mut dyn Write) -> io::Result<()>;
}

/// golangci `output.print-issued-lines` / `output.print-linter-name` (and CLI equivalents).
///
/// Defaults match golangci: both `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrinterOptions {
    /// Print the source line and a `^` caret under the column (text / colored-line-number).
    pub print_issued_lines: bool,
    /// Append ` (linter)` / include the linter column (text / tab formats).
    pub print_linter_name: bool,
}

impl Default for PrinterOptions {
    fn default() -> Self {
        Self {
            print_issued_lines: true,
            print_linter_name: true,
        }
    }
}

impl PrinterOptions {
    /// Build from optional config keys (`None` → golangci default `true`).
    pub fn from_config(print_issued_lines: Option<bool>, print_linter_name: Option<bool>) -> Self {
        Self {
            print_issued_lines: print_issued_lines.unwrap_or(true),
            print_linter_name: print_linter_name.unwrap_or(true),
        }
    }
}

/// Supported `--out-format` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutputFormatKind {
    /// Plain `file:line:col: message (analyzer)` (also `line-number`).
    Text,
    /// Colored text + source line/caret when available (`colored-line-number`).
    Colored,
    /// golangci-lint JSON schema (`{"Issues":[...],"Report":...}`).
    Json,
    /// Checkstyle XML (`version="5.0"`).
    Checkstyle,
    /// SARIF 2.1.0 JSON.
    Sarif,
    /// Tab-aligned columns.
    Tab,
    /// Tab + colors (`colored-tab`).
    ColoredTab,
    /// GitHub Actions workflow commands (`::error file=…`).
    GithubActions,
}

/// One output destination: a format plus optional path.
///
/// Path semantics (golangci `createWriter`):
/// - [`None`] / empty / `"stdout"` → write to the shared writer (usually stdout)
/// - `"stderr"` → write to stderr
/// - otherwise → create parent dirs and write to that file (mode `0o644`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputSpec {
    pub kind: OutputFormatKind,
    pub path: Option<String>,
}

impl OutputSpec {
    pub fn new(kind: OutputFormatKind) -> Self {
        Self { kind, path: None }
    }

    pub fn with_path(kind: OutputFormatKind, path: impl Into<String>) -> Self {
        let path = path.into();
        let path = match path.as_str() {
            "" | "stdout" => None,
            other => Some(other.to_string()),
        };
        Self { kind, path }
    }

    /// Parse `name` or `name:path` (first `:` separates format from path).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let (name, path) = match raw.split_once(':') {
            Some((n, p)) => (n, Some(p.to_string())),
            None => (raw, None),
        };
        let kind = OutputFormatKind::parse(name)?;
        Ok(Self::with_path(kind, path.unwrap_or_default()))
    }
}

impl From<OutputFormatKind> for OutputSpec {
    fn from(kind: OutputFormatKind) -> Self {
        Self::new(kind)
    }
}

impl OutputFormatKind {
    /// Parse a single format name (guff + golangci aliases).
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "text" | "line-number" => Ok(Self::Text),
            "colored-line-number" | "colored" => Ok(Self::Colored),
            "json" => Ok(Self::Json),
            "checkstyle" => Ok(Self::Checkstyle),
            "sarif" => Ok(Self::Sarif),
            "tab" => Ok(Self::Tab),
            "colored-tab" => Ok(Self::ColoredTab),
            "github-actions" | "github" => Ok(Self::GithubActions),
            other => Err(format!(
                "unknown output format {other:?}; supported: text, line-number, \
                 colored-line-number, json, checkstyle, sarif, tab, colored-tab, \
                 github-actions"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Colored => "colored-line-number",
            Self::Json => "json",
            Self::Checkstyle => "checkstyle",
            Self::Sarif => "sarif",
            Self::Tab => "tab",
            Self::ColoredTab => "colored-tab",
            Self::GithubActions => "github-actions",
        }
    }

    pub fn formatter(self) -> Box<dyn Formatter> {
        self.formatter_with(&PrinterOptions::default())
    }

    /// Build a formatter applying golangci `output.print-*` options.
    pub fn formatter_with(self, opts: &PrinterOptions) -> Box<dyn Formatter> {
        match self {
            Self::Text => Box::new(TextFormatter::new().with_printer_options(opts)),
            Self::Colored => Box::new(TextFormatter::colored().with_printer_options(opts)),
            Self::Json => Box::new(JsonFormatter::new()),
            Self::Checkstyle => Box::new(CheckstyleFormatter::new()),
            Self::Sarif => Box::new(SarifFormatter::new()),
            Self::Tab => Box::new(TabFormatter::new().with_printer_options(opts)),
            Self::ColoredTab => Box::new(TabFormatter::colored().with_printer_options(opts)),
            Self::GithubActions => Box::new(GithubActionsFormatter::new()),
        }
    }
}

/// Resolve CLI `--out-format` values (repeatable). Empty → `[Text]` on stdout.
///
/// Accepts `format` or `format:path` (golangci-compatible). Paths other than
/// `stdout`/`stderr` are written to disk by [`print_issues`].
///
/// Callers that want golangci's interactive default (`colored-line-number` on a
/// TTY) should pass [`default_stdout_format`] instead of relying on empty CLI.
pub fn resolve_out_formats(cli: &[String]) -> Result<Vec<OutputSpec>, String> {
    if cli.is_empty() {
        return Ok(vec![OutputSpec::new(OutputFormatKind::Text)]);
    }
    let mut out = Vec::with_capacity(cli.len());
    for raw in cli {
        out.push(OutputSpec::parse(raw)?);
    }
    Ok(out)
}

/// Implicit default when neither CLI nor config selects a format.
///
/// Matches golangci-lint: `colored-line-number` on a TTY, plain `text` otherwise
/// (so pipes/CI stay free of ANSI).
pub fn default_stdout_format(stdout_is_tty: bool) -> OutputSpec {
    if stdout_is_tty {
        OutputSpec::new(OutputFormatKind::Colored)
    } else {
        OutputSpec::new(OutputFormatKind::Text)
    }
}

/// Print `issues` for each selected format.
///
/// Specs without a path (or with `stdout`) use `default_out`. File paths create
/// parent directories as needed (golangci `MkdirAll` + `OpenFile` O_TRUNC).
pub fn print_issues(
    formats: &[OutputSpec],
    issues: &[Issue],
    default_out: &mut dyn Write,
) -> io::Result<usize> {
    print_issues_with(formats, &PrinterOptions::default(), issues, default_out)
}

/// Like [`print_issues`], applying golangci `output.print-*` options.
pub fn print_issues_with(
    formats: &[OutputSpec],
    opts: &PrinterOptions,
    issues: &[Issue],
    default_out: &mut dyn Write,
) -> io::Result<usize> {
    if formats.is_empty() {
        TextFormatter::new()
            .with_printer_options(opts)
            .print(issues, default_out)?;
        return Ok(issues.len());
    }
    for spec in formats {
        write_spec(spec, opts, issues, default_out)?;
    }
    Ok(issues.len())
}

fn write_spec(
    spec: &OutputSpec,
    opts: &PrinterOptions,
    issues: &[Issue],
    default_out: &mut dyn Write,
) -> io::Result<()> {
    let formatter = spec.kind.formatter_with(opts);
    match spec.path.as_deref() {
        None | Some("stdout") => formatter.print(issues, default_out),
        Some("stderr") => {
            let mut err = io::stderr().lock();
            formatter.print(issues, &mut err)
        }
        Some(path) => {
            let p = Path::new(path);
            if let Some(parent) = p.parent() {
                if !parent.as_os_str().is_empty() {
                    fs::create_dir_all(parent)?;
                }
            }
            let mut f = fs::File::create(p)?;
            formatter.print(issues, &mut f)
        }
    }
}

/// Best-effort parse of `output.formats` / deprecated `output.format`.
///
/// Returns an empty list when neither key selects a format; the CLI then applies
/// [`default_stdout_format`] (TTY → colored-line-number, else text).
///
/// Unknown formats are skipped (caller may log). Supported shapes:
/// - string: `json` / `json:path` / comma-separated
/// - sequence of strings or `{format, path}` maps
/// - golangci v2 map: `{ json: { path: ... }, text: { path: stdout } }`
pub fn formats_from_output_config(
    formats: &serde_yaml::Value,
    legacy_format: Option<&str>,
) -> Vec<OutputSpec> {
    let mut specs: Vec<OutputSpec> = Vec::new();

    let push_raw = |specs: &mut Vec<OutputSpec>, raw: &str| {
        let raw = raw.trim();
        if raw.is_empty() {
            return;
        }
        match OutputSpec::parse(raw) {
            Ok(spec) => {
                if !specs.iter().any(|s| s.kind == spec.kind && s.path == spec.path) {
                    specs.push(spec);
                }
            }
            Err(e) => {
                let name = raw.split_once(':').map(|(n, _)| n).unwrap_or(raw);
                eprintln!("guff: ignoring output format {name:?}: {e}");
            }
        }
    };

    let push_kind_path = |specs: &mut Vec<OutputSpec>, name: &str, path: Option<&str>| {
        match OutputFormatKind::parse(name) {
            Ok(kind) => {
                let spec = match path {
                    Some(p) => OutputSpec::with_path(kind, p),
                    None => OutputSpec::new(kind),
                };
                if !specs.iter().any(|s| s.kind == spec.kind && s.path == spec.path) {
                    specs.push(spec);
                }
            }
            Err(e) => {
                eprintln!("guff: ignoring output format {name:?}: {e}");
            }
        }
    };

    if let Some(legacy) = legacy_format {
        if !legacy.is_empty() {
            for part in legacy.split(',') {
                push_raw(&mut specs, part);
            }
        }
    }

    match formats {
        serde_yaml::Value::Null => {}
        serde_yaml::Value::String(s) => {
            for part in s.split(',') {
                push_raw(&mut specs, part);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                match item {
                    serde_yaml::Value::String(s) => push_raw(&mut specs, s),
                    serde_yaml::Value::Mapping(m) => {
                        let format = m
                            .get(serde_yaml::Value::String("format".into()))
                            .and_then(|v| v.as_str());
                        let path = m
                            .get(serde_yaml::Value::String("path".into()))
                            .and_then(|v| v.as_str());
                        if let Some(f) = format {
                            push_kind_path(&mut specs, f, path);
                        }
                    }
                    _ => {}
                }
            }
        }
        serde_yaml::Value::Mapping(m) => {
            for (k, v) in m {
                let Some(name) = k.as_str() else {
                    continue;
                };
                let path = match v {
                    serde_yaml::Value::Mapping(inner) => inner
                        .get(serde_yaml::Value::String("path".into()))
                        .and_then(|p| p.as_str()),
                    serde_yaml::Value::String(p) => Some(p.as_str()),
                    serde_yaml::Value::Null => None,
                    _ => None,
                };
                push_kind_path(&mut specs, name, path);
            }
        }
        _ => {}
    }

    if specs.is_empty() {
        // No formats in config — caller applies [`default_stdout_format`] / Text.
        Vec::new()
    } else {
        specs
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;
    use std::fs;
    use tempfile::tempdir;

    fn sample_issue() -> Issue {
        Issue {
            from_linter: "errcheck".into(),
            analyzer: "errcheck".into(),
            text: "unchecked error".into(),
            severity: String::new(),
            filename: "bad.go".into(),
            line: 5,
            column: 2,
            source_line: None,
            diagnostic: Diagnostic {
                message: "unchecked error".into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn parse_text_aliases() {
        assert_eq!(OutputFormatKind::parse("text").unwrap(), OutputFormatKind::Text);
        assert_eq!(
            OutputFormatKind::parse("line-number").unwrap(),
            OutputFormatKind::Text
        );
        assert_eq!(
            OutputFormatKind::parse("colored-line-number").unwrap(),
            OutputFormatKind::Colored
        );
    }

    #[test]
    fn parse_r8_formats() {
        assert_eq!(OutputFormatKind::parse("json").unwrap(), OutputFormatKind::Json);
        assert_eq!(
            OutputFormatKind::parse("checkstyle").unwrap(),
            OutputFormatKind::Checkstyle
        );
        assert_eq!(OutputFormatKind::parse("sarif").unwrap(), OutputFormatKind::Sarif);
        assert_eq!(OutputFormatKind::parse("tab").unwrap(), OutputFormatKind::Tab);
        assert_eq!(
            OutputFormatKind::parse("colored-tab").unwrap(),
            OutputFormatKind::ColoredTab
        );
        assert_eq!(
            OutputFormatKind::parse("github-actions").unwrap(),
            OutputFormatKind::GithubActions
        );
    }

    #[test]
    fn resolve_default_is_text() {
        assert_eq!(
            resolve_out_formats(&[]).unwrap(),
            vec![OutputSpec::new(OutputFormatKind::Text)]
        );
    }

    #[test]
    fn default_stdout_format_matches_golangci_tty_rule() {
        assert_eq!(
            default_stdout_format(true),
            OutputSpec::new(OutputFormatKind::Colored)
        );
        assert_eq!(
            default_stdout_format(false),
            OutputSpec::new(OutputFormatKind::Text)
        );
    }

    #[test]
    fn formats_from_config_null_is_empty() {
        assert!(formats_from_output_config(&serde_yaml::Value::Null, None).is_empty());
    }

    #[test]
    fn print_issues_with_respects_printer_options() {
        let mut issue = sample_issue();
        issue.source_line = Some("ab".into());
        issue.column = 2;
        let opts = PrinterOptions {
            print_issued_lines: true,
            print_linter_name: true,
        };
        let mut buf = Vec::new();
        print_issues_with(
            &[OutputSpec::new(OutputFormatKind::Text)],
            &opts,
            &[issue.clone()],
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.contains("ab\n"), "{s:?}");
        assert!(s.contains(" ^\n") || s.ends_with("^\n"), "{s:?}");

        let opts_off = PrinterOptions {
            print_issued_lines: false,
            print_linter_name: false,
        };
        let mut buf = Vec::new();
        print_issues_with(
            &[OutputSpec::new(OutputFormatKind::Text)],
            &opts_off,
            &[issue],
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "bad.go:5:2: unchecked error\n");
    }

    #[test]
    fn resolve_keeps_path_suffix() {
        assert_eq!(
            resolve_out_formats(&["text:/tmp/out.txt".into()]).unwrap(),
            vec![OutputSpec::with_path(OutputFormatKind::Text, "/tmp/out.txt")]
        );
        assert_eq!(
            resolve_out_formats(&["json:/tmp/out.json".into()]).unwrap(),
            vec![OutputSpec::with_path(OutputFormatKind::Json, "/tmp/out.json")]
        );
        assert_eq!(
            resolve_out_formats(&["checkstyle:/tmp/cs.xml".into()]).unwrap(),
            vec![OutputSpec::with_path(
                OutputFormatKind::Checkstyle,
                "/tmp/cs.xml"
            )]
        );
        assert_eq!(
            resolve_out_formats(&["json:stdout".into()]).unwrap(),
            vec![OutputSpec::new(OutputFormatKind::Json)]
        );
    }

    #[test]
    fn text_formatter_matches_legacy_line() {
        let mut buf = Vec::new();
        TextFormatter::new()
            .print(&[sample_issue()], &mut buf)
            .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(s, "bad.go:5:2: unchecked error (errcheck)\n");
    }

    #[test]
    fn json_formatter_via_print_issues() {
        let mut buf = Vec::new();
        print_issues(
            &[OutputSpec::new(OutputFormatKind::Json)],
            &[sample_issue()],
            &mut buf,
        )
        .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
        assert_eq!(v["Issues"][0]["FromLinter"], "errcheck");
        assert_eq!(v["Issues"][0]["Text"], "unchecked error");
        assert!(v["Report"].is_null());
    }

    #[test]
    fn github_actions_via_print_issues() {
        let mut buf = Vec::new();
        print_issues(
            &[OutputSpec::new(OutputFormatKind::GithubActions)],
            &[sample_issue()],
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert_eq!(
            s,
            "::error file=bad.go,line=5,col=2::unchecked error (errcheck)\n"
        );
    }

    #[test]
    fn print_issues_writes_format_path_to_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested").join("out.json");
        let path_str = path.to_string_lossy().into_owned();
        let mut stdout_buf = Vec::new();
        print_issues(
            &[OutputSpec::with_path(OutputFormatKind::Json, path_str)],
            &[sample_issue()],
            &mut stdout_buf,
        )
        .unwrap();
        assert!(
            stdout_buf.is_empty(),
            "file destination must not also write to default writer"
        );
        let contents = fs::read_to_string(&path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&contents).unwrap();
        assert_eq!(v["Issues"][0]["FromLinter"], "errcheck");
    }

    #[test]
    fn print_issues_can_split_stdout_and_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("issues.json");
        let mut stdout_buf = Vec::new();
        print_issues(
            &[
                OutputSpec::new(OutputFormatKind::Text),
                OutputSpec::with_path(
                    OutputFormatKind::Json,
                    path.to_string_lossy().into_owned(),
                ),
            ],
            &[sample_issue()],
            &mut stdout_buf,
        )
        .unwrap();
        let text = String::from_utf8(stdout_buf).unwrap();
        assert!(text.contains("unchecked error"));
        let v: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(v["Issues"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn formats_from_config_v2_map_keeps_paths() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
json:
  path: report.json
text:
  path: stdout
"#,
        )
        .unwrap();
        let specs = formats_from_output_config(&yaml, None);
        assert!(specs.iter().any(|s| {
            s.kind == OutputFormatKind::Json && s.path.as_deref() == Some("report.json")
        }));
        assert!(specs
            .iter()
            .any(|s| s.kind == OutputFormatKind::Text && s.path.is_none()));
    }

    #[test]
    fn formats_from_config_sequence_with_path() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(
            r#"
- format: checkstyle
  path: cs.xml
- json:out.json
"#,
        )
        .unwrap();
        let specs = formats_from_output_config(&yaml, None);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].kind, OutputFormatKind::Checkstyle);
        assert_eq!(specs[0].path.as_deref(), Some("cs.xml"));
        assert_eq!(specs[1].kind, OutputFormatKind::Json);
        assert_eq!(specs[1].path.as_deref(), Some("out.json"));
    }
}
