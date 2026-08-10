//! `unused-receiver` — warn on method receivers not referenced in the body.

use guff::ast::Ident;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank;

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
        let Some(recv) = &f.recv else {
            return;
        };
        let Some(field) = recv.list.first() else {
            return;
        };
        let Some(recv_name) = field.names.first() else {
            return;
        };
        if is_blank(recv_name) {
            return;
        }
        let Some(body) = &f.body else {
            return;
        };
        let mut used = false;
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::Ident(Ident { name, .. })) = n else {
                return true;
            };
            if name == &recv_name.name {
                used = true;
                return false;
            }
            true
        });
        if !used {
            self.failures.push(Failure {
                rule: "unused-receiver",
                pos: recv_name.name_pos.0 as u32,
                message: format!(
                    "method receiver '{}' is not referenced in method's body, consider removing or renaming it as _",
                    recv_name.name
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
