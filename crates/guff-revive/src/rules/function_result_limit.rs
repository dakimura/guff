//! `function-result-limit` — restrict maximum number of return results (default 3).

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_RESULTS: usize = 3;

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
        let num: usize = results
            .list
            .iter()
            .map(|field| {
                if field.names.is_empty() {
                    1
                } else {
                    field.names.len()
                }
            })
            .sum();
        if num > MAX_RESULTS {
            self.failures.push(Failure {
                rule: "function-result-limit",
                pos: f.name.name_pos.0 as u32,
                message: format!(
                    "maximum number of return results per function exceeded; max {MAX_RESULTS} but got {num}"
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
