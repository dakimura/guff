//! `guff run --watch` — keep the process alive and re-lint on file changes.
//!
//! PERF_TASKS_V2 C-2. The default one-shot `guff run` path is unchanged. Watch
//! reuses the in-memory metadata graph + [`IssueCache`] and surgically
//! invalidates content hashes for dirty paths (see
//! [`guff_runner::IssueCache::invalidate_paths`]). Type/SSA arenas are **not**
//! retained across iterations so idle RSS stays near warm one-shot (~0.1 GiB),
//! not cold peak (~3.5 GiB).

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify_debouncer_mini::notify::RecursiveMode;
use notify_debouncer_mini::notify::Watcher;
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};

use guff_runner::IssueCache;

use crate::format::print_issues_with;
use crate::{
    prepare_linter_run, run_format_checks, run_linters_on_graph, LintOptions, PreparedLint,
    RunError,
};

/// Exit codes / knobs for tests and scripts.
///
/// `GUFF_WATCH_MAX_ITERS=N` — stop after N successful lint passes (including
/// the initial one). Used by perf/regress harnesses; undocumented.
fn watch_max_iters() -> Option<u32> {
    std::env::var("GUFF_WATCH_MAX_ITERS")
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .filter(|&n| n > 0)
}

/// Debounce window for coalescing editor save storms.
fn debounce() -> Duration {
    Duration::from_millis(120)
}

/// Run an initial lint, then re-lint when `.go` / module files change.
///
/// Incompatible with `--fix` (rewriting while watching races the analyzer).
pub fn run_watch(opts: &LintOptions) -> Result<i32, RunError> {
    if opts.fix {
        return Err(RunError::Message(
            "--watch cannot be combined with --fix".into(),
        ));
    }
    guff_runner::init_rayon_global_stack();

    let watch_roots = discover_watch_roots(opts)?;
    if watch_roots.is_empty() {
        return Err(RunError::Message(
            "--watch: no directories to watch (check patterns / cwd)".into(),
        ));
    }

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(debounce(), move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| RunError::Message(format!("watch: {e}")))?;

    for root in &watch_roots {
        debouncer
            .watcher()
            .watch(root, RecursiveMode::Recursive)
            .map_err(|e| RunError::Message(format!("watch {}: {e}", root.display())))?;
    }

    eprintln!(
        "guff: watch mode on {} root(s); press Ctrl-C to stop",
        watch_roots.len()
    );

    let mut prepared = prepare_linter_run(opts).map_err(RunError::Runner)?;
    let mut last_code = 0i32;
    let mut iters = 0u32;
    let max_iters = watch_max_iters();
    let mut first = true;

    loop {
        let t0 = Instant::now();
        let speculate = if first {
            first = false;
            prepared.speculate_job.take()
        } else {
            None
        };
        last_code = run_one_pass(opts, &prepared, speculate)?;
        iters += 1;
        eprintln!(
            "guff: watch: pass #{iters} {:.2}s (exit {last_code})",
            t0.elapsed().as_secs_f64(),
        );

        if max_iters.is_some_and(|m| iters >= m) {
            return Ok(last_code);
        }

        // Drain events until we have a actionable batch.
        let changes = wait_for_changes(&rx)?;
        match classify_changes(&changes, &prepared.graph) {
            ChangeClass::Ignore => continue,
            ChangeClass::Content(paths) => {
                if crate::debug::enabled() {
                    eprintln!(
                        "guff: watch: content change ({} path(s)); invalidating hashes",
                        paths.len()
                    );
                }
                reclaim_and_invalidate(&mut prepared, &paths)?;
            }
            ChangeClass::Structural => {
                if crate::debug::enabled() {
                    eprintln!("guff: watch: structural change; reloading package graph");
                }
                prepared = prepare_linter_run(opts).map_err(RunError::Runner)?;
            }
        }
    }
}

fn run_one_pass(
    opts: &LintOptions,
    prepared: &PreparedLint,
    speculate: Option<guff_packages::SpeculativeSeedJob>,
) -> Result<i32, RunError> {
    let timing = crate::debug::enabled();
    let cache = prepared.cache.as_ref().map(Arc::clone);
    let result = run_linters_on_graph(opts, &prepared.graph, cache, speculate)
        .map_err(RunError::Runner)?;

    let tf = Instant::now();
    let (mut issues, _) = result.issues_and_fix(false)?;
    if timing {
        eprintln!("guff: phase issues+filter {:.2}s", tf.elapsed().as_secs_f64());
    }
    // Drop LintResult (packages + action graph) before format so peak RSS
    // during watch idle / format does not stack on type artifacts.
    drop(result);

    if let Some(fmt_cfg) = &opts.formatters {
        let tfmt = Instant::now();
        let fmt_issues = run_format_checks(fmt_cfg, &opts.filter)?;
        if timing {
            eprintln!("guff: phase format_checks {:.2}s", tfmt.elapsed().as_secs_f64());
        }
        issues.extend(fmt_issues);
    }

    let tp = Instant::now();
    let mut out = io::stdout();
    print_issues_with(&opts.out_formats, &opts.printer, &issues, &mut out).map_err(RunError::Io)?;
    let _ = out.flush();
    if timing {
        eprintln!("guff: phase print {:.2}s", tp.elapsed().as_secs_f64());
    }
    Ok(if issues.is_empty() {
        0
    } else {
        opts.issues_exit_code
    })
}

fn reclaim_and_invalidate(prepared: &mut PreparedLint, paths: &[PathBuf]) -> Result<(), RunError> {
    let Some(arc) = prepared.cache.take() else {
        return Ok(());
    };
    let mut cache = match Arc::try_unwrap(arc) {
        Ok(c) => c,
        Err(arc) => {
            // Action graph still held the Arc (shouldn't after drop). Fall back
            // to a fresh registry rebuild — correct, just slower.
            eprintln!("guff: watch: cache still shared; rebuilding dep-hash registry");
            let mut c = IssueCache::open(arc.dir().to_path_buf(), arc.salt().to_string())
                .map_err(|e| RunError::Message(e.to_string()))?;
            c.set_dep_hashes(&prepared.graph.all_packages)
                .map_err(|e| RunError::Message(e.to_string()))?;
            prepared.cache = Some(Arc::new(c));
            return reclaim_and_invalidate(prepared, paths);
        }
    };
    cache
        .invalidate_paths(paths, &prepared.graph.all_packages)
        .map_err(|e| RunError::Message(e.to_string()))?;
    prepared.cache = Some(Arc::new(cache));
    Ok(())
}

enum ChangeClass {
    Ignore,
    Content(Vec<PathBuf>),
    Structural,
}

fn classify_changes(
    paths: &[PathBuf],
    graph: &crate::MetadataGraph,
) -> ChangeClass {
    let mut content = Vec::new();
    let mut structural = false;
    for p in paths {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name == "go.mod" || name == "go.sum" || name == "go.work" || name == "go.work.sum" {
            structural = true;
            break;
        }
        if !name.ends_with(".go") {
            continue;
        }
        // Create/delete: path not in any compiled_go_files list → structural.
        let known = graph.all_packages.iter().any(|pkg| {
            pkg.compiled_go_files.iter().any(|f| f == p)
                || pkg
                    .compiled_go_files
                    .iter()
                    .any(|f| f.canonicalize().ok().as_ref() == Some(p))
                || p.canonicalize()
                    .ok()
                    .map(|c| pkg.compiled_go_files.iter().any(|f| f == &c))
                    .unwrap_or(false)
        });
        if known {
            if !p.exists() {
                structural = true;
                break;
            }
            content.push(p.clone());
        } else {
            // New file, or deleted file whose path vanished — reload graph.
            structural = true;
            break;
        }
    }
    if structural {
        ChangeClass::Structural
    } else if content.is_empty() {
        ChangeClass::Ignore
    } else {
        ChangeClass::Content(content)
    }
}

fn wait_for_changes(
    rx: &mpsc::Receiver<
        Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify_debouncer_mini::notify::Error>,
    >,
) -> Result<Vec<PathBuf>, RunError> {
    loop {
        let evs = rx
            .recv()
            .map_err(|_| RunError::Message("watch: channel closed".into()))?
            .map_err(|e| RunError::Message(format!("watch: {e}")))?;
        let mut paths = Vec::new();
        for ev in evs {
            // DebouncedEventKind::Any covers create/modify/remove.
            if matches!(
                ev.kind,
                DebouncedEventKind::Any | DebouncedEventKind::AnyContinuous
            ) {
                paths.push(ev.path);
            }
        }
        if !paths.is_empty() {
            paths.sort();
            paths.dedup();
            return Ok(paths);
        }
    }
}

fn discover_watch_roots(opts: &LintOptions) -> Result<Vec<PathBuf>, RunError> {
    let mut roots = Vec::new();
    let cwd = std::env::current_dir().map_err(RunError::Io)?;
    for pat in &opts.patterns {
        let p = Path::new(pat);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            cwd.join(p)
        };
        // `./...` → cwd; `./pkg` → that dir; files → parent.
        let dir = if abs.is_dir() {
            abs
        } else if abs.extension().and_then(|e| e.to_str()) == Some("go") {
            abs.parent().unwrap_or(&cwd).to_path_buf()
        } else {
            // pattern like `./...` or package path — watch cwd.
            cwd.clone()
        };
        if !roots.iter().any(|r| r == &dir) {
            roots.push(dir);
        }
    }
    if roots.is_empty() {
        roots.push(cwd);
    }
    Ok(roots)
}
