//! `modifies-value-receiver` — warn on assignments to value method receivers.

use guff::ast::{AssignStmt, Expr, Ident, SelectorExpr, UnaryExpr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{type_string, unparen};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        let Some(recv) = f.recv.as_ref().and_then(|r| r.list.first()) else {
            return;
        };
        if matches!(unparen(&recv.ty.as_ref().expect("recv type")), Expr::StarExpr(_)) {
            return;
        }
        let recv_name = recv
            .names
            .first()
            .map(|id| id.name.as_str())
            .unwrap_or("_");
        if recv_name == "_" {
            return;
        }
        if let Some(typ) = recv.ty.as_ref().and_then(|e| {
            self.pass
                .types_info()
                .and_then(|info| info.types.get(&e.id()).map(|t| t.typ))
        }) {
            let ty = type_string(self.pass, typ);
            if ty.starts_with("[]") || ty.starts_with("map[") {
                return;
            }
        }
        let Some(body) = &f.body else {
            return;
        };
        if returns_receiver(recv_name, body) {
            return;
        }
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            match n {
                Some(NodeRef::AssignStmt(assign)) => {
                    if modifies_receiver(recv_name, assign) {
                        self.failures.push(Failure {
                            rule: "modifies-value-receiver",
                            pos: assign.lhs.first().map(|e| e.pos().0).unwrap_or(0) as u32,
                            message: "suspicious assignment to a by-value method receiver".into(),
                            confidence: None,
                        });
                    }
                }
                Some(NodeRef::IncDecStmt(inc)) => {
                    if receiver_selector(recv_name, &inc.x).is_some() {
                        self.failures.push(Failure {
                            rule: "modifies-value-receiver",
                            pos: inc.x.pos().0 as u32,
                            message: "suspicious assignment to a by-value method receiver".into(),
                            confidence: None,
                        });
                    }
                }
                _ => {}
            }
            true
        });
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

fn returns_receiver(recv_name: &str, body: &guff::ast::BlockStmt) -> bool {
    let mut found = false;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::ReturnStmt(ret)) = n else {
            return true;
        };
        for result in &ret.results {
            match unparen(result) {
                Expr::Ident(Ident { name, .. }) if name == recv_name => found = true,
                Expr::SelectorExpr(sel) => {
                    if receiver_name(&sel.x) == Some(recv_name) {
                        found = true;
                    }
                }
                Expr::UnaryExpr(UnaryExpr { op, x, .. })
                    if *op == Token::AND && receiver_name(x) == Some(recv_name) =>
                {
                    found = true;
                }
                _ => {}
            }
        }
        true
    });
    found
}

fn modifies_receiver(recv_name: &str, assign: &AssignStmt) -> bool {
    for lhs in &assign.lhs {
        match unparen(lhs) {
            Expr::IndexExpr(_) | Expr::StarExpr(_) => continue,
            Expr::SelectorExpr(sel) if receiver_name(&sel.x) == Some(recv_name) => return true,
            Expr::Ident(Ident { name, .. }) if name == recv_name => return true,
            _ => {}
        }
    }
    false
}

fn receiver_selector<'a>(recv_name: &str, expr: &'a Expr) -> Option<&'a SelectorExpr> {
    match unparen(expr) {
        Expr::SelectorExpr(sel) if receiver_name(&sel.x) == Some(recv_name) => Some(sel),
        _ => None,
    }
}

fn receiver_name(expr: &Expr) -> Option<&str> {
    match unparen(expr) {
        Expr::Ident(Ident { name, .. }) => Some(name.as_str()),
        _ => None,
    }
}
