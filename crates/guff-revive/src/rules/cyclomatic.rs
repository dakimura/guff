//! `cyclomatic` — restrict maximum cyclomatic complexity (default 10).

use guff::ast::{Expr, FuncDecl};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_COMPLEXITY: i64 = 10;

/// `Configure`: `arguments[0]` is the limit, 10 when there is none.
pub fn max_complexity(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "cyclomatic", 0).unwrap_or(DEFAULT_MAX_COMPLEXITY)
}

pub struct Checker {
    max: i64,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            max: max_complexity(pass),
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        let c = complexity(f);
        if c as i64 > self.max {
            self.failures.push(Failure {
                rule: "cyclomatic",
                pos: f.ty.func.0 as u32,
                message: format!(
                    "function {} has cyclomatic complexity {} (> max enabled {})",
                    func_name(f),
                    c,
                    self.max
                ),
                ..Failure::default()
            });
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
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

fn recv_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_string(&s.x)),
        _ => "BADRECV".into(),
    }
}

fn func_name(f: &FuncDecl) -> String {
    if let Some(recv) = &f.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_string(ty), f.name.name);
            }
        }
    }
    f.name.name.clone()
}

fn complexity(f: &FuncDecl) -> usize {
    let mut c = 1usize;
    let Some(body) = &f.body else {
        return 0;
    };
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::FuncDecl(_)
            | NodeRef::IfStmt(_)
            | NodeRef::ForStmt(_)
            | NodeRef::RangeStmt(_)
            | NodeRef::CaseClause(_)
            | NodeRef::CommClause(_) => {
                c += 1;
            }
            NodeRef::BinaryExpr(b) if b.op == Token::LAND || b.op == Token::LOR => {
                c += 1;
            }
            _ => {}
        }
        true
    });
    c
}
