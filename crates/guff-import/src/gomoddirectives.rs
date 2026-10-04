//! Port of [`github.com/ldez/gomoddirectives`](https://github.com/ldez/gomoddirectives)
//! (golangci-lint wrapper in `pkg/golinters/gomoddirectives`).
//!
//! Defaults match golangci-lint: replace directives are forbidden (unless listed /
//! local allowed — both off by default); retract requires a rationale comment;
//! exclude / toolchain / tool / godebug are allowed unless explicitly forbidden.
//!
//! Settings: `linters.settings.gomoddirectives` (`replace-local`,
//! `replace-allow-list`, `retract-allow-no-explanation`, `exclude-forbidden`,
//! `toolchain-forbidden`, `tool-forbidden`, `go-debug-forbidden`).
//! `replace-allow-all` and `ignore-forbidden` are gomoddirectives v0.9 / v0.10
//! (golangci-lint 2.13 / 2.14), as is the unconditional check that an `ignore`
//! path does not name a directory the go command already skips.
//! DEFERRED: `toolchain-pattern`, `go-version-pattern`, `check-module-path`.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::gomod::{find_gomod, parse_gomod, Replace};
use crate::options::GomoddirectivesOptions;

const REASON_REPLACE: &str = "replacement are not allowed";
const REASON_REPLACE_LOCAL: &str = "local replacement are not allowed";
const REASON_REPLACE_DUPLICATE: &str = "multiple replacement of the same module";
const REASON_REPLACE_IDENTICAL: &str = "the original module and the replacement are identical";
const REASON_RETRACT: &str = "a comment is mandatory to explain why the version has been retracted";
const REASON_EXCLUDE: &str = "exclude directive is not allowed";
const REASON_TOOLCHAIN: &str = "toolchain directive is not allowed";
const REASON_TOOL: &str = "tool directive is not allowed";
const REASON_GODEBUG: &str = "godebug directive is not allowed";
const REASON_IGNORE: &str = "ignore directive is not allowed";
const REASON_IGNORED_BY_DEFAULT_HIDDEN: &str =
    "files/directories starting with '.' and '_' are ignored by default";

/// Go's `path.Clean`, for splitting an `ignore` path into elements the way
/// upstream does (`strings.SplitSeq(path.Clean(value.Path), "/")`).
fn go_path_clean(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let absolute = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if out.last().is_some_and(|l| *l != "..") {
                    out.pop();
                } else if !absolute {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

/// `checkIgnoreDirectives` (v0.10): one finding per *element* that the go
/// command would skip anyway — `vendor`, `testdata`, or a name starting with
/// `.` or `_`. `..` starts with `.`, so `ignore ../x` is reported too; that
/// is upstream's, and kept.
fn ignored_by_default(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    for elem in go_path_clean(value).split('/') {
        if elem == "." {
            continue;
        }
        if elem == "vendor" || elem == "testdata" {
            out.push(format!("directories named '{elem}' are ignored by default"));
        } else if elem.starts_with('.') || elem.starts_with('_') {
            out.push(REASON_IGNORED_BY_DEFAULT_HIDDEN.to_string());
        }
    }
    out
}

/// Deduplicate analysis across packages that share a module root.
fn checked_gomods() -> &'static Mutex<HashSet<String>> {
    static CHECKED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CHECKED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn check_replace(r: &Replace, opts: &GomoddirectivesOptions) -> Option<String> {
    if opts.replace_allow_all {
        return None;
    }
    if opts.replace_allow_list.iter().any(|p| p == &r.old_path) {
        return None;
    }
    if r.is_local() {
        if opts.replace_local {
            return None;
        }
        return Some(format!("{REASON_REPLACE_LOCAL}: {}", r.old_path));
    }
    // Non-local replace: still forbidden unless on allow-list (handled above).
    Some(format!("{REASON_REPLACE}: {}", r.old_path))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gomoddirectives requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GomoddirectivesOptions>("gomoddirectives")
        .cloned()
        .unwrap_or_default();

    let Some(gomod_path) = find_gomod(&pass.pkg().dir) else {
        return Ok(None);
    };
    let key = gomod_path.to_string_lossy().to_string();
    {
        let mut checked = checked_gomods().lock().unwrap();
        if !checked.insert(key) {
            return Ok(None);
        }
    }

    let src = std::fs::read_to_string(&gomod_path)
        .map_err(|e| format!("gomoddirectives: read {}: {e}", gomod_path.display()))?;
    let Some(gomod) = parse_gomod(&gomod_path) else {
        return Ok(None);
    };

    // Register go.mod in the analysis FileSet so reports land on the directive
    // line (golangci/ldez also point at go.mod, not a .go file).
    let filename = gomod_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("go.mod");
    let gomod_file = pass.fset().add_file(filename, -1, src.len() as i64);
    gomod_file.set_lines_for_content(src.as_bytes());
    // modfile reports a directive at its `Syntax.Start`: the first token of
    // the line, which inside a `replace ( … )` / `ignore ( … )` block is past
    // the indentation. Reporting at the line start put every block entry one
    // column early.
    let src_lines: Vec<&str> = src.split('\n').collect();
    let line_pos = |line: u32| -> u32 {
        let line = line.max(1) as usize;
        let max = gomod_file.line_count().max(1);
        let line = line.min(max);
        let indent = src_lines
            .get(line - 1)
            .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
            .unwrap_or(0);
        gomod_file.line_start(line).0 as u32 + indent as u32
    };

    let mut pending: Vec<(u32, String)> = Vec::new();

    let mut uniq = HashSet::new();
    for r in &gomod.replaces {
        if let Some(reason) = check_replace(r, &opts) {
            pending.push((line_pos(r.line), reason));
            continue;
        }
        if r.old_path == r.new_path && r.old_version == r.new_version {
            pending.push((line_pos(r.line), REASON_REPLACE_IDENTICAL.to_string()));
            continue;
        }
        let key = format!("{}{}", r.old_path, r.old_version);
        if !uniq.insert(key) {
            pending.push((line_pos(r.line), REASON_REPLACE_DUPLICATE.to_string()));
        }
    }

    if !opts.retract_allow_no_explanation {
        for retract in &gomod.retracts {
            if retract.rationale.is_empty() {
                pending.push((line_pos(retract.line), REASON_RETRACT.to_string()));
            }
        }
    }

    // Each of these is reported at the directive's own line. `line_pos(1)` put
    // them all on the `module` line, which no fixture could see while it held
    // one directive: with one finding there is nothing for the line to be wrong
    // relative to.
    if opts.exclude_forbidden {
        for excl in &gomod.excludes {
            pending.push((line_pos(excl.line), REASON_EXCLUDE.to_string()));
        }
    }
    for ig in &gomod.ignores {
        for reason in ignored_by_default(&ig.value) {
            pending.push((line_pos(ig.line), reason));
        }
    }
    if opts.ignore_forbidden {
        for ig in &gomod.ignores {
            pending.push((line_pos(ig.line), REASON_IGNORE.to_string()));
        }
    }
    if opts.toolchain_forbidden {
        if let Some(tc) = &gomod.toolchain {
            pending.push((line_pos(tc.line), REASON_TOOLCHAIN.to_string()));
        }
    }
    if opts.tool_forbidden {
        for tool in &gomod.tools {
            pending.push((line_pos(tool.line), REASON_TOOL.to_string()));
        }
    }
    if opts.go_debug_forbidden {
        for dbg in &gomod.godebugs {
            pending.push((line_pos(dbg.line), REASON_GODEBUG.to_string()));
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gomoddirectives",
        doc: "Manage the use of 'replace', 'retract', and 'excludes' directives in go.mod.",
        url: "https://github.com/ldez/gomoddirectives",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
