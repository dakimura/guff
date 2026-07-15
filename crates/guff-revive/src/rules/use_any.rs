//! `use-any` — suggest `any` instead of empty `interface{}`.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::InterfaceType(it)) = n else {
                return true;
            };
            if !it.methods.list.is_empty() {
                return true;
            }
            failures.push(Failure {
                rule: "use-any",
                pos: it.interface_.0 as u32,
                message: "since Go 1.18 'interface{}' can be replaced by 'any'".into(),
            });
            true
        });
    }
    failures
}
