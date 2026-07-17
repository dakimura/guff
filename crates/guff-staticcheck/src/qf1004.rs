//! QF1004 — use `ReplaceAll` / `Split` instead of `Replace`/`SplitN` with `n == -1`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1004` (without typeindex; uses call-name matching).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_call_to;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_expr;

const FNS: &[(&str, &str)] = &[
    ("strings.Replace", "strings.ReplaceAll"),
    ("strings.SplitN", "strings.Split"),
    ("strings.SplitAfterN", "strings.SplitAfter"),
    ("bytes.Replace", "bytes.ReplaceAll"),
    ("bytes.SplitN", "bytes.Split"),
    ("bytes.SplitAfterN", "bytes.SplitAfter"),
];

/// Build a replacement like `s.ReplaceAll` when the call uses a renamed import.
fn replacement_func_name(call_fun: &Expr, canonical_to: &str) -> String {
    let new_method = canonical_to
        .rsplit('.')
        .next()
        .unwrap_or(canonical_to);
    match call_fun {
        Expr::SelectorExpr(sel) => format!("{}.{}", render_expr(&sel.x), new_method),
        _ => canonical_to.into(),
    }
}

fn is_minus_one(expr: &Expr) -> bool {
    match expr {
        Expr::UnaryExpr(u) if u.op == Token::SUB => matches!(
            &*u.x,
            Expr::BasicLit(lit) if lit.value == "1"
        ),
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, u32, u32, String, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if call.args.len() < 2 {
            return;
        }
        let last_i = call.args.len() - 1;
        let last = &call.args[last_i];
        if !is_minus_one(last) {
            return;
        }
        let prev_end = call.args[last_i - 1].end().0 as u32;
        for &(from, to) in FNS {
            if !is_call_to(pass, call, from) {
                continue;
            }
            let replacement = replacement_func_name(&call.fun, to);
            // Delete from end of previous arg through `-1` so the comma is removed too
            // (upstream only deletes the unary node; we avoid a trailing-comma syntax error).
            pending.push((
                call.fun.pos().0 as u32,
                call.fun.end().0 as u32,
                prev_end,
                last.end().0 as u32,
                replacement.clone(),
                format!("could use {replacement} instead"),
            ));
            break;
        }
    });

    for (fun_pos, fun_end, del_pos, del_end, replacement, message) in pending {
        pass.report(Diagnostic {
            pos: fun_pos,
            end: fun_end,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: format!("Use {replacement} instead"),
                text_edits: vec![
                    TextEdit {
                        pos: fun_pos,
                        end: fun_end,
                        new_text: replacement,
                    },
                    TextEdit {
                        pos: del_pos,
                        end: del_end,
                        new_text: String::new(),
                    },
                ],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1004",
        doc: "use strings.ReplaceAll instead of strings.Replace with n == -1",
        url: "https://staticcheck.dev/docs/checks/#QF1004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
