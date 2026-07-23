//! `unnecessary-if` — replace if/else with boolean expressions when possible.

use guff::ast::{AssignStmt, BinaryExpr, Expr, Ident, IfStmt, ReturnStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::unparen;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::IfStmt(if_stmt) = n else {
            return;
        };
        if let Some(msg) = check_if(if_stmt) {
            self.failures.push(Failure {
                rule: "unnecessary-if",
                pos: if_stmt.if_.0 as u32,
                message: msg,
                confidence: None,
            });
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_if(if_stmt: &IfStmt) -> Option<String> {
    if if_stmt.init.is_some() {
        return None;
    }
    let Stmt::BlockStmt(else_block) = if_stmt.else_.as_deref()? else {
        return None;
    };
    if if_stmt.body.list.len() != 1 || else_block.list.len() != 1 {
        return None;
    }
    let (replacement, then_bool) = replacement_for_then(&if_stmt.body.list[0], &else_block.list[0])?;
    let cond = cond_as_string(&if_stmt.cond, !then_bool);
    Some(format!("replace this conditional by: {replacement} {cond}"))
}

fn replacement_for_then(then_stmt: &Stmt, else_stmt: &Stmt) -> Option<(String, bool)> {
    match then_stmt {
        Stmt::ReturnStmt(then_ret) => {
            let (then_bool, ok) = single_bool_literal(&then_ret.results)?;
            if !ok {
                return None;
            }
            let Stmt::ReturnStmt(else_ret) = else_stmt else {
                return None;
            };
            single_bool_literal(&else_ret.results)?;
            Some(("return".into(), then_bool == "true"))
        }
        Stmt::AssignStmt(then_assign) => {
            let (then_bool, ok) = single_bool_literal(&then_assign.rhs)?;
            if !ok || then_assign.lhs.len() != 1 {
                return None;
            }
            let Stmt::AssignStmt(else_assign) = else_stmt else {
                return None;
            };
            single_bool_literal(&else_assign.rhs)?;
            let then_lhs = expr_fmt(&then_assign.lhs[0]);
            let else_lhs = expr_fmt(&else_assign.lhs[0]);
            if then_lhs != else_lhs {
                return None;
            }
            let tok = then_assign.tok?.as_str();
            Some((format!("{then_lhs} {tok}"), then_bool == "true"))
        }
        _ => None,
    }
}

fn single_bool_literal(exprs: &[Expr]) -> Option<(&str, bool)> {
    if exprs.len() != 1 {
        return None;
    }
    let Expr::Ident(Ident { name, .. }) = unparen(&exprs[0]) else {
        return None;
    };
    match name.as_str() {
        "true" | "false" => Some((name.as_str(), true)),
        _ => None,
    }
}

fn cond_as_string(cond: &Expr, must_negate: bool) -> String {
    if must_negate {
        if let Expr::BinaryExpr(BinaryExpr { op, x, y, .. }) = unparen(cond) {
            if let Some(opposite) = relational_opposite(*op) {
                return format!("{} {} {}", expr_fmt(x), opposite.as_str(), expr_fmt(y));
            }
        }
        format!("!({})", expr_fmt(cond))
    } else {
        expr_fmt(cond)
    }
}

fn relational_opposite(op: Token) -> Option<Token> {
    match op {
        Token::EQL => Some(Token::NEQ),
        Token::NEQ => Some(Token::EQL),
        Token::LSS => Some(Token::GEQ),
        Token::LEQ => Some(Token::GTR),
        Token::GTR => Some(Token::LEQ),
        Token::GEQ => Some(Token::LSS),
        _ => None,
    }
}

fn expr_fmt(expr: &Expr) -> String {
    match unparen(expr) {
        Expr::Ident(id) => id.name.clone(),
        Expr::BinaryExpr(b) => format!(
            "{} {} {}",
            expr_fmt(&b.x),
            b.op.as_str(),
            expr_fmt(&b.y)
        ),
        Expr::UnaryExpr(u) => format!("{}{}", u.op.as_str(), expr_fmt(&u.x)),
        Expr::ParenExpr(p) => format!("({})", expr_fmt(&p.x)),
        _ => "<expr>".into(),
    }
}
