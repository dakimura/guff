//! `function-result-limit` — restrict maximum number of return results (default 3).

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_RESULTS: i64 = 3;

/// `Configure`: `arguments[0]` is the limit, 3 when there is none. A negative
/// value is a configuration error upstream, not a limit.
pub fn max_results(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "function-result-limit", 0).unwrap_or(DEFAULT_MAX_RESULTS)
}

pub struct Checker {
    max: i64,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            max: max_results(pass),
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
        if num as i64 > self.max {
            self.failures.push(Failure {
                rule: "function-result-limit",
                pos: f.ty.func.0 as u32,
                message: format!(
                    "maximum number of return results per function exceeded; max {} but got {num}",
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
