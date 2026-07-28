//! QF1002 — convert untagged switch to tagged switch.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1002`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CaseClause, Expr, Stmt, SwitchStmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_expr;

fn unparen(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen(&p.x),
        _ => expr,
    }
}

fn expr_equal(a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name,
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.value == y.value && x.kind == y.kind,
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && expr_equal(&x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            expr_equal(&x.x, &y.x) && expr_equal(&x.index, &y.index)
        }
        (Expr::ParenExpr(x), other) | (other, Expr::ParenExpr(x)) => expr_equal(&x.x, other),
        (Expr::UnaryExpr(x), Expr::UnaryExpr(y)) if x.op == y.op => expr_equal(&x.x, &y.x),
        (Expr::BinaryExpr(x), Expr::BinaryExpr(y)) if x.op == y.op => {
            expr_equal(&x.x, &y.x) && expr_equal(&x.y, &y.y)
        }
        (Expr::CallExpr(x), Expr::CallExpr(y)) => {
            expr_equal(&x.fun, &y.fun)
                && x.args.len() == y.args.len()
                && x.args.iter().zip(&y.args).all(|(a, b)| expr_equal(a, b))
        }
        _ => false,
    }
}

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

/// Collect `x == y` / `x == y || x == z` pairs. Returns false if the expression
/// is not a valid switch-style comparison chain.
fn find_switch_pairs<'a>(expr: &'a Expr, pairs: &mut Vec<&'a BinaryExpr>) -> bool {
    let binexpr = match unparen(expr) {
        Expr::BinaryExpr(b) => b,
        _ => return false,
    };
    match binexpr.op {
        Token::EQL => {
            if may_have_side_effects(&binexpr.x) || may_have_side_effects(&binexpr.y) {
                return false;
            }
            if !pairs.is_empty() && !expr_equal(&binexpr.x, &pairs[0].x) {
                return false;
            }
            pairs.push(binexpr);
            true
        }
        Token::LOR => {
            find_switch_pairs(&binexpr.x, pairs) && find_switch_pairs(&binexpr.y, pairs)
        }
        _ => false,
    }
}

fn check_switch(_pass: &Pass<'_>, swtch: &SwitchStmt, pending: &mut Vec<(u32, u32, String, Vec<TextEdit>)>) {
    if swtch.tag.is_some() || swtch.body.list.is_empty() {
        return;
    }

    let mut pairs: Vec<Vec<&BinaryExpr>> = Vec::with_capacity(swtch.body.list.len());
    for stmt in &swtch.body.list {
        let Stmt::CaseClause(clause) = stmt else {
            return;
        };
        let mut case_pairs = Vec::new();
        for cond in &clause.list {
            if !find_switch_pairs(cond, &mut case_pairs) {
                return;
            }
        }
        pairs.push(case_pairs);
    }

    let mut x: Option<&Expr> = None;
    for case_pairs in &pairs {
        if case_pairs.is_empty() {
            continue;
        }
        match x {
            None => x = Some(&case_pairs[0].x),
            Some(prev) if !expr_equal(prev, &case_pairs[0].x) => return,
            _ => {}
        }
    }
    let Some(x) = x else {
        // default-only switch
        return;
    };

    let x_render = render_expr(x);
    let mut edits = Vec::new();
    for (i, stmt) in swtch.body.list.iter().enumerate() {
        let Stmt::CaseClause(CaseClause { list, colon, .. }) = stmt else {
            continue;
        };
        if list.is_empty() {
            continue;
        }
        let values: Vec<String> = pairs[i]
            .iter()
            .map(|b| {
                let y = unparen(&b.y);
                render_expr(y)
            })
            .collect();
        edits.push(TextEdit {
            pos: list[0].pos().0 as u32,
            end: colon.0 as u32,
            new_text: values.join(", "),
        });
    }
    // Insert tag after `{` of the switch body.
    let lbrace = swtch.body.lbrace.0 as u32;
    edits.push(TextEdit {
        pos: lbrace,
        end: lbrace,
        new_text: format!(" {x_render}"),
    });

    pending.push((
        swtch.switch.0 as u32,
        swtch.switch.0 as u32 + 6, // "switch"
        format!("could use tagged switch on {x_render}"),
        edits,
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1002 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(SwitchStmt), pass.files(), |node| {
        if let NodeRef::SwitchStmt(swtch) = node {
            check_switch(pass, swtch, &mut pending);
        }
    });

    for (pos, end, message, text_edits) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with tagged switch".into(),
                text_edits,
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1002",
        doc: "convert untagged switch to tagged switch",
        url: "https://staticcheck.dev/docs/checks/#QF1002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
