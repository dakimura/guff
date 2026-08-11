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
use crate::Formatter;

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
#[derive(Debug, Clone)]
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
    /// Include `*_test.go` (golangci `run.tests`; default **true**).
    ///
    /// When false, test files are skipped during `guff run` format diagnostics
    /// (matching `golangci-lint run` with `run.tests: false`).
    pub include_tests: bool,
    /// Extra build tags from `run.build-tags` (merged into [`guff_build::Context`]).
    pub build_tags: Vec<String>,
    /// When true, skip files whose `//go:build` lines are not satisfied
    /// (golangci `run` format analyzers via package loader). `guff fmt` keeps
    /// this false so inactive-tag files are still rewritten.
    pub filter_build_constraints: bool,
}

impl Default for RunnerOptions {
    fn default() -> Self {
        Self {
            diff: false,
            stdin: false,
            exclude_paths: Vec::new(),
            generated: GeneratedMode::default(),
            color: false,
            format_cache: None,
            include_tests: true,
            build_tags: Vec::new(),
            filter_build_constraints: false,
        }
    }
}

impl RunnerOptions {
    /// Build context used to filter format-eligible files.
    fn build_context(&self) -> guff_build::Context {
        guff_build::DEFAULT.clone().with_build_tags(self.build_tags.iter())
    }
}

/// A single formatting difference found by [`Runner::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatFinding {
    /// File that is not properly formatted.
    pub file: String,
    /// 1-based line number of the change (start of the first differing hunk).
    pub line: i64,
}

/// [`FormatFinding`] tagged with the formatter that produced it (B-10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedFinding {
    pub formatter: String,
    pub file: String,
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
        self.check_files(&Self::collect_paths(paths)?)
    }

    /// Collect `.go` files under `paths` (same discovery as [`Self::check`]).
    /// Shared by `guff run` format checks so multiple formatters share one walk.
    pub fn collect_paths(paths: &[PathBuf]) -> Result<Vec<PathBuf>, FormatError> {
        let roots: Vec<PathBuf> = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.to_vec()
        };
        let mut files = Vec::new();
        for root in &roots {
            collect_go_files(root, &mut files)?;
        }
        Ok(files)
    }

    /// Like [`Self::check`], but with a pre-collected file list (skips the walk).
    pub fn check_files(&self, files: &[PathBuf]) -> Result<Vec<FormatFinding>, FormatError> {
        if let Some(cache) = &self.opts.format_cache {
            if let (Some(name), Some(opts_fp)) =
                (self.meta.primary_name(), self.meta.options_fingerprint())
            {
                return self.check_with_cache(files, cache, name, &opts_fp);
            }
        }

        // Subprocess fast path: when a single formatter exposes batch "list
        // unformatted" (`gofmt -l`, `gci list`, …), one invocation flags the
        // (usually small) subset whose formatting differs, and we run the
        // per-file diff only on those. Native formatters return `None` here so
        // we format each file once in `check_file` (a list pre-pass would
        // re-format every flagged file). Generated / excluded filtering stays
        // inside `check_file`.
        let targets: Vec<PathBuf> = match self.meta.batch_list_unformatted(files) {
            Some(flagged) => map_flagged(files, &flagged).unwrap_or_else(|| files.to_vec()),
            None => files.to_vec(),
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
        if !self.opts.include_tests && is_test_go_path(path) {
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
        if !self.file_matches_build(path, &input) {
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
        if !self.opts.include_tests && is_test_go_path(path) {
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
        if !self.file_matches_build(path, &input) {
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

    /// Whether `path`/`src` satisfies `run.build-tags` (and GOOS/GOARCH).
    fn file_matches_build(&self, path: &Path, src: &[u8]) -> bool {
        if !self.opts.filter_build_constraints {
            return true;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file.go");
        match self.opts.build_context().match_file(name, src) {
            Ok(ok) => ok,
            // Malformed build lines: still format (golangci loads what it can).
            Err(_) => true,
        }
    }
}

/// Check-mode multi-formatter pass (B-10): one `fs::read` per file, then each
/// formatter's `format` independently. Findings are grouped by formatter in
/// `formatters` order (same attribution as the old per-formatter `check_files`
/// loop). Does not touch `--fix` chaining.
///
/// When `opts.format_cache` is set, content is hashed once per file and each
/// formatter probes/puts under its own cache key.
pub fn check_files_multi(
    formatters: &[Box<dyn Formatter>],
    files: &[PathBuf],
    opts: &RunnerOptions,
) -> Result<Vec<AttributedFinding>, FormatError> {
    if formatters.is_empty() {
        return Ok(Vec::new());
    }

    let cache = opts.format_cache.as_ref();
    let fingerprints: Vec<(&str, String)> = formatters
        .iter()
        .map(|f| (f.name(), f.options_fingerprint()))
        .collect();

    // Per-file results: Vec of (formatter_index, lines). Empty lines = clean.
    let per_file: Vec<Result<Vec<(usize, Vec<i64>)>, FormatError>> = files
        .par_iter()
        .map(|path| check_file_multi(path, formatters, &fingerprints, opts, cache))
        .collect();

    // Preserve legacy issue order: all findings for formatter[0] (sorted by
    // file, line), then formatter[1], …
    let mut by_fmt: Vec<Vec<AttributedFinding>> =
        (0..formatters.len()).map(|_| Vec::new()).collect();
    for (path, res) in files.iter().zip(per_file) {
        let path_str = path.to_string_lossy().to_string();
        for (fmt_idx, lines) in res? {
            let name = formatters[fmt_idx].name();
            // One finding per file per formatter, at the first change.
            //
            // The diff can have several change groups and `first_changed_lines`
            // returns all of them, but golangci-lint reports "File is not
            // properly formatted" once for the file: measured on a file with
            // two `func f(  )` declarations seven lines apart (two hunks),
            // golangci reports line 3 only, with `max-same-issues: 0` and
            // `uniq-by-line: false`. Its own golines testdata expects a single
            // want-comment for a file with a dozen over-long lines.
            if let Some(&line) = lines.first() {
                by_fmt[fmt_idx].push(AttributedFinding {
                    formatter: name.to_string(),
                    file: path_str.clone(),
                    line,
                });
            }
        }
    }
    let mut out = Vec::new();
    for mut group in by_fmt {
        group.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
        out.append(&mut group);
    }
    Ok(out)
}

fn check_file_multi(
    path: &Path,
    formatters: &[Box<dyn Formatter>],
    fingerprints: &[(&str, String)],
    opts: &RunnerOptions,
    cache: Option<&crate::FormatCheckCache>,
) -> Result<Vec<(usize, Vec<i64>)>, FormatError> {
    use crate::{content_hash, CachedCheck};

    if is_excluded_path(path, &opts.exclude_paths) {
        return Ok(Vec::new());
    }
    if !opts.include_tests && is_test_go_path(path) {
        return Ok(Vec::new());
    }
    let path_str = path.to_string_lossy();
    let src = fs::read(path).map_err(|e| FormatError::Io {
        formatter: "guff-fmt".into(),
        path: path_str.to_string(),
        source: e,
    })?;
    if is_generated(&src, opts.generated) {
        return Ok(Vec::new());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file.go");
    if opts.filter_build_constraints {
        match opts.build_context().match_file(name, &src) {
            Ok(false) => return Ok(Vec::new()),
            Ok(true) => {}
            // Malformed build lines: still format (golangci loads what it can).
            Err(_) => {}
        }
    }

    let ch = cache.map(|_| content_hash(&src));
    let mut out = Vec::with_capacity(formatters.len());

    // Prefer a shared skip-object parse when native gci + gofumpt are both in
    // the set (B-10 parse share). Other formatters still run on `src`.
    let shared = try_shared_gci_gofumpt(formatters, fingerprints, &path_str, &src, ch.as_deref(), cache);

    for (i, formatter) in formatters.iter().enumerate() {
        if let Some(lines) = shared.as_ref().and_then(|s| s[i].clone()) {
            out.push((i, lines));
            continue;
        }

        let name = fingerprints[i].0;
        let opts_fp = &fingerprints[i].1;
        if let (Some(cache), Some(ch)) = (cache, ch.as_ref()) {
            match cache.get(name, opts_fp, ch) {
                Some(CachedCheck::Clean) => {
                    out.push((i, Vec::new()));
                    continue;
                }
                Some(CachedCheck::Lines(lines)) => {
                    out.push((i, lines));
                    continue;
                }
                None => {}
            }
        }

        let formatted = formatter.format(&path_str, &src)?;
        let lines = if formatted == src {
            Vec::new()
        } else {
            let old = String::from_utf8_lossy(&src);
            let new = String::from_utf8_lossy(&formatted);
            let diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
            first_changed_lines(&diff)
        };
        if let (Some(cache), Some(ch)) = (cache, ch.as_ref()) {
            let entry = if lines.is_empty() {
                CachedCheck::Clean
            } else {
                CachedCheck::Lines(lines.clone())
            };
            cache.put(name, opts_fp, ch, &entry);
        }
        out.push((i, lines));
    }
    Ok(out)
}

/// When both native `gci` and `gofumpt` are present, parse once and format both.
/// Returns `Some(per_formatter)` where `per_formatter[i] = Some(lines)` if that
/// index was handled, `None` if the caller should format normally. `None` for
/// the whole result means shared path was not applicable.
fn try_shared_gci_gofumpt(
    formatters: &[Box<dyn Formatter>],
    fingerprints: &[(&str, String)],
    path_str: &str,
    src: &[u8],
    content_hash: Option<&str>,
    cache: Option<&crate::FormatCheckCache>,
) -> Option<Vec<Option<Vec<i64>>>> {
    use crate::CachedCheck;

    if std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "0") {
        return None;
    }
    let gci_i = formatters.iter().position(|f| f.name() == "gci")?;
    let gofumpt_i = formatters.iter().position(|f| f.name() == "gofumpt")?;

    // If both are cache hits, skip the shared parse entirely.
    if let (Some(cache), Some(ch)) = (cache, content_hash) {
        let gci_hit = cache.get(fingerprints[gci_i].0, &fingerprints[gci_i].1, ch);
        let fumpt_hit = cache.get(fingerprints[gofumpt_i].0, &fingerprints[gofumpt_i].1, ch);
        if gci_hit.is_some() && fumpt_hit.is_some() {
            let mut slots = vec![None; formatters.len()];
            slots[gci_i] = Some(match gci_hit.unwrap() {
                CachedCheck::Clean => Vec::new(),
                CachedCheck::Lines(l) => l,
            });
            slots[gofumpt_i] = Some(match fumpt_hit.unwrap() {
                CachedCheck::Clean => Vec::new(),
                CachedCheck::Lines(l) => l,
            });
            return Some(slots);
        }
        // One hit: still fall through to shared parse for the miss; the hit
        // will be overwritten with the same result (or we could skip — but
        // re-format is rare on warm partial hits).
    }

    let gci_fmt = formatters[gci_i].as_ref();
    let fumpt_fmt = formatters[gofumpt_i].as_ref();
    // Build native opts via format() only if both expose the shared helper.
    // Concrete types: call through Formatter::format is the fallback; shared
    // path lives on native module and needs NativeOptions from the wrappers.
    let (gci_out, fumpt_out) =
        crate::native::format_gci_gofumpt_shared(src, path_str, gci_fmt, fumpt_fmt)?;

    let lines_of = |out: &Vec<u8>| -> Vec<i64> {
        if out.as_slice() == src {
            Vec::new()
        } else {
            let old = String::from_utf8_lossy(src);
            let new = String::from_utf8_lossy(out);
            let diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
            first_changed_lines(&diff)
        }
    };

    let gci_lines = match gci_out {
        Ok(ref o) => lines_of(o),
        Err(_) => return None, // fall back to per-formatter format()
    };
    let fumpt_lines = match fumpt_out {
        Ok(ref o) => lines_of(o),
        Err(_) => return None,
    };

    if let (Some(cache), Some(ch)) = (cache, content_hash) {
        let put = |idx: usize, lines: &[i64]| {
            let entry = if lines.is_empty() {
                CachedCheck::Clean
            } else {
                CachedCheck::Lines(lines.to_vec())
            };
            cache.put(fingerprints[idx].0, &fingerprints[idx].1, ch, &entry);
        };
        put(gci_i, &gci_lines);
        put(gofumpt_i, &fumpt_lines);
    }

    let mut slots = vec![None; formatters.len()];
    slots[gci_i] = Some(gci_lines);
    slots[gofumpt_i] = Some(fumpt_lines);
    let _ = (gci_out, fumpt_out);
    Some(slots)
}

fn is_excluded_path(path: &Path, exclude_paths: &[String]) -> bool {
    if exclude_paths.is_empty() {
        return false;
    }
    let s = path.to_string_lossy().replace('\\', "/");
    exclude_paths.iter().any(|pat| path_matches(&s, pat))
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

fn is_test_go_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_test.go"))
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
