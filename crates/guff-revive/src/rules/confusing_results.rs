//! `confusing-results` — warn on consecutive unnamed results of the same type.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
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
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        let Some(results) = &f.ty.results else {
            return;
        };
        if results.list.len() <= 1 {
            return;
        }
        if results.list[0].names.first().is_some() {
            return;
        }
        let mut last_type = String::new();
        for field in &results.list {
            let Some(ty_expr) = field.ty.as_ref() else {
                continue;
            };
            let ty = expr_fmt(ty_expr);
            if ty == last_type {
                self.failures.push(Failure {
                    rule: "confusing-results",
                    pos: field.pos().0 as u32,
                    message: "unnamed results of the same type may be confusing, consider using named results".into(),
                    ..Failure::default()
                });
                break;
            }
            last_type = ty;
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
