//! `cyclomatic` — restrict maximum cyclomatic complexity (default 10).

use guff::ast::{Decl, Expr, FuncDecl};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_COMPLEXITY: usize = 10;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let c = complexity(f);
            if c > MAX_COMPLEXITY {
                failures.push(Failure {
                    rule: "cyclomatic",
                    pos: f.name.name_pos.0 as u32,
                    message: format!(
                        "function {} has cyclomatic complexity {} (> max enabled {})",
                        func_name(f),
                        c,
                        MAX_COMPLEXITY
                    ),
                });
            }
        }
    }
    failures
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
