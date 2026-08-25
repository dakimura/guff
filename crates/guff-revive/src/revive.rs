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
    // `//revive:disable[...]` comments, read from the same reparse the
    // comment-reading rules use, before the cache is dropped.
    let enabled_rules: Vec<String> = config::all_rules()
        .iter()
        .filter(|r| config::rule_enabled(pass, r))
        .map(|r| (*r).to_string())
        .collect();
    let directives = crate::directives::collect(pass, &enabled_rules);
    let failures = crate::directives::filter(pass, &directives, failures);
    crate::util::clear_reparse_cache();
    for failure in failures {
        if failure.confidence() < settings.confidence_threshold() {
            continue;
        }
        if settings.ignore_generated_header && code::is_generated_at(pass, failure.pos) {
            continue;
        }
        let message = failure.format();
        pass.report(Diagnostic {
            pos: failure.pos,
            suggested_fixes: replacement_fix(pass, &failure, &message),
            message,
            severity: config::rule_severity(pass, failure.rule),
            column: failure.column,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// A revive `ReplacementLine` as golangci-lint applies it: one edit covering
/// whole lines, from the start of the failure's first to the end of its last.
///
/// Port of `pkg/golinters/revive/revive.go`. The replacement is a line, so the
/// edit has to span lines — replacing just the node would leave whatever the
/// rule rewrote around it. The trailing newline is added here for the same
/// reason upstream adds it: `ReplacementLine` does not carry one, and the span
/// being replaced ends before the line's own newline.
fn replacement_fix(
    pass: &Pass<'_>,
    failure: &crate::failure::Failure,
    message: &str,
) -> Vec<guff_analysis::SuggestedFix> {
    let Some(line_text) = failure.replacement_line.as_ref() else {
        return Vec::new();
    };
    let fpos = guff::position::Pos(i64::from(failure.pos));
    let Some(file) = pass.fset().file(fpos) else {
        return Vec::new();
    };
    let start_line = file.line(guff::position::Pos(i64::from(failure.pos)));
    let end_pos = failure.replacement_end.unwrap_or(failure.pos);
    let end_line = file.line(guff::position::Pos(i64::from(end_pos)));
    if start_line <= 0 || end_line < start_line {
        return Vec::new();
    }
    let pos = file.line_start(start_line as usize).0 as u32;
    // The replaced span ends *past* the line's newline, which is why the
    // replacement text carries one of its own.
    //
    // Upstream spells this `EndOfLinePos` = `LineStart(line+1) - 1`, which
    // points at the newline rather than past it — but `fixer.go` casts the
    // `token.Pos` straight to a byte offset instead of resolving it through
    // `Fset.Position`, so the offset it applies is the one after. guff's
    // offsets are real, so the intent has to be written out: without this the
    // original newline survives and every fixed line gains a blank one after
    // it.
    let end = if end_line as usize >= file.line_count() {
        file.pos(file.size()).0 as u32
    } else {
        file.line_start(end_line as usize + 1).0 as u32
    };
    vec![guff_analysis::SuggestedFix {
        message: message.to_string(),
        text_edits: vec![guff_analysis::TextEdit {
            pos,
            end,
            new_text: format!("{line_text}\n"),
        }],
    }]
}
