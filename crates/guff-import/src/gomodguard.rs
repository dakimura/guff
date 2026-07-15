//! Port of [`github.com/ryancurrah/gomodguard`](https://github.com/ryancurrah/gomodguard)
//! (golangci-lint wrapper in `pkg/golinters/gomodguard`).
//!
//! Default (empty allowed/blocked, `local-replace-directives=false`) reports
//! nothing — matching golangci when settings are unset.
//!
//! Test helpers [`analyzer_block_logrus`] / [`analyzer_local_replace`] hard-code
//! common configs so fixtures work without settings wiring.
//!
//! DEFERRED: `linters.settings.gomodguard` / `gomodguard_v2` (allowed domains,
//! version constraints, recommendations, regex match types).

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::gomod::{find_gomod, is_package_in_module, parse_gomod};

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Blocked module paths (exact / prefix of import module).
    pub blocked_modules: Vec<(String, String)>, // (module, reason)
    /// When true, imports of modules with a local `replace` are blocked.
    pub local_replace_directives: bool,
}

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

fn options_default() -> Options {
    Options::default()
}

fn options_block_logrus() -> Options {
    Options {
        blocked_modules: vec![(
            "github.com/sirupsen/logrus".into(),
            "use log/slog".into(),
        )],
        local_replace_directives: false,
    }
}

fn options_local_replace() -> Options {
    Options {
        blocked_modules: Vec::new(),
        local_replace_directives: true,
    }
}

fn run_with(pass: &mut Pass<'_>, opts: &Options) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gomodguard requires inspect analyzer".to_string())?;

    // Prefer settings bag when wired; else use hardcoded options for this run.
    let opts = pass
        .settings::<Options>("gomodguard")
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

    // Module paths that are blocked for this run.
    let mut blocked: Vec<(String, String)> = Vec::new();

    for req in &gomod.requires {
        for (mod_path, reason) in &opts.blocked_modules {
            if req == mod_path || is_package_in_module(req, mod_path) {
                blocked.push((
                    req.clone(),
                    format!(
                        "import of package `{{pkg}}` is blocked because the module is in the blocked modules list. {reason}."
                    ),
                ));
            }
        }
    }

    if opts.local_replace_directives {
        for r in &gomod.replaces {
            if r.is_local() {
                blocked.push((
                    r.old_path.clone(),
                    "import of package `{pkg}` is blocked because the module has a local replace directive."
                        .into(),
                ));
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
            for (mod_path, reason_tmpl) in &blocked {
                if is_package_in_module(pkg, mod_path) {
                    let message = reason_tmpl.replace("{pkg}", pkg);
                    pending.push((imp.path.value_pos.0 as u32, message));
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
