//! `revive` analyzer — runs enabled revive rules on each package.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{code, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn};

use crate::config;
use crate::rules;

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "revive",
        doc: "Fast, configurable, extensible, flexible, and beautiful linter for Go. Drop-in replacement of golint.",
        url: "https://github.com/mgechev/revive",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "revive requires inspect analyzer".to_string())?;

    let settings = config::effective_settings(pass);
    // Rules that read comments share one PARSE_COMMENTS reparse per file; the
    // cache is scoped to this package so its ASTs are dropped with it.
    crate::util::clear_reparse_cache();
    let failures = rules::run_enabled_rules(pass);
    crate::util::clear_reparse_cache();
    for failure in failures {
        if failure.confidence() < settings.confidence_threshold() {
            continue;
        }
        if settings.ignore_generated_header && code::is_generated_at(pass, failure.pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos: failure.pos,
            message: failure.format(),
            severity: config::rule_severity(pass, failure.rule),
            column: failure.column,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}
