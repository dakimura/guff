//! `unnecessary-stmt` — warn on unnecessary statements (bare return, break, etc.).

use guff::token::Token;
use guff::ast::{BlockStmt, CaseClause, Decl, File, FuncDecl, Stmt};
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

    pub fn on_file(&mut self, file: &File) {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(f, &mut self.failures);
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::SwitchStmt(s) => check_switch_body(&s.body, &mut self.failures),
            NodeRef::TypeSwitchStmt(s) => check_switch_body(&s.body, &mut self.failures),
            NodeRef::CaseClause(c) => check_case_clause(c, &mut self.failures),
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
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_func(f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    if f.ty.results.is_some() {
        return;
    }
    let Some(last) = body.list.last() else {
        return;
    };
    let Stmt::ReturnStmt(ret) = last else {
        return;
    };
    if ret.results.is_empty() {
        failures.push(Failure {
            rule: "unnecessary-stmt",
            pos: ret.return_.0 as u32,
            message: "omit unnecessary return statement".into(),
            ..Failure::default()
        });
    }
}

fn check_switch_body(body: &BlockStmt, failures: &mut Vec<Failure>) {
    if body.list.len() != 1 {
        return;
    }
    let Stmt::CaseClause(_) = &body.list[0] else {
        return;
    };
    failures.push(Failure {
        rule: "unnecessary-stmt",
        pos: body.lbrace.0 as u32,
        message: "switch with only one case can be replaced by an if-then".into(),
        ..Failure::default()
    });
}

fn check_case_clause(c: &CaseClause, failures: &mut Vec<Failure>) {
    if c.list.len() > 1 {
        return;
    }
    let Some(last) = c.body.last() else {
        return;
    };
    let Stmt::BranchStmt(branch) = last else {
        return;
    };
    if branch.tok == Token::BREAK && branch.label.is_none() {
        failures.push(Failure {
            rule: "unnecessary-stmt",
            pos: branch.tok_pos.0 as u32,
            message: "omit unnecessary break at the end of case clause".into(),
            ..Failure::default()
        });
    }
}
