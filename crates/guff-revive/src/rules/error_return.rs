//! `error-return` — `error` should be the last return type.

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_error_ident_type;

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
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_func(f, &mut self.failures);
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

fn check_func(f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(results) = &f.ty.results else {
        return;
    };
    if results.list.len() <= 1 {
        return;
    }
    let last = results.list.last().expect("checked len");
    if last.ty.as_ref().is_some_and(|t| is_error_ident_type(t)) {
        return;
    }
    for r in &results.list[..results.list.len() - 1] {
        if r.ty.as_ref().is_some_and(|t| is_error_ident_type(t)) {
            failures.push(Failure {
                rule: "error-return",
                pos: f.ty.func.0 as u32,
                message: "error should be the last type when returning multiple items".into(),
                ..Failure::default()
            });
            break;
        }
    }
}
