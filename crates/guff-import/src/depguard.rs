//! Port of [`github.com/OpenPeeDeeP/depguard/v2`](https://github.com/OpenPeeDeeP/depguard)
//! (golangci-lint wrapper in `pkg/golinters/depguard`).
//!
//! Default (no custom rules) matches upstream: only allow `$gostd` in all files
//! under the list name `Main`.
//!
//! DEFERRED: `linters.settings.depguard` (custom rules / list-mode / file globs /
//! deny suggestions wiring via SettingsBag).

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

/// Top-level GOROOT/src directory names (`$gostd` expander).
fn gostd_prefixes() -> &'static [String] {
    static PREFIXES: OnceLock<Vec<String>> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let goroot = find_goroot();
        let root = Path::new(&goroot).join("src");
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out.sort();
        out
    })
}

fn find_goroot() -> String {
    if let Ok(env) = std::env::var("GOROOT") {
        if !env.is_empty() {
            return env;
        }
    }
    if let Ok(out) = Command::new("go").args(["env", "GOROOT"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}

/// Prefix-list match from depguard `strInPrefixList`.
fn prefix_match(imp: &str, prefixes: &[String]) -> Option<usize> {
    // Binary-search the first prefix that would sort after `imp`, then step back.
    let idx = prefixes
        .binary_search_by(|p| {
            let key = p.trim_end_matches('$');
            if key > imp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        })
        .unwrap_or_else(|i| i);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    let ioc = &prefixes[i];
    if let Some(exact) = ioc.strip_suffix('$') {
        return if imp == exact { Some(i) } else { None };
    }
    // GOROOT top-level name (no `.`/`/`): reject domain-looking imports.
    if !ioc.contains('.') && !ioc.contains('/') && imp.contains('.') {
        return None;
    }
    if imp.starts_with(ioc.as_str()) {
        // Require a path boundary unless exact.
        if imp.len() == ioc.len() || imp.as_bytes().get(ioc.len()) == Some(&b'/') {
            return Some(i);
        }
        // Upstream uses plain HasPrefix — `"os"` matches `"os/exec"` AND also
        // a hypothetical `"osfoo"`. Keep HasPrefix for parity.
        return Some(i);
    }
    None
}

fn import_allowed(imp: &str, allow: &[String]) -> bool {
    // listModeOriginal with allow=$gostd, empty deny:
    // allowed = (in allow) && !inDenied
    prefix_match(imp, allow).is_some()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "depguard requires inspect analyzer".to_string())?;

    let allow = gostd_prefixes();
    let mut pending = Vec::new();

    for file in pass.files() {
        let filename = pass.fset().position(file.pos()).filename;
        let filename = filename.replace('\\', "/");
        // Default files=$all → **/*.go
        if !filename.ends_with(".go") && !filename.is_empty() {
            continue;
        }

        for imp in &file.imports {
            let path = unquote_import(&imp.path.value);
            if !import_allowed(path, allow) {
                pending.push((
                    imp.path.value_pos.0 as u32,
                    format!("import '{path}' is not allowed from list 'Main'"),
                ));
            }
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
        name: "depguard",
        doc: "Go linter that checks if package imports are in a list of acceptable packages",
        url: "https://github.com/OpenPeeDeeP/depguard",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
