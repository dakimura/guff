//! S1004 — replace `bytes.Compare` with `bytes.Equal`.
//!
//! Port of `honnef.co/go/tools/simple/s1004`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{self, is_call_to, is_integer_literal};
use guff_analysis::passes::inspect;
use crate::render::{render_expr, render_node};
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

fn check_compare(pass: &Pass<'_>, expr: &BinaryExpr) -> Option<(String, String)> {
    let Expr::CallExpr(call) = &*expr.x else {
        return None;
    };
    if !is_call_to(pass, call, "bytes.Compare") {
        return None;
    }
    if !is_integer_literal(pass, &expr.y, 0) {
        return None;
    }
    let prefix = match expr.op {
        Token::NEQ => "!",
        Token::EQL => "",
        _ => return None,
    };
    // Upstream renders the rewritten call, so the message carries the real
    // arguments. The package is always spelled `bytes` even when the file
    // imports it under an alias (verified against golangci-lint 2.12.2).
    let args = call
        .args
        .iter()
        .map(render_expr)
        .collect::<Vec<_>>()
        .join(", ");
    // The message keeps the renderer it has always used; the fix goes through
    // `format.Node` because upstream's `ReplaceWithPattern` prints that way and
    // this text lands on disk. They agree on ordinary arguments.
    let fix_args = call
        .args
        .iter()
        .map(|a| render_node(pass, a).unwrap_or_else(|| render_expr(a)))
        .collect::<Vec<_>>()
        .join(", ");
    Some((
        format!("should use {prefix}bytes.Equal({args}) instead"),
        format!("{prefix}bytes.Equal({fix_args})"),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg == "bytes" || pkg == "bytes_test" {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, TextEdit)> = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(expr) = node else {
            return;
        };
        if let Some((msg, replacement)) = check_compare(pass, expr) {
            pending.push((
                expr.x.pos().0 as u32,
                msg,
                TextEdit {
                    pos: expr.x.pos().0 as u32,
                    end: expr.y.end().0 as u32,
                    new_text: replacement,
                },
            ));
        }
    });
    for (pos, message, edit) in pending {
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Simplify use of bytes.Compare".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1004",
        doc: "replace call to bytes.Compare with bytes.Equal",
        url: "https://staticcheck.dev/docs/checks/#S1004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
