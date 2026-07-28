//! `waitgroup-by-value` — warn when `sync.WaitGroup` is passed by value.

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_pkg_dot_type;

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
    let Some(params) = &f.ty.params else {
        return;
    };
    for field in &params.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        if is_pkg_dot_type(ty, "sync", "WaitGroup") {
            failures.push(Failure {
                rule: "waitgroup-by-value",
                pos: ty.pos().0 as u32,
                message: "sync.WaitGroup passed by value, the function will get a copy of the original one".into(),
                confidence: None,
            });
        }
    }
}
