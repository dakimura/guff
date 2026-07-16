//! QF1003 — convert if/else-if chain to tagged switch.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1003`.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr, IfStmt, Stmt};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
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

fn body_has_non_goto_branch(body: &guff::ast::BlockStmt) -> bool {
    let mut skip = false;
    preorder(NodeRef::BlockStmt(body), |node| {
        if let NodeRef::BranchStmt(b) = node {
            if b.tok != Token::GOTO {
                skip = true;
                return false;
            }
        }
        true
    });
    skip
}

fn check_if_chain(ifstmt: &IfStmt, pending: &mut Vec<(u32, u32, String, Vec<TextEdit>)>) {
    // Map condition expr id → equality pairs.
    let mut m: HashMap<u32, Vec<&BinaryExpr>> = HashMap::new();
    let mut chain_len = 0usize;

    let mut item = Some(ifstmt);
    while let Some(cur) = item {
        if cur.init.is_some() {
            return;
        }
        if body_has_non_goto_branch(&cur.body) {
            return;
        }
        let mut pairs = Vec::new();
        if !find_switch_pairs(&cur.cond, &mut pairs) {
            return;
        }
        m.insert(cur.cond.id(), pairs);
        chain_len += 1;
        item = match cur.else_.as_deref() {
            Some(Stmt::IfStmt(els)) => Some(els),
            Some(Stmt::BlockStmt(_)) | None => None,
            Some(_) => return,
        };
    }

    let mut x: Option<&Expr> = None;
    for pairs in m.values() {
        if pairs.is_empty() {
            continue;
        }
        match x {
            None => x = Some(&pairs[0].x),
            Some(prev) if !expr_equal(prev, &pairs[0].x) => return,
            _ => {}
        }
    }
    let Some(x) = x else {
        return;
    };
    // Require at least two `if`s to avoid clutter.
    if chain_len < 2 {
        return;
    }

    let x_render = render_expr(x);
    let mut edits = vec![TextEdit {
        pos: ifstmt.if_.0 as u32,
        end: ifstmt.if_.0 as u32,
        new_text: format!("switch {x_render} {{\n"),
    }];

    let mut item = Some(ifstmt);
    while let Some(cur) = item {
        let end = match cur.else_.as_deref() {
            Some(els) => els.pos().0 as u32,
            None => cur.body.rbrace.0 as u32,
        };
        let conds: Vec<String> = m
            .get(&cur.cond.id())
            .into_iter()
            .flatten()
            .map(|b| render_expr(unparen(&b.y)))
            .collect();
        let sconds = conds.join(", ");
        edits.push(TextEdit {
            pos: cur.if_.0 as u32,
            end: cur.body.lbrace.0 as u32 + 1,
            new_text: format!("case {sconds}:"),
        });
        edits.push(TextEdit {
            pos: cur.body.rbrace.0 as u32,
            end,
            new_text: String::new(),
        });

        item = match cur.else_.as_deref() {
            Some(Stmt::IfStmt(els)) => Some(els),
            Some(Stmt::BlockStmt(els)) => {
                edits.push(TextEdit {
                    pos: els.lbrace.0 as u32,
                    end: els.lbrace.0 as u32 + 1,
                    new_text: "default:".into(),
                });
                None
            }
            _ => None,
        };
    }

    pending.push((
        ifstmt.if_.0 as u32,
        ifstmt.if_.0 as u32 + 2, // "if"
        format!("could use tagged switch on {x_render}"),
        edits,
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1003 requires inspect analyzer".to_string())?
        .clone();

    // Skip ifs that are else-if continuations (only process chain roots).
    let mut else_ifs = HashSet::new();
    inspect.preorder(pass.files(), |node| {
        if let NodeRef::IfStmt(i) = node {
            if let Some(Stmt::IfStmt(els)) = i.else_.as_deref() {
                else_ifs.insert(els.id);
            }
        }
    });

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        if let NodeRef::IfStmt(i) = node {
            if else_ifs.contains(&i.id) {
                return;
            }
            check_if_chain(i, &mut pending);
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

fn qf1003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1003",
        doc: "convert if/else-if chain to tagged switch",
        url: "https://staticcheck.dev/docs/checks/#QF1003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
