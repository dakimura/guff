//! `banned-characters` — warn when identifiers contain banned substrings.

use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub struct Checker {
    banned: Vec<String>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let banned = config::banned_characters(pass);
        if banned.is_empty() {
            return None;
        }
        Some(Self {
            banned,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::Ident(id) = n else {
            return;
        };
        for ch in &self.banned {
            if id.name.contains(ch.as_str()) {
                self.failures.push(Failure {
                    rule: "banned-characters",
                    pos: id.name_pos.0 as u32,
                    message: format!("banned character found: {ch}"),
                    ..Failure::default()
                });
            }
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let Some(mut c) = Checker::try_new(pass) else {
        return Vec::new();
    };
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
