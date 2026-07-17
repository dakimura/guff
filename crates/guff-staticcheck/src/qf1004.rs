//! QF1004 — use `ReplaceAll` / `Split` instead of `Replace`/`SplitN` with `n == -1`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1004` (uses typeindex for call-site lookup).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff_analysis::code::is_call_to;
use guff_analysis::passes::{inspect, typeindex};
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
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1004 requires inspect analyzer".to_string())?;
    let index = pass
        .result_of::<typeindex::Index>(typeindex::analyzer())
        .ok_or_else(|| "QF1004 requires typeindex analyzer".to_string())?
        .clone();

    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Ok(None);
    };

    let mut pending: Vec<(u32, u32, u32, u32, String, String)> = Vec::new();
    for &(from, to) in FNS {
        let Some((path, name)) = from.rsplit_once('.') else {
            continue;
        };
        let Some(obj) = index.object(&artifacts.packages, &artifacts.scopes, path, name) else {
            continue;
        };
        index.for_each_call(obj, pass.files(), |call| {
            if call.args.len() < 2 {
                return true;
            }
            let last_i = call.args.len() - 1;
            let last = &call.args[last_i];
            if !is_minus_one(last) {
                return true;
            }
            if !is_call_to(pass, call, from) {
                return true;
            }
            let replacement = replacement_func_name(&call.fun, to);
            let prev_end = call.args[last_i - 1].end().0 as u32;
            pending.push((
                call.fun.pos().0 as u32,
                call.fun.end().0 as u32,
                prev_end,
                last.end().0 as u32,
                replacement.clone(),
                format!("could use {replacement} instead"),
            ));
            true
        });
    }

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
        requires: vec![inspect::analyzer(), typeindex::analyzer()],
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
