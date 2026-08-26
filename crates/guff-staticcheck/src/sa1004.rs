//! SA1004 — suspiciously small untyped constant in `time.Sleep`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1004`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_int, is_call_to, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass, SuggestedFix, TextEdit};

fn check_sleep(n: i64) -> bool {
    n != 0 && n <= 120
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String, String)> = Vec::new();
    // The literal's own text is what both replacements are built from —
    // upstream binds `duration` to `lit` itself in either pattern.
    {
        let files = pass.files();
        inspect.preorder_typed(node_mask!(CallExpr), files, |n| {
            let NodeRef::CallExpr(call) = n else {
                return;
            };
            if !is_call_to(pass, call, "time.Sleep") {
                return;
            }
            // Upstream's pattern is `(CallExpr (Symbol "time.Sleep")
            // lit@(IntegerLiteral value))`, and `pattern.match` strips the
            // parentheses before binding `lit` — so `time.Sleep((42))` matches
            // and is reported at the `42`, not at the `(`.
            let Some(arg) = call.args.first().map(unparen) else {
                return;
            };
            if !matches!(arg, Expr::BasicLit(_)) {
                return;
            };
            let Some(n) = expr_to_int(pass, arg) else {
                return;
            };
            if !check_sleep(n) {
                return;
            }
            let lit = match arg {
                Expr::BasicLit(b) => b,
                _ => return,
            };
            pending.push((
                lit.value_pos.0 as u32,
                lit.end().0 as u32,
                format!("sleeping for {n} nanoseconds is probably a bug; be explicit if it isn't"),
                lit.value.clone(),
            ));
        });
    }
    for (pos, end, message, duration) in pending {
        // Upstream offers *two* fixes over the same span — nanoseconds and
        // seconds — and that is load-bearing, not cosmetic. golangci's fixer
        // gathers the edits of every `SuggestedFix` on an issue into one list
        // per linter, so two fixes for one literal overlap, and the conflict
        // pass drops every staticcheck edit for that file. Emitting only one of
        // them made guff rewrite a file upstream leaves alone.
        let unit_fix = |unit: &str, label: &str| SuggestedFix {
            message: label.to_string(),
            text_edits: vec![TextEdit {
                pos,
                end,
                new_text: format!("{duration} * time.{unit}"),
            }],
        };
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes: vec![
                unit_fix("Nanosecond", "Explicitly use nanoseconds"),
                unit_fix("Second", "Use seconds"),
            ],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn sa1004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1004",
        doc: "suspiciously small untyped constant in time.Sleep",
        url: "https://staticcheck.dev/docs/checks/#SA1004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1004 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn check_sleep_bounds() {
        assert!(!check_sleep(0));
        assert!(check_sleep(1));
        assert!(check_sleep(120));
        assert!(!check_sleep(121));
    }
}
