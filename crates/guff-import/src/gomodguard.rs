//! Port of [`github.com/ryancurrah/gomodguard`](https://github.com/ryancurrah/gomodguard)
//! (golangci-lint wrapper in `pkg/golinters/gomodguard`).
//!
//! Default (empty allowed/blocked, `local-replace-directives=false`) reports
//! nothing — matching golangci when settings are unset.
//!
//! Settings: `linters.settings.gomodguard` (v1 `blocked.modules` map list +
//! `blocked.local_replace_directives`) and `gomodguard_v2` (`blocked` list +
//! `local-replace-directives`). Both keys populate the same [`GomodguardOptions`].
//!
//! Test helpers [`analyzer_block_logrus`] / [`analyzer_local_replace`] hard-code
//! common configs so fixtures work without a settings bag.
//!
//! DEFERRED: allowed modules/domains, version constraints, `match-type`
//! (`prefix` / `regex`).

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::gomod::{find_gomod, is_package_in_module, parse_gomod};
use crate::options::{BlockedModule, GomodguardOptions};

/// `processor.go`'s `blockReasonInBlockedList`. The `%s` is filled in by the
/// *second* `Sprintf` — see [`sprintf_package_name`].
const BLOCK_REASON_IN_BLOCKED_LIST: &str =
    "import of package `%s` is blocked because the module is in the blocked modules list.";

/// `processor.go`'s `blockReasonHasLocalReplaceDirective`.
const BLOCK_REASON_LOCAL_REPLACE: &str =
    "import of package `%s` is blocked because the module has a local replace directive.";

/// `BlockedModule.BlockReason` (`blocked.go`), minus the version-constraint
/// clause that opens it — version constraints are DEFERRED here, so the
/// builder always starts empty.
///
/// The recommendation list is spelled by upstream's four-arm `switch` and is
/// not a plain join: one module reads ``` `errors` is a recommended module.```,
/// two read ``` `errors` and `fmt` are recommended modules.```, and three
/// read ``` `a`, `b` and `c` are recommended modules.``` — note the comma
/// before the last-but-one but not before `and`.
fn block_reason(blocked: &BlockedModule) -> String {
    let mut sb = String::new();

    let recs = &blocked.recommendations;
    let n = recs.len();
    for (i, rec) in recs.iter().enumerate() {
        if n == 1 {
            sb.push_str(&format!("`{rec}` is a recommended module."));
        } else if i + 1 != n && i + 2 == n {
            sb.push_str(&format!("`{rec}` "));
        } else if i + 1 != n {
            sb.push_str(&format!("`{rec}`, "));
        } else {
            sb.push_str(&format!("and `{rec}` are recommended modules."));
        }
    }

    if !blocked.reason.is_empty() {
        // `strings.TrimRight(r.Reason, ".")` — every trailing dot, then one
        // is put back, so a reason that already ends in `.` is left alone.
        let reason = blocked.reason.trim_end_matches('.');
        if sb.is_empty() {
            sb.push_str(&format!("{reason}."));
        } else {
            sb.push_str(&format!(" {reason}."));
        }
    }

    sb
}

/// `isBlockedPackageFromModFile`'s `fmt.Sprintf(blockReason, packageName)`.
///
/// The block reason has already been through one `Sprintf` (`"%s %s"` over the
/// constant and `BlockReason`), so by the time it gets here it is *one* format
/// string that happens to carry the user's `reason` text inside it. Go runs it
/// anyway, with a single argument: the first verb takes the package name and
/// every verb after it renders as `%!<verb>(MISSING)`. beats blocks
/// `github.com/pkg/errors` with the reason "use `fmt.Errorf` with `%w`
/// instead", and golangci-lint 2.12.2 prints that `%w` as `%!w(MISSING)`.
///
/// This is not a general `fmt.Sprintf`: it is the one call gomodguard makes,
/// with exactly one string argument and no width, precision or flags in the
/// verbs it can meet.
fn sprintf_package_name(format: &str, pkg: &str) -> String {
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    let mut arg_used = false;
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(verb) = chars.next() else {
            // A trailing `%` is `%!(NOVERB)` in Go.
            out.push_str("%!(NOVERB)");
            break;
        };
        if verb == '%' {
            out.push('%');
            continue;
        }
        if arg_used {
            out.push_str(&format!("%!{verb}(MISSING)"));
        } else {
            arg_used = true;
            out.push_str(pkg);
        }
    }
    out
}

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

fn options_default() -> GomodguardOptions {
    GomodguardOptions::default()
}

fn options_block_logrus() -> GomodguardOptions {
    GomodguardOptions {
        blocked_modules: vec![BlockedModule {
            module: "github.com/sirupsen/logrus".into(),
            recommendations: vec!["log/slog".into()],
            reason: "use log/slog".into(),
        }],
        local_replace_directives: false,
    }
}

fn options_local_replace() -> GomodguardOptions {
    GomodguardOptions {
        blocked_modules: Vec::new(),
        local_replace_directives: true,
    }
}

fn run_with(pass: &mut Pass<'_>, opts: &GomodguardOptions) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gomodguard requires inspect analyzer".to_string())?;

    // Prefer settings bag when wired; else use hardcoded options for this run.
    let opts = pass
        .settings::<GomodguardOptions>("gomodguard")
        .cloned()
        .unwrap_or_else(|| opts.clone());

    if opts.blocked_modules.is_empty() && !opts.local_replace_directives {
        return Ok(None);
    }

    let Some(gomod_path) = find_gomod(&pass.pkg().dir) else {
        return Ok(None);
    };
    let Some(gomod) = parse_gomod(&gomod_path) else {
        return Ok(None);
    };

    // Module paths that are blocked for this run, each with the format string
    // `isBlockedPackageFromModFile` will later fill with the *package* name.
    let mut blocked: Vec<(String, String)> = Vec::new();

    for req in &gomod.requires {
        for rule in &opts.blocked_modules {
            if req == &rule.module || is_package_in_module(req, &rule.module) {
                // `fmt.Sprintf("%s %s", blockReasonInBlockedList, BlockReason())`
                // — the separator goes in even when `BlockReason` is empty, and
                // the trailing space it leaves is what the text printer trims.
                blocked.push((
                    req.clone(),
                    format!("{BLOCK_REASON_IN_BLOCKED_LIST} {}", block_reason(rule)),
                ));
            }
        }
    }

    if opts.local_replace_directives {
        for r in &gomod.replaces {
            if r.is_local() {
                blocked.push((r.old_path.clone(), BLOCK_REASON_LOCAL_REPLACE.to_string()));
            }
        }
    }

    if blocked.is_empty() {
        return Ok(None);
    }

    let mut pending = Vec::new();
    for file in pass.files() {
        for imp in &file.imports {
            let pkg = unquote_import(&imp.path.value);
            // `imports[n].Pos()` — `ast.ImportSpec.Pos()` is the *name* when the
            // spec has one, so a blank import reports at the `_`, not at the
            // path literal two columns later.
            let pos = imp
                .name
                .as_ref()
                .map(|n| n.pos().0 as u32)
                .unwrap_or(imp.path.value_pos.0 as u32);
            for (mod_path, reason_tmpl) in &blocked {
                if is_package_in_module(pkg, mod_path) {
                    let message = sprintf_package_name(reason_tmpl, pkg);
                    pending.push((pos, message));
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn run_default(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    run_with(pass, &options_default())
}

fn run_block_logrus(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    run_with(pass, &options_block_logrus())
}

fn run_local_replace(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    run_with(pass, &options_local_replace())
}

fn make_analyzer(run: RunFn) -> Analyzer {
    Analyzer {
        name: "gomodguard",
        doc: "Allow and blocklist linter for direct Go module dependencies.",
        url: "https://github.com/ryancurrah/gomodguard",
        run,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_default as RunFn))
}

/// Test helper: block `github.com/sirupsen/logrus`.
pub fn analyzer_block_logrus() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_block_logrus as RunFn))
}

/// Test helper: block imports of modules with a local `replace`.
pub fn analyzer_local_replace() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| make_analyzer(run_local_replace as RunFn))
}
