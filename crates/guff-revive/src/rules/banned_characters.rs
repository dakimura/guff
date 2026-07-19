//! `banned-characters` — warn when identifiers contain banned substrings.

use guff::ast::Ident;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let banned = config::banned_characters(pass);
    if banned.is_empty() {
        return Vec::new();
    }
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::Ident(id)) = n else {
                return true;
            };
            for ch in &banned {
                if id.name.contains(ch.as_str()) {
                    failures.push(Failure {
                        rule: "banned-characters",
                        pos: id.name_pos.0 as u32,
                        message: format!("banned character found: {ch}"),
            confidence: None,
        });
                }
            }
            true
        });
    }
    failures
}

#[allow(unused_imports)]
use guff::ast::Ident as _;
