//! SA6005 — inefficient string comparison with `strings.ToLower`/`ToUpper`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6005`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_call_to_any;
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass, Diagnostic, SuggestedFix, TextEdit};

use crate::render::render_node;

fn is_to_lower_or_upper(pass: &Pass<'_>, call: &CallExpr) -> bool {
    is_call_to_any(pass, call, &["strings.ToLower", "strings.ToUpper"])
}

fn same_to_lower_or_upper(pass: &Pass<'_>, left: &CallExpr, right: &CallExpr) -> bool {
    if !is_to_lower_or_upper(pass, left) || !is_to_lower_or_upper(pass, right) {
        return false;
    }
    guff_analysis::code::call_name(pass, &left.fun) == guff_analysis::code::call_name(pass, &right.fun)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(expr) = node else {
            return;
        };
        if expr.op != Token::EQL && expr.op != Token::NEQ {
            return;
        }
        let Expr::CallExpr(left) = expr.x.as_ref() else {
            return;
        };
        let Expr::CallExpr(right) = expr.y.as_ref() else {
            return;
        };
        if !same_to_lower_or_upper(pass, left, right) {
            return;
        }
        // The message is a constant upstream — `!=` does not make it
        // `!strings.EqualFold`. guff said that until 2026-08-27, and the
        // fixture held only the `==` spelling, so nothing measured it.
        //
        // The *fix* is where the negation goes: upstream wraps the rebuilt
        // `strings.EqualFold(a, b)` in a `!` when the operator was `!=`.
        let edit = (|| {
            let (a, b) = (left.args.first()?, right.args.first()?);
            let (at, bt) = (render_node(pass, a)?, render_node(pass, b)?);
            let bang = if expr.op == Token::NEQ { "!" } else { "" };
            Some(TextEdit {
                pos: expr.x.pos().0 as u32,
                end: expr.y.end().0 as u32,
                new_text: format!("{bang}strings.EqualFold({at}, {bt})"),
            })
        })();
        pending.push((
            match_pos(node),
            "should use strings.EqualFold instead".to_string(),
            edit,
        ));
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with strings.EqualFold".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn sa6005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6005",
        doc: "inefficient string comparison with strings.ToLower or strings.ToUpper",
        url: "https://staticcheck.dev/docs/checks/#SA6005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
