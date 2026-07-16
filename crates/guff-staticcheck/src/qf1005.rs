//! QF1005 — expand call to `math.Pow`.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1005`.
//!
//! DEFERRED: `types.CheckExpr`-based `float64(...)` wrap when the base is a
//! non-float64 constant expression (upstream `needConversion`).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::NodeRef;
use guff_analysis::code::is_call_to;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_constant::{int64_val, to_int, Kind};

use crate::render::render_expr;

fn may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => true,
        Expr::UnaryExpr(u) => may_have_side_effects(&u.x),
        Expr::BinaryExpr(b) => may_have_side_effects(&b.x) || may_have_side_effects(&b.y),
        Expr::IndexExpr(i) => may_have_side_effects(&i.x) || may_have_side_effects(&i.index),
        Expr::SelectorExpr(s) => may_have_side_effects(&s.x),
        Expr::StarExpr(s) => may_have_side_effects(&s.x),
        Expr::ParenExpr(p) => may_have_side_effects(&p.x),
        Expr::SliceExpr(s) => {
            may_have_side_effects(&s.x)
                || s.low.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.high.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.max.as_ref().is_some_and(|e| may_have_side_effects(e))
        }
        _ => false,
    }
}

fn render_factor(expr: &Expr) -> String {
    match expr {
        Expr::BinaryExpr(_) | Expr::UnaryExpr(_) => format!("({})", render_expr(expr)),
        _ => render_expr(expr),
    }
}

fn expand_pow(x: &Expr, n: i64) -> Option<String> {
    match n {
        0 => Some("1.0".into()),
        1 => Some(render_expr(x)),
        2 => {
            let f = render_factor(x);
            Some(format!("{f} * {f}"))
        }
        3 => {
            let f = render_factor(x);
            Some(format!("{f} * {f} * {f}"))
        }
        _ => None,
    }
}

fn pow_exponent(pass: &Pass<'_>, expr: &Expr) -> Option<i64> {
    // Prefer AST integer literal (avoids float conversion of typed call args).
    if let Expr::BasicLit(lit) = expr {
        if lit.kind == Some(guff::token::Token::INT) {
            return lit.value.parse().ok();
        }
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let val = tav.val.as_ref()?;
    let as_int = to_int(val.clone());
    if as_int.kind() == Kind::Unknown {
        return None;
    }
    let (n, exact) = int64_val(&as_int);
    exact.then_some(n)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to(pass, call, "math.Pow") {
            return;
        }
        if call.args.len() != 2 {
            return;
        }
        let x = &call.args[0];
        let Some(n) = pow_exponent(pass, &call.args[1]) else {
            return;
        };
        if n >= 2 && may_have_side_effects(x) {
            return;
        }
        let Some(replacement) = expand_pow(x, n) else {
            return;
        };
        pending.push((
            call.pos().0 as u32,
            call.end().0 as u32,
            replacement,
        ));
    });

    for (pos, end, replacement) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: "could expand call to math.Pow".into(),
            suggested_fixes: vec![SuggestedFix {
                message: "Expand call to math.Pow".into(),
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

fn qf1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1005",
        doc: "expand call to math.Pow",
        url: "https://staticcheck.dev/docs/checks/#QF1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
