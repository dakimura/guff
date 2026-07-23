//! `bare-return` — warn on bare returns in functions with named results.

use guff::ast::{BlockStmt, FuncDecl, FuncLit, ReturnStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

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
        match n {
            NodeRef::FuncDecl(f) => check_func(f, &mut self.failures),
            NodeRef::FuncLit(f) => check_func_lit(f, &mut self.failures),
            _ => {}
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

fn has_named_results(f: &FuncDecl) -> bool {
    f.ty
        .results
        .as_ref()
        .is_some_and(|r| r.list.first().is_some_and(|f| !f.names.is_empty()))
}

fn check_func(f: &FuncDecl, failures: &mut Vec<Failure>) {
    if !has_named_results(f) {
        return;
    }
    let Some(body) = &f.body else {
        return;
    };
    check_block(body, failures);
}

fn check_func_lit(f: &FuncLit, failures: &mut Vec<Failure>) {
    let named = f
        .ty
        .results
        .as_ref()
        .is_some_and(|r| r.list.first().is_some_and(|f| !f.names.is_empty()));
    if !named {
        return;
    }
    check_block(&f.body, failures);
}

fn check_block(block: &BlockStmt, failures: &mut Vec<Failure>) {
    for stmt in &block.list {
        check_stmt(stmt, failures);
    }
}

fn check_stmt(stmt: &Stmt, failures: &mut Vec<Failure>) {
    match stmt {
        Stmt::ReturnStmt(r) => check_return(r, failures),
        Stmt::BlockStmt(b) => check_block(b, failures),
        Stmt::IfStmt(i) => {
            check_block(&i.body, failures);
            if let Some(else_) = &i.else_ {
                check_stmt(else_, failures);
            }
        }
        Stmt::ForStmt(f) => check_block(&f.body, failures),
        Stmt::RangeStmt(r) => check_block(&r.body, failures),
        Stmt::SwitchStmt(s) => check_block(&s.body, failures),
        Stmt::TypeSwitchStmt(s) => check_block(&s.body, failures),
        Stmt::SelectStmt(s) => check_block(&s.body, failures),
        Stmt::LabeledStmt(l) => check_stmt(&l.stmt, failures),
        _ => {}
    }
}

fn check_return(ret: &ReturnStmt, failures: &mut Vec<Failure>) {
    if ret.results.is_empty() {
        failures.push(Failure {
            rule: "bare-return",
            pos: ret.return_.0 as u32,
            message: "avoid using bare returns, please add return expressions".into(),
            confidence: None,
        });
    }
}
