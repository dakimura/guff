//! `argument-limit` — restrict maximum number of function parameters (default 8).

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_ARGUMENTS: usize = 8;

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
        let num_params = count_params(f);
        if num_params > MAX_ARGUMENTS {
            self.failures.push(Failure {
                rule: "argument-limit",
                pos: f.name.name_pos.0 as u32,
                message: format!(
                    "maximum number of arguments per function exceeded; max {MAX_ARGUMENTS} but got {num_params}"
                ),
                confidence: None,
            });
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

fn count_params(f: &FuncDecl) -> usize {
    let Some(params) = &f.ty.params else {
        return 0;
    };
    params
        .list
        .iter()
        .map(|field| {
            if field.names.is_empty() {
                1
            } else {
                field.names.len()
            }
        })
        .sum()
}
