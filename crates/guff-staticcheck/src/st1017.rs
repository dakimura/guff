//! ST1017 — don't use Yoda conditions.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1017`.

use std::sync::OnceLock;

use guff::walk::NodeRef;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::inspect;
use guff_analysis::{
    matches, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_pattern::{must_parse, Pattern};

use crate::render::render_expr;

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| {
        must_parse(
            r#"(BinaryExpr left@(TrulyConstantExpression _) tok@(Or "==" "!=") right@(Not (TrulyConstantExpression _)))"#,
        )
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1017 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        let NodeRef::BinaryExpr(bin) = node else {
            return true;
        };
        let pos = bin.x.pos().0 as u32;
        if is_generated_at(pass, pos) {
            return true;
        }
        let replacement = format!(
            "{} {} {}",
            render_expr(&bin.y),
            match bin.op {
                guff::token::Token::EQL => "==",
                guff::token::Token::NEQ => "!=",
                _ => return true,
            },
            render_expr(&bin.x)
        );
        pending.push((pos, bin.y.end().0 as u32, replacement));
        true
    });

    for (pos, end, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: "don't use Yoda conditions".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Un-Yoda-fy".into(),
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

fn st1017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1017",
        doc: "don't use Yoda conditions",
        url: "https://staticcheck.dev/docs/checks/#ST1017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
