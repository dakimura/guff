//! Port of [`github.com/ldez/gomoddirectives`](https://github.com/ldez/gomoddirectives)
//! (golangci-lint wrapper in `pkg/golinters/gomoddirectives`).
//!
//! Defaults match golangci-lint: replace directives are forbidden (unless listed /
//! local allowed — both off by default); retract requires a rationale comment;
//! exclude / toolchain / tool / godebug are allowed unless explicitly forbidden.
//!
//! DEFERRED: `linters.settings.gomoddirectives` (replace-local, allow-list,
//! exclude-forbidden, toolchain-pattern, …).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::gomod::{find_gomod, parse_gomod, Replace};

const REASON_REPLACE: &str = "replacement are not allowed";
const REASON_REPLACE_LOCAL: &str = "local replacement are not allowed";
const REASON_REPLACE_DUPLICATE: &str = "multiple replacement of the same module";
const REASON_REPLACE_IDENTICAL: &str = "the original module and the replacement are identical";
const REASON_RETRACT: &str = "a comment is mandatory to explain why the version has been retracted";

/// Deduplicate analysis across packages that share a module root.
fn checked_gomods() -> &'static Mutex<HashSet<String>> {
    static CHECKED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    CHECKED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn check_replace(r: &Replace) -> Option<String> {
    // Defaults: ReplaceAllowAll=false, ReplaceAllowLocal=false, empty allow-list.
    if r.is_local() {
        return Some(format!("{REASON_REPLACE_LOCAL}: {}", r.old_path));
    }
    Some(format!("{REASON_REPLACE}: {}", r.old_path))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gomoddirectives requires inspect analyzer".to_string())?;

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

    let Some(gomod) = parse_gomod(&gomod_path) else {
        return Ok(None);
    };

    let report_pos = pass
        .files()
        .first()
        .map(|f| f.package.0 as u32)
        .unwrap_or(0);

    let mut pending = Vec::new();

    let mut uniq = HashSet::new();
    for r in &gomod.replaces {
        if let Some(reason) = check_replace(r) {
            pending.push(reason);
            continue;
        }
        if r.old_path == r.new_path && r.old_version == r.new_version {
            pending.push(REASON_REPLACE_IDENTICAL.to_string());
            continue;
        }
        let key = format!("{}{}", r.old_path, r.old_version);
        if !uniq.insert(key) {
            pending.push(REASON_REPLACE_DUPLICATE.to_string());
        }
    }

    for retract in &gomod.retracts {
        if retract.rationale.is_empty() {
            pending.push(REASON_RETRACT.to_string());
        }
    }

    for message in pending {
        pass.reportf(report_pos, message);
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
