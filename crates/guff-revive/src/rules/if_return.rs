//! `if-return` — warn on redundant `if err != nil { return err }; return nil`.

use guff::ast::{AssignStmt, BlockStmt, Expr, Ident, IfStmt, ReturnStmt, Stmt};
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
                    let NodeRef::BlockStmt(block) = n else { return; };
                    check_block(block, &mut self.failures);
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


fn check_block(block: &BlockStmt, failures: &mut Vec<Failure>) {
    let stmts = &block.list;
    for i in 0..stmts.len().saturating_sub(1) {
        let Stmt::IfStmt(if_stmt) = &stmts[i] else {
            continue;
        };
        if let Some(msg) = check_if_return(if_stmt, &stmts[i + 1]) {
            failures.push(Failure {
                rule: "if-return",
                pos: if_stmt.if_.0 as u32,
                message: msg,
            confidence: None,
        });
        }
    }
}

fn check_if_return(if_stmt: &IfStmt, next: &Stmt) -> Option<String> {
    if if_stmt.else_.is_some() || if_stmt.body.list.len() != 1 {
        return None;
    }
    let Stmt::AssignStmt(assign) = if_stmt.init.as_deref()? else {
        return None;
    };
    if assign.lhs.len() != 1 {
        return None;
    }
    if !matches!(assign.tok, Some(Token::DEFINE) | Some(Token::ASSIGN)) {
        return None;
    }
    let Expr::Ident(id) = unparen(&assign.lhs[0]) else {
        return None;
    };
    let Expr::BinaryExpr(cond) = unparen(&if_stmt.cond) else {
        return None;
    };
    if cond.op != Token::NEQ {
        return None;
    }
    let Expr::Ident(lhs) = unparen(&cond.x) else {
        return None;
    };
    if lhs.name != id.name {
        return None;
    }
    if !matches!(unparen(&cond.y), Expr::Ident(Ident { name, .. }) if name == "nil") {
        return None;
    }
    let Stmt::ReturnStmt(ret) = if_stmt.body.list.first()? else {
        return None;
    };
    if ret.results.len() != 1 {
        return None;
    }
    let Expr::Ident(ret_id) = unparen(&ret.results[0]) else {
        return None;
    };
    if ret_id.name != id.name {
        return None;
    }
    let Stmt::ReturnStmt(next_ret) = next else {
        return None;
    };
    if next_ret.results.len() != 1 {
        return None;
    }
    if !matches!(unparen(&next_ret.results[0]), Expr::Ident(Ident { name, .. }) if name == "nil") {
        return None;
    }
    Some("redundant if ...; err != nil check, just return error instead.".into())
}
