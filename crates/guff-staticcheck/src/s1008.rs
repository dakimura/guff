//! S1008 — simplify returning boolean expression.
//!
//! Port of `honnef.co/go/tools/simple/s1008`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, BlockStmt, Expr, IfStmt, ReturnStmt, Stmt};
use guff::ast::is_generated;
use guff::commentmap::{new_comment_map, CommentMap};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::predeclared_bool_ident;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_constant::{int64_val, Kind};

use crate::render::render_expr;

fn is_comparison_if_cond(cond: &Expr) -> bool {
    let Expr::BinaryExpr(BinaryExpr { op, .. }) = cond else {
        return true;
    };
    matches!(
        *op,
        Token::EQL | Token::LSS | Token::GTR | Token::NEQ | Token::LEQ | Token::GEQ
    )
}

fn has_comments(cm: &CommentMap<'_>, n: NodeRef<'_>) -> bool {
    let filtered = cm.filter(n);
    for group in filtered.comments() {
        for cmt in &group.list {
            if cmt.text.contains("//@ diag") {
                continue;
            }
            return true;
        }
    }
    false
}

fn is_zero_int(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    let tav = match info.types.get(&expr.id()) {
        Some(t) => t,
        None => return false,
    };
    let val = match tav.val.as_ref() {
        Some(v) if v.kind() == Kind::Int => v,
        _ => return false,
    };
    int64_val(val) == (0, true)
}

fn is_len_cap_copy_call(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    guff_analysis::code::is_call_to_any(pass, call, &["len", "cap", "copy"])
}

fn negate(pass: &Pass<'_>, expr: &Expr) -> Expr {
    match expr {
        Expr::BinaryExpr(b) => {
            let mut out = b.clone();
            out.op = match b.op {
                Token::EQL => Token::NEQ,
                Token::LSS => Token::GEQ,
                Token::GTR => {
                    if is_len_cap_copy_call(pass, &b.x) && is_zero_int(pass, &b.y) {
                        Token::EQL
                    } else {
                        Token::LEQ
                    }
                }
                Token::NEQ => Token::EQL,
                Token::LEQ => Token::GTR,
                Token::GEQ => Token::LSS,
                _ => {
                    return Expr::UnaryExpr(guff::ast::UnaryExpr {
                        op_pos: Default::default(),
                        op: Token::NOT,
                        x: Box::new(Expr::ParenExpr(guff::ast::ParenExpr {
                            lparen: Default::default(),
                            x: Box::new(expr.clone()),
                            rparen: Default::default(),
                            id: 0,
                        })),
                        id: 0,
                    });
                }
            };
            Expr::BinaryExpr(out)
        }
        Expr::Ident(_) | Expr::CallExpr(_) | Expr::IndexExpr(_) | Expr::StarExpr(_) => {
            Expr::UnaryExpr(guff::ast::UnaryExpr {
                op_pos: Default::default(),
                op: Token::NOT,
                x: Box::new(expr.clone()),
                id: 0,
            })
        }
        Expr::UnaryExpr(u) if u.op == Token::NOT => *u.x.clone(),
        other => Expr::UnaryExpr(guff::ast::UnaryExpr {
            op_pos: Default::default(),
            op: Token::NOT,
            x: Box::new(Expr::ParenExpr(guff::ast::ParenExpr {
                lparen: Default::default(),
                x: Box::new(other.clone()),
                rparen: Default::default(),
                id: 0,
            })),
            id: 0,
        }),
    }
}

fn check_if_return(
    pass: &Pass<'_>,
    if_: &IfStmt,
    ret2: &ReturnStmt,
    cm: &CommentMap<'_>,
) -> Option<String> {
    if if_.init.is_some() || if_.else_.is_some() {
        return None;
    }
    if if_.body.list.len() != 1 {
        return None;
    }
    let Stmt::ReturnStmt(ret1) = &if_.body.list[0] else {
        return None;
    };
    if ret1.results.len() != 1 || ret2.results.len() != 1 {
        return None;
    }
    let Expr::Ident(id1) = &ret1.results[0] else {
        return None;
    };
    let Expr::Ident(id2) = &ret2.results[0] else {
        return None;
    };
    let val1 = predeclared_bool_ident(pass, id1)?;
    let val2 = predeclared_bool_ident(pass, id2)?;
    if val1 == val2 {
        return None;
    }
    if !is_comparison_if_cond(&if_.cond) {
        return None;
    }

    let n1 = NodeRef::IfStmt(if_);
    let n2 = NodeRef::ReturnStmt(ret2);
    if has_comments(cm, n1) || has_comments(cm, n2) {
        return None;
    }

    let orig_cond = &if_.cond;
    let cond = if val1 {
        orig_cond.clone()
    } else {
        negate(pass, orig_cond)
    };
    let simplified = render_expr(&cond);
    let orig = render_expr(orig_cond);
    Some(format!(
        "should use 'return {simplified}' instead of 'if {orig} {{ return {} }}; return {}'",
        id1.name, id2.name
    ))
}

fn check_block(pass: &Pass<'_>, block: &BlockStmt, cm: &CommentMap<'_>) -> Option<(u32, String)> {
    let l = block.list.len();
    if l < 2 {
        return None;
    }
    if l >= 3 && matches!(block.list[l - 3], Stmt::IfStmt(_)) {
        return None;
    }
    let Stmt::IfStmt(if_) = &block.list[l - 2] else {
        return None;
    };
    let Stmt::ReturnStmt(ret2) = &block.list[l - 1] else {
        return None;
    };
    check_if_return(pass, if_, ret2, cm)
        .map(|msg| (if_.if_.0 as u32, msg))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1008 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    {
        let files = pass.files();
        for file in files {
            if is_generated(file) {
                continue;
            }
            let cm = new_comment_map(
                pass.fset(),
                guff::walk::NodeRef::File(file),
                &file.comments,
            );
            inspect.preorder_typed(node_mask!(BlockStmt), std::slice::from_ref(file), |n| {
                let NodeRef::BlockStmt(block) = n else {
                    return;
                };
                if let Some(diag) = check_block(pass, block, &cm) {
                    pending.push(diag);
                }
            });
        }
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn s1008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1008",
        doc: "simplify returning boolean expression",
        url: "https://staticcheck.dev/docs/checks/#S1008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1008 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
