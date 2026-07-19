//! `unused-receiver` — warn on method receivers not referenced in the body.

use guff::ast::{Decl, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::is_blank;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(recv) = &f.recv else {
                continue;
            };
            let Some(field) = recv.list.first() else {
                continue;
            };
            let Some(recv_name) = field.names.first() else {
                continue;
            };
            if is_blank(recv_name) {
                continue;
            }
            let Some(body) = &f.body else {
                continue;
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
                failures.push(Failure {
                    rule: "unused-receiver",
                    pos: recv_name.name_pos.0 as u32,
                    message: format!(
                        "method receiver '{}' is not referenced in method's body, consider removing or renaming it as _",
                        recv_name.name
                    ),
            confidence: None,
        });
            }
        }
    }
    failures
}
