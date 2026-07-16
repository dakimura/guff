//! QF1009 — use `time.Time.Equal` instead of `==`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1009`.

use std::sync::OnceLock;

use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_of_type_with_name;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_expr;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1009 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::BinaryExpr(expr) = node else {
            return;
        };
        if expr.op != Token::EQL {
            return;
        }
        if !is_of_type_with_name(pass, &expr.x, "time.Time")
            || !is_of_type_with_name(pass, &expr.y, "time.Time")
        {
            return;
        }
        let replacement = format!("{}.Equal({})", render_expr(&expr.x), render_expr(&expr.y));
        pending.push((
            expr.x.pos().0 as u32,
            expr.y.end().0 as u32,
            replacement,
        ));
    });

    for (pos, end, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: "probably want to use time.Time.Equal instead".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Use time.Time.Equal method".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: replacement,
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1009",
        doc: "use time.Time.Equal instead of == operator",
        url: "https://staticcheck.dev/docs/checks/#QF1009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
