//! File-tree walker that applies a [`MetaFormatter`] (golangci `pkg/goformat`).

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use similar::TextDiff;

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

        // Skip generated files (golangci GeneratedFileMatcher default heuristic).
        if is_generated(&input) {
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
            stdout
                .write_all(unified.as_bytes())
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

fn skip_dir(name: &str) -> bool {
    matches!(name, "vendor" | "testdata" | "node_modules")
        || (name.starts_with('.') && name != ".")
}

fn is_go_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with(".go") && !n.starts_with('.'))
}

/// golangci / go generate marker: `Code generated` … `DO NOT EDIT`.
fn is_generated(src: &[u8]) -> bool {
    let head = &src[..src.len().min(2048)];
    let s = String::from_utf8_lossy(head);
    for line in s.lines().take(40) {
        let t = line.trim();
        if t.starts_with("//") || t.starts_with("/*") {
            if t.contains("Code generated") && t.contains("DO NOT EDIT") {
                return true;
            }
        } else if !t.is_empty() {
            break;
        }
    }
    false
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
    use crate::meta::MetaFormatter;
    use std::io::Cursor;

    #[test]
    fn rewrites_file_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.go");
        fs::write(&path, "package main\nfunc main(  ) {\n}\n").unwrap();

        let meta = MetaFormatter::new(&["gofmt".into()], GofmtOptions::default()).unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
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

        let meta = MetaFormatter::new(&["gofmt".into()], GofmtOptions::default()).unwrap();
        let runner = Runner::new(
            meta,
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

        let meta = MetaFormatter::new(&["gofmt".into()], GofmtOptions::default()).unwrap();
        let runner = Runner::new(meta, RunnerOptions::default());
        let mut out = Cursor::new(Vec::new());
        let stats = runner.run(&[path.clone()], &mut out).unwrap();
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.rewritten, 0);
    }
}
