//! File-tree walker that applies a [`MetaFormatter`] (golangci `pkg/goformat`).

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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
    /// Persistent format-check cache (warm path). `None` → always run `-l`/format.
    pub format_cache: Option<crate::FormatCheckCache>,
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

        if let Some(cache) = &self.opts.format_cache {
            if let (Some(name), Some(opts_fp)) =
                (self.meta.primary_name(), self.meta.options_fingerprint())
            {
                return self.check_with_cache(&files, cache, name, &opts_fp);
            }
        }

        // Fast path: when a single formatter exposes a batch "list unformatted
        // files" mode (`gofmt -l`, `gci list`, …), one invocation flags the
        // (usually small) subset of files whose formatting differs, and we run
        // the per-file diff only on those. Findings are byte-identical: files
        // the tool does not flag satisfy `format(f) == f`, so `check_file` would
        // produce nothing for them anyway. Generated / excluded filtering still
        // happens per-file inside `check_file` on the flagged subset.
        let targets: Vec<PathBuf> = match self.meta.batch_list_unformatted(&files) {
            Some(flagged) => map_flagged(&files, &flagged).unwrap_or(files),
            None => files,
        };

        let mut findings: Vec<FormatFinding> = targets
            .par_iter()
            .map(|path| self.check_file(path))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        findings.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        Ok(findings)
    }

    /// Warm-path check: hash each file, reuse cached clean/lines; only miss
    /// subset goes through `-l` + format. Populates the cache for next run.
    fn check_with_cache(
        &self,
        files: &[PathBuf],
        cache: &crate::FormatCheckCache,
        formatter: &str,
        opts_fp: &str,
    ) -> Result<Vec<FormatFinding>, FormatError> {
        use crate::{content_hash, CachedCheck};

        enum Probe {
            Skip,
            HitClean,
            HitLines(Vec<i64>),
            Miss,
            Io(String, std::io::Error),
        }

        let probed: Vec<(PathBuf, Probe)> = files
            .par_iter()
            .map(|path| {
                if self.is_excluded(path) {
                    return (path.clone(), Probe::Skip);
                }
                let path_str = path.to_string_lossy().to_string();
                let src = match fs::read(path) {
                    Ok(b) => b,
                    Err(e) => return (path.clone(), Probe::Io(path_str, e)),
                };
                if is_generated(&src, self.opts.generated) {
                    return (path.clone(), Probe::Skip);
                }
                let ch = content_hash(&src);
                match cache.get(formatter, opts_fp, &ch) {
                    Some(CachedCheck::Clean) => (path.clone(), Probe::HitClean),
                    Some(CachedCheck::Lines(lines)) => (path.clone(), Probe::HitLines(lines)),
                    None => (path.clone(), Probe::Miss),
                }
            })
            .collect();

        let mut findings: Vec<FormatFinding> = Vec::new();
        let mut misses: Vec<PathBuf> = Vec::new();
        for (path, probe) in probed {
            match probe {
                Probe::Skip | Probe::HitClean => {}
                Probe::HitLines(lines) => {
                    let path_str = path.to_string_lossy().to_string();
                    for line in lines {
                        findings.push(FormatFinding {
                            file: path_str.clone(),
                            line,
                        });
                    }
                }
                Probe::Miss => misses.push(path),
                Probe::Io(path_str, e) => {
                    return Err(FormatError::Io {
                        formatter: "guff-fmt".into(),
                        path: path_str,
                        source: e,
                    });
                }
            }
        }

        if !misses.is_empty() {
            let targets: Vec<PathBuf> = match self.meta.batch_list_unformatted(&misses) {
                Some(flagged) => map_flagged(&misses, &flagged).unwrap_or(misses.clone()),
                None => misses.clone(),
            };
            let flagged: std::collections::HashSet<PathBuf> =
                targets.into_iter().collect();

            let miss_results: Vec<(PathBuf, Result<CachedCheck, FormatError>)> = misses
                .par_iter()
                .map(|path| {
                    let path_str = path.to_string_lossy();
                    let src = match fs::read(path) {
                        Ok(b) => b,
                        Err(e) => {
                            return (
                                path.clone(),
                                Err(FormatError::Io {
                                    formatter: "guff-fmt".into(),
                                    path: path_str.to_string(),
                                    source: e,
                                }),
                            );
                        }
                    };
                    let ch = content_hash(&src);
                    if flagged.contains(path) {
                        match self.meta.format(&path_str, &src) {
                            Ok(out) if out == src => {
                                cache.put(formatter, opts_fp, &ch, &CachedCheck::Clean);
                                (path.clone(), Ok(CachedCheck::Clean))
                            }
                            Ok(out) => {
                                let old = String::from_utf8_lossy(&src);
                                let new = String::from_utf8_lossy(&out);
                                let diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
                                let lines = first_changed_lines(&diff);
                                cache.put(
                                    formatter,
                                    opts_fp,
                                    &ch,
                                    &CachedCheck::Lines(lines.clone()),
                                );
                                (path.clone(), Ok(CachedCheck::Lines(lines)))
                            }
                            Err(e) => (path.clone(), Err(e)),
                        }
                    } else {
                        cache.put(formatter, opts_fp, &ch, &CachedCheck::Clean);
                        (path.clone(), Ok(CachedCheck::Clean))
                    }
                })
                .collect();

            for (path, res) in miss_results {
                match res? {
                    CachedCheck::Clean => {}
                    CachedCheck::Lines(lines) => {
                        let path_str = path.to_string_lossy().to_string();
                        for line in lines {
                            findings.push(FormatFinding {
                                file: path_str.clone(),
                                line,
                            });
                        }
                    }
                }
            }
        }

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

/// Run a formatter's batch "list unformatted files" mode over `files`,
/// returning the union of the paths it lists.
///
/// Files are split into roughly one chunk per rayon worker and the per-chunk
/// tool invocations run concurrently: some list tools (notably `goimports`) are
/// single-threaded internally, so fanning chunks across processes recovers the
/// cores (goimports over the prometheus tree: ~1.4s in one process → ~0.35s in
/// eight). Tools that already parallelize internally (`gofumpt`, `gci`) are
/// unaffected. The chunk size is also capped so a huge tree stays under ARG_MAX.
///
/// Returns `None` if any spawn fails or a chunk exits non-zero (e.g. a file
/// fails to parse) so the caller falls back to the per-file path, preserving
/// the per-file error behavior exactly. `configure` builds a fresh command with
/// the list subcommand/flags set; the file paths are appended per chunk.
pub(crate) fn batch_list(
    files: &[&Path],
    configure: impl Fn() -> Command + Sync,
) -> Option<Vec<PathBuf>> {
    if files.is_empty() {
        return Some(Vec::new());
    }
    let nthreads = rayon::current_num_threads().max(1);
    let chunk_size = files.len().div_ceil(nthreads).clamp(1, 400);
    let chunks: Vec<&[&Path]> = files.chunks(chunk_size).collect();
    let per: Option<Vec<Vec<PathBuf>>> = chunks
        .par_iter()
        .map(|chunk| run_list_chunk(chunk, &configure))
        .collect();
    per.map(|v| v.into_iter().flatten().collect())
}

/// Native (in-process) equivalent of a formatter's external `-l` list mode:
/// read each file, format it via `format`, and flag the ones whose formatting
/// differs. Used by the default-native formatters (gofmt/gofumpt/gci) so the
/// cold check path no longer spawns a `-l`/`list` subprocess — native
/// `format()` now outpaces it (PERF_TASKS Task 1: native gofumpt 161ms <
/// `gofumpt -l` 180ms on 725 files) and removes the external-tool dependency.
///
/// A file that can't be read, or that `format` fails on, is flagged rather than
/// dropped: the caller runs the per-file [`Runner::check_file`] on the flagged
/// subset, which re-reads/re-formats it and reproduces the exact I/O or format
/// error — or, for generated/excluded files, drops it — so behavior matches the
/// old subprocess path (`-l` also can't see generated/excluded exclusions; that
/// filtering has always happened per-file in `check_file`). `format` must be the
/// same formatting `check_file` applies, so the flagged set is exactly the files
/// that will yield a finding. Runs in parallel; always returns `Some`.
pub(crate) fn native_list<F>(files: &[&Path], format: F) -> Option<Vec<PathBuf>>
where
    F: Fn(&str, &[u8]) -> Result<Vec<u8>, FormatError> + Sync,
{
    let flagged: Vec<PathBuf> = files
        .par_iter()
        .filter_map(|path| {
            let src = match fs::read(path) {
                Ok(b) => b,
                // Unreadable → flag so check_file reproduces the I/O error.
                Err(_) => return Some(path.to_path_buf()),
            };
            match format(&path.to_string_lossy(), &src) {
                Ok(out) if out == src => None,
                // Differs, or format failed → flag (check_file emits the finding
                // or reproduces the error / drops it if generated).
                Ok(_) | Err(_) => Some(path.to_path_buf()),
            }
        })
        .collect();
    Some(flagged)
}

/// One chunked invocation for [`batch_list`]. `None` = spawn failed or the tool
/// exited non-zero (caller falls back to per-file).
fn run_list_chunk(chunk: &[&Path], configure: &(impl Fn() -> Command + ?Sized)) -> Option<Vec<PathBuf>> {
    let mut cmd = configure();
    for f in chunk {
        cmd.arg(f);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let mut out = Vec::new();
    for line in output.stdout.split(|&b| b == b'\n') {
        let s = String::from_utf8_lossy(line);
        let t = s.trim();
        if !t.is_empty() {
            out.push(PathBuf::from(t));
        }
    }
    Some(out)
}

/// Normalize a path for matching a tool's (possibly path-cleaned) echoed output
/// against the original file list: forward slashes, drop a single leading `./`.
fn path_match_key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Map each tool-flagged path back to the matching original `PathBuf` so the
/// reported finding path stays byte-identical to the per-file path. Returns
/// `None` if any flagged path can't be matched (caller falls back to per-file).
fn map_flagged(files: &[PathBuf], flagged: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let mut by_key: HashMap<String, &PathBuf> = HashMap::with_capacity(files.len());
    for f in files {
        by_key.insert(path_match_key(f), f);
    }
    let mut out = Vec::with_capacity(flagged.len());
    for f in flagged {
        out.push((*by_key.get(&path_match_key(f))?).clone());
    }
    Some(out)
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
