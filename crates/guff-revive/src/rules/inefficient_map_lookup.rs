//! `inefficient-map-lookup` — warn on key iteration used as map lookup.

use guff::ast::{BinaryExpr, BlockStmt, BranchStmt, Expr, Ident, IfStmt, Stmt};
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
        let Some(body) = &f.body else {
            return;
        };
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::BlockStmt(block)) = n else {
                return true;
            };
            analyze_block(self.pass, block, &mut self.failures);
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

fn analyze_block(pass: &Pass<'_>, block: &BlockStmt, failures: &mut Vec<Failure>) {
    for stmt in &block.list {
        let Stmt::RangeStmt(range) = stmt else {
            continue;
        };
        let Expr::Ident(key) = unparen(range.key.as_ref().expect("range key")) else {
            continue;
        };
        let has_value = range
            .value
            .as_ref()
            .is_some_and(|v| !matches!(unparen(v), Expr::Ident(Ident { name, .. }) if name == "_"));
        if has_value {
            continue;
        }
        let is_map = pass
            .types_info()
            .and_then(|info| info.types.get(&range.x.id()))
            .map(|t| type_string(pass, t.typ).starts_with("map["))
            .unwrap_or(false);
        if !is_map {
            continue;
        }
        if is_key_lookup(&key.name, &range.body) {
            failures.push(Failure {
                rule: "inefficient-map-lookup",
                pos: range.for_.0 as u32,
                message: "inefficient lookup of map key".into(),
                confidence: None,
            });
        }
    }
}

fn is_key_lookup(key_name: &str, block: &BlockStmt) -> bool {
    let Some(first) = block.list.first() else {
        return false;
    };
    let Stmt::IfStmt(IfStmt { cond, body, .. }) = first else {
        return false;
    };
    let Expr::BinaryExpr(BinaryExpr { op, x, .. }) = unparen(cond) else {
        return false;
    };
    if !matches!(unparen(x), Expr::Ident(Ident { name, .. }) if name == key_name) {
        return false;
    }
    match op {
        Token::EQL => block.list.len() == 1,
        Token::NEQ => {
            let Some(Stmt::BranchStmt(BranchStmt { tok, .. })) = body.list.first() else {
                return false;
            };
            *tok == Token::CONTINUE
        }
        _ => false,
    }
}
