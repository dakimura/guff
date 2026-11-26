//! `argument-limit` — restrict maximum number of function parameters (default 8).

use guff::ast::FuncDecl;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_ARGUMENTS: i64 = 8;

/// `Configure`: `arguments[0]` is the limit, 8 when there is none. guff had the
/// default baked in as a constant and never read the argument, so no
/// configuration could move it.
pub fn max_arguments(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "argument-limit", 0).unwrap_or(DEFAULT_MAX_ARGUMENTS)
}

pub struct Checker {
    max: i64,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            max: max_arguments(pass),
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        let num_params = count_params(f);
        if num_params as i64 > self.max {
            self.failures.push(Failure {
                rule: "argument-limit",
                pos: f.ty.func.0 as u32,
                message: format!(
                    "maximum number of arguments per function exceeded; max {} but got {num_params}",
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
