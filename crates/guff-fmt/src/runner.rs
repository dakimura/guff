//! File-tree walker that applies a [`MetaFormatter`] (golangci `pkg/goformat`).

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use similar::TextDiff;

use crate::generated::{is_generated, GeneratedMode};
use crate::meta::MetaFormatter;

/// Errors from formatting / walking.
#[derive(Debug)]
pub enum FormatError {
    Io {
        formatter: String,
        path: String,
        source: io::Error,
    },
    Message {
        formatter: String,
        path: String,
        message: String,
    },
    InvalidFormatter(String),
    /// Formatter name is known but not yet implemented in guff.
    Deferred(String),
    Walk(io::Error),
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io {
                formatter,
                path,
                source,
            } => write!(f, "{formatter}: {path}: {source}"),
            Self::Message {
                formatter,
                path,
                message,
            } => write!(f, "{formatter}: {path}: {message}"),
            Self::InvalidFormatter(name) => write!(f, "invalid formatter {name:?}"),
            Self::Deferred(name) => {
                write!(
                    f,
                    "formatter {name:?} is not implemented yet (DEFERRED → R15)"
                )
            }
            Self::Walk(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FormatError {}

/// Options for [`Runner`].
#[derive(Debug, Clone, Default)]
pub struct RunnerOptions {
    /// Print unified diffs instead of rewriting files.
    pub diff: bool,
    /// Read one file from stdin; write formatted result to stdout.
    pub stdin: bool,
    /// Path exclusion regexes (relative path matched). Empty = none.
    pub exclude_paths: Vec<String>,
    /// `formatters.exclusions.generated` (`lax` default, matching golangci fmt).
    pub generated: GeneratedMode,
    /// Colorize `--diff` output with ANSI codes (default: off).
    pub color: bool,
}

/// A single formatting difference found by [`Runner::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatFinding {
    /// File that is not properly formatted.
    pub file: String,
    /// 1-based line number of the change (start of the first differing hunk).
    pub line: i64,
}

/// Summary of a format run.
#[derive(Debug, Clone, Default)]
pub struct RunStats {
    pub rewritten: usize,
    pub unchanged: usize,
    pub skipped: usize,
    /// Non-zero when `--diff` found differences (golangci exit 1).
    pub exit_code: i32,
}

/// Applies a meta-formatter across paths.
pub struct Runner {
    meta: MetaFormatter,
    opts: RunnerOptions,
}

impl Runner {
    pub fn new(meta: MetaFormatter, opts: RunnerOptions) -> Self {
        Self { meta, opts }
    }

    pub fn run(&self, paths: &[PathBuf], stdout: &mut dyn Write) -> Result<RunStats, FormatError> {
        let mut stats = RunStats::default();

        if self.opts.stdin {
            let mut input = Vec::new();
            io::stdin()
                .read_to_end(&mut input)
                .map_err(FormatError::Walk)?;
            let output = self.meta.format("<standard input>", &input)?;
            stdout.write_all(&output).map_err(FormatError::Walk)?;
            return Ok(stats);
        }

        let roots: Vec<PathBuf> = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.to_vec()
        };

        for root in roots {
            self.walk(&root, stdout, &mut stats)?;
        }

        Ok(stats)
    }

    /// Report files that are not properly formatted, without rewriting anything.
    ///
    /// Used by `guff run` to surface formatter diagnostics. Honors the same
    /// generated-file and path exclusions as [`run`](Self::run).
    ///
    /// Each file is formatted independently (gofumpt/goimports/… spawn a
    /// subprocess per file), so the per-file checks run in parallel across the
    /// rayon pool — on large trees this dominated wall time when serial. Paths
    /// are gathered first, then checked concurrently and the findings sorted by
    /// `(file, line)` for a deterministic result regardless of scheduling.
    pub fn check(&self, paths: &[PathBuf]) -> Result<Vec<FormatFinding>, FormatError> {
        let roots: Vec<PathBuf> = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.to_vec()
        };
        let mut files = Vec::new();
        for root in &roots {
            collect_go_files(root, &mut files)?;
        }
        let mut findings: Vec<FormatFinding> = files
            .par_iter()
            .map(|path| self.check_file(path))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        Ok(findings)
    }

    fn check_file(&self, path: &Path) -> Result<Vec<FormatFinding>, FormatError> {
        let path_str = path.to_string_lossy();
        if self.is_excluded(path) {
            return Ok(Vec::new());
        }
        let input = fs::read(path).map_err(|e| FormatError::Io {
            formatter: "guff-fmt".into(),
            path: path_str.to_string(),
            source: e,
        })?;
        if is_generated(&input, self.opts.generated) {
            return Ok(Vec::new());
        }
        let output = self.meta.format(&path_str, &input)?;
        if output == input {
            return Ok(Vec::new());
        }
        let old = String::from_utf8_lossy(&input);
        let new = String::from_utf8_lossy(&output);
        let diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
        Ok(first_changed_lines(&diff)
            .into_iter()
            .map(|line| FormatFinding {
                file: path_str.to_string(),
                line,
            })
            .collect())
    }

    fn walk(
        &self,
        root: &Path,
        stdout: &mut dyn Write,
        stats: &mut RunStats,
    ) -> Result<(), FormatError> {
        let meta = fs::metadata(root).map_err(FormatError::Walk)?;
        if meta.is_file() {
            if is_go_path(root) {
                self.process(root, stdout, stats)?;
            }
            return Ok(());
        }

        let entries = fs::read_dir(root).map_err(FormatError::Walk)?;
        for entry in entries {
            let entry = entry.map_err(FormatError::Walk)?;
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let ft = entry.file_type().map_err(FormatError::Walk)?;
            if ft.is_dir() {
                if skip_dir(&name) {
                    continue;
                }
                self.walk(&path, stdout, stats)?;
            } else if ft.is_file() && is_go_path(&path) {
                self.process(&path, stdout, stats)?;
            }
        }
        Ok(())
    }

    fn process(
        &self,
        path: &Path,
        stdout: &mut dyn Write,
        stats: &mut RunStats,
    ) -> Result<(), FormatError> {
        let path_str = path.to_string_lossy();
        if self.is_excluded(path) {
            stats.skipped += 1;
            return Ok(());
        }

        let input = fs::read(path).map_err(|e| FormatError::Io {
            formatter: "guff-fmt".into(),
            path: path_str.to_string(),
            source: e,
        })?;

        // Skip generated files (`formatters.exclusions.generated`).
        if is_generated(&input, self.opts.generated) {
            stats.skipped += 1;
            return Ok(());
        }

        let output = self.meta.format(&path_str, &input)?;
        if output == input {
            stats.unchanged += 1;
            return Ok(());
        }

        if self.opts.diff {
            let old = String::from_utf8_lossy(&input);
            let new = String::from_utf8_lossy(&output);
            let display = path_str.replace('\\', "/");
            let diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
            let unified = diff
                .unified_diff()
                .context_radius(3)
                .header(&format!("{display}.orig"), &display)
                .to_string();
            let rendered = if self.opts.color {
                colorize_diff(&unified)
            } else {
                unified
            };
            stdout
                .write_all(rendered.as_bytes())
                .map_err(FormatError::Walk)?;
            stats.exit_code = 1;
            stats.rewritten += 1;
            return Ok(());
        }

        fs::write(path, &output).map_err(|e| FormatError::Io {
            formatter: "guff-fmt".into(),
            path: path_str.to_string(),
            source: e,
        })?;
        stats.rewritten += 1;
        Ok(())
    }

    fn is_excluded(&self, path: &Path) -> bool {
        if self.opts.exclude_paths.is_empty() {
            return false;
        }
        let s = path.to_string_lossy().replace('\\', "/");
        self.opts
            .exclude_paths
            .iter()
            .any(|pat| path_matches(&s, pat))
    }
}

/// Recursively gather formatter-eligible `.go` files under `root`, applying the
/// same directory skips as the in-place [`walk`](Runner::walk). Per-file
/// exclusion / generated checks stay in [`Runner::check_file`].
fn collect_go_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<(), FormatError> {
    let meta = fs::metadata(root).map_err(FormatError::Walk)?;
    if meta.is_file() {
        if is_go_path(root) {
            out.push(root.to_path_buf());
        }
        return Ok(());
    }
    let entries = fs::read_dir(root).map_err(FormatError::Walk)?;
    for entry in entries {
        let entry = entry.map_err(FormatError::Walk)?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let ft = entry.file_type().map_err(FormatError::Walk)?;
        if ft.is_dir() {
            if skip_dir(&name) {
                continue;
            }
            collect_go_files(&path, out)?;
        } else if ft.is_file() && is_go_path(&path) {
            out.push(path);
        }
    }
    Ok(())
}

fn skip_dir(name: &str) -> bool {
    matches!(name, "vendor" | "testdata" | "node_modules")
        || (name.starts_with('.') && name != ".")
}

fn is_go_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".go") && !n.starts_with('.'))
}

/// 1-based original-file line numbers where each change group begins.
fn first_changed_lines<'a>(diff: &TextDiff<'a, 'a, 'a, str>) -> Vec<i64> {
    use similar::ChangeTag;
    let mut lines = Vec::new();
    let mut old_line: i64 = 0;
    let mut in_change = false;
    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                in_change = false;
                old_line += 1;
            }
            ChangeTag::Delete => {
                if !in_change {
                    lines.push(old_line + 1);
                    in_change = true;
                }
                old_line += 1;
            }
            ChangeTag::Insert => {
                if !in_change {
                    // Insertion between two original lines: attribute to the next
                    // original line (at least 1).
                    lines.push((old_line + 1).max(1));
                    in_change = true;
                }
            }
        }
    }
    lines
}

/// Apply ANSI colors to a unified diff (golangci-style `fmt -d`).
fn colorize_diff(diff: &str) -> String {
    const RESET: &str = "\x1b[0m";
    const RED: &str = "\x1b[31m";
    const GREEN: &str = "\x1b[32m";
    const CYAN: &str = "\x1b[36m";
    const BOLD: &str = "\x1b[1m";

    let mut out = String::with_capacity(diff.len() + 64);
    for line in diff.split_inclusive('\n') {
        let (body, nl) = match line.strip_suffix('\n') {
            Some(b) => (b, "\n"),
            None => (line, ""),
        };
        let color = if body.starts_with("+++") || body.starts_with("---") {
            BOLD
        } else if body.starts_with("@@") {
            CYAN
        } else if body.starts_with('+') {
            GREEN
        } else if body.starts_with('-') {
            RED
        } else {
            ""
        };
        if color.is_empty() {
            out.push_str(body);
        } else {
            out.push_str(color);
            out.push_str(body);
            out.push_str(RESET);
        }
        out.push_str(nl);
    }
    out
}

/// Substring match, or `*` wildcard glob (DEFERRED: full regex like golangci).
fn path_matches(path: &str, pat: &str) -> bool {
    if !pat.contains('*') {
        return path.contains(pat);
    }
    let parts: Vec<&str> = pat.split('*').collect();
    let mut rest = path;
    if !parts[0].is_empty() {
        match rest.find(parts[0]) {
            Some(i) => rest = &rest[i + parts[0].len()..],
            None => return false,
        }
    }
    for part in &parts[1..] {
        if part.is_empty() {
            continue;
        }
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gofmt::GofmtOptions;
    use crate::gofumpt::GofumptOptions;
    use crate::meta::MetaFormatter;
    use std::io::Cursor;
    use std::process::Command;

    fn meta_gofmt() -> MetaFormatter {
        MetaFormatter::new(
            &["gofmt".into()],
            GofmtOptions::default(),
            GofumptOptions::default(),
            crate::goimports::GoimportsOptions::default(),
            crate::gci::GciOptions::default(),
            crate::golines::GolinesOptions::default(),
        )
        .unwrap()
    }

    fn gofumpt_available() -> bool {
        Command::new("gofumpt")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn goimports_available() -> bool {
        Command::new("goimports")
            .arg("-h")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty())
            .unwrap_or(false)
    }

    #[test]
    fn rewrites_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        fs::write(&path, "package main\nfunc main(  ) {\n}\n").unwrap();

        let runner = Runner::new(meta_gofmt(), RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.rewritten, 1);

        let got = fs::read_to_string(&path).unwrap();
        assert!(got.contains("func main() {"));
    }

    #[test]
    fn diff_mode_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        let original = "package main\nfunc main(  ) {\n}\n";
        fs::write(&path, original).unwrap();

        let runner = Runner::new(
            meta_gofmt(),
            RunnerOptions {
                diff: true,
                ..Default::default()
            },
        );
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.exit_code, 1);
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        let diff = String::from_utf8(out.into_inner()).unwrap();
        assert!(diff.contains("func main()"), "diff:\n{diff}");
    }

    #[test]
    fn skips_generated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gen.go");
        fs::write(
            &path,
            "// Code generated by tool. DO NOT EDIT.\npackage p\nfunc f(  ) {}\n",
        )
        .unwrap();

        let runner = Runner::new(meta_gofmt(), RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.rewritten, 0);
    }

    #[test]
    fn check_reports_unformatted_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        let original = "package main\nfunc main(  ) {\n}\n";
        fs::write(&path, original).unwrap();

        let runner = Runner::new(meta_gofmt(), RunnerOptions::default());
        let findings = runner.check(&[path.clone()]).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].file, path.to_string_lossy());
        assert_eq!(findings[0].line, 2);
        // check() must not rewrite.
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn check_reports_nothing_for_formatted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        fs::write(&path, "package main\n\nfunc main() {}\n").unwrap();
        let runner = Runner::new(meta_gofmt(), RunnerOptions::default());
        let findings = runner.check(&[path.clone()]).unwrap();
        assert!(findings.is_empty(), "unexpected findings: {findings:?}");
    }

    #[test]
    fn colorize_diff_wraps_added_and_removed() {
        let diff = "--- a.go.orig\n+++ a.go\n@@ -1,2 +1,2 @@\n-old\n+new\n ctx\n";
        let colored = colorize_diff(diff);
        assert!(colored.contains("\x1b[32m+new\x1b[0m"), "green add:\n{colored}");
        assert!(colored.contains("\x1b[31m-old\x1b[0m"), "red del:\n{colored}");
        assert!(colored.contains("\x1b[36m@@"), "cyan hunk:\n{colored}");
        assert!(colored.contains(" ctx\n"), "context unchanged:\n{colored}");
    }

    #[test]
    fn generated_disable_formats_generated_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gen.go");
        fs::write(
            &path,
            "// Code generated by tool. DO NOT EDIT.\npackage p\nfunc f(  ) {}\n",
        )
        .unwrap();

        let runner = Runner::new(
            meta_gofmt(),
            RunnerOptions {
                generated: GeneratedMode::Disable,
                ..Default::default()
            },
        );
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.rewritten, 1);
        let got = fs::read_to_string(&path).unwrap();
        assert!(got.contains("func f() {}"), "got:\n{got}");
    }

    #[test]
    fn generated_strict_does_not_skip_lax_only_marker() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gen.go");
        // Lax marker only (missing trailing period → not strict).
        fs::write(
            &path,
            "// DO NOT EDIT\npackage p\nfunc f(  ) {}\n",
        )
        .unwrap();

        let runner = Runner::new(
            meta_gofmt(),
            RunnerOptions {
                generated: GeneratedMode::Strict,
                ..Default::default()
            },
        );
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.skipped, 0);
        assert_eq!(stats.rewritten, 1);
    }

    #[test]
    fn generated_lax_skips_do_not_edit_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gen.go");
        fs::write(
            &path,
            "// DO NOT EDIT\npackage p\nfunc f(  ) {}\n",
        )
        .unwrap();

        let runner = Runner::new(
            meta_gofmt(),
            RunnerOptions {
                generated: GeneratedMode::Lax,
                ..Default::default()
            },
        );
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.rewritten, 0);
    }

    #[test]
    fn gofumpt_rewrites_empty_line_at_block_start() {
        if !gofumpt_available() {
            eprintln!("skip: gofumpt not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.go");
        fs::write(&path, "package p\n\nfunc f() {\n\n\tx := 1\n\tprintln(x)\n}\n").unwrap();

        let meta = MetaFormatter::new(
            &["gofumpt".into()],
            GofmtOptions::default(),
            GofumptOptions::default(),
            crate::goimports::GoimportsOptions::default(),
            crate::gci::GciOptions::default(),
            crate::golines::GolinesOptions::default(),
        )
        .unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.rewritten, 1);
        let got = fs::read_to_string(&path).unwrap();
        assert!(
            got.contains("func f() {\n\tx := 1"),
            "expected gofumpt rewrite, got:\n{got}"
        );
    }

    #[test]
    fn goimports_sorts_imports_in_place() {
        if !goimports_available() {
            eprintln!("skip: goimports not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.go");
        fs::write(
            &path,
            "package p\n\nimport (\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n}\n",
        )
        .unwrap();

        let meta = MetaFormatter::new(
            &["goimports".into()],
            GofmtOptions::default(),
            GofumptOptions::default(),
            crate::goimports::GoimportsOptions::default(),
            crate::gci::GciOptions::default(),
            crate::golines::GolinesOptions::default(),
        )
        .unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.rewritten, 1);
        let got = fs::read_to_string(&path).unwrap();
        let fmt_pos = got.find("\"fmt\"").expect("fmt");
        let bar_pos = got.find("\"github.com/foo/bar\"").expect("bar");
        assert!(
            fmt_pos < bar_pos,
            "expected stdlib before third-party, got:\n{got}"
        );
    }

    fn gci_available() -> bool {
        Command::new("gci")
            .arg("print")
            .arg("--help")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn gci_sorts_imports_in_place() {
        if !gci_available() {
            eprintln!("skip: gci not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.go");
        fs::write(
            &path,
            "package p\n\nimport (\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n}\n",
        )
        .unwrap();

        let meta = MetaFormatter::new(
            &["gci".into()],
            GofmtOptions::default(),
            GofumptOptions::default(),
            crate::goimports::GoimportsOptions::default(),
            crate::gci::GciOptions::default(),
            crate::golines::GolinesOptions::default(),
        )
        .unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.rewritten, 1);
        let got = fs::read_to_string(&path).unwrap();
        let fmt_pos = got.find("\"fmt\"").expect("fmt");
        let bar_pos = got.find("\"github.com/foo/bar\"").expect("bar");
        assert!(
            fmt_pos < bar_pos,
            "expected stdlib before third-party, got:\n{got}"
        );
    }

    fn golines_available() -> bool {
        Command::new("golines")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn golines_shortens_in_place() {
        if !golines_available() {
            eprintln!("skip: golines not on PATH");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.go");
        fs::write(
            &path,
            "package p\n\nfunc f() {\n\tfoo(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)\n}\n",
        )
        .unwrap();

        let meta = MetaFormatter::new(
            &["golines".into()],
            GofmtOptions::default(),
            GofumptOptions::default(),
            crate::goimports::GoimportsOptions::default(),
            crate::gci::GciOptions::default(),
            crate::golines::GolinesOptions {
                max_len: 60,
                ..Default::default()
            },
        )
        .unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.rewritten, 1);
        let got = fs::read_to_string(&path).unwrap();
        assert!(
            got.contains("foo(\n") || got.lines().count() > 5,
            "expected golines wrap, got:\n{got}"
        );
    }
}
