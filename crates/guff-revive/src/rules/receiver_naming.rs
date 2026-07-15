//! `receiver-naming` — receiver names should be short and consistent.

use guff::ast::{Decl, FuncDecl};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank, receiver_type_key};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    let mut type_receiver = std::collections::HashMap::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_method(f, &mut type_receiver, &mut failures);
        }
    }
    failures
}

fn check_method(
    f: &FuncDecl,
    type_receiver: &mut std::collections::HashMap<String, String>,
    failures: &mut Vec<Failure>,
) {
    let Some(recv) = &f.recv else {
        return;
    };
    let Some(field) = recv.list.first() else {
        return;
    };
    let Some(name) = field.names.first() else {
        return;
    };
    let pos = name.name_pos.0 as u32;

    if is_blank(name) {
        failures.push(Failure {
            rule: "receiver-naming",
            pos,
            message: "receiver name should not be an underscore, omit the name if it is unused"
                .into(),
        });
        return;
    }

    if name.name == "this" || name.name == "self" {
        failures.push(Failure {
            rule: "receiver-naming",
            pos,
            message: "receiver name should be a reflection of its identity; don't use generic names such as \"this\" or \"self\"".into(),
        });
        return;
    }

    let Some(recv_ty) = field.ty.as_ref() else {
        return;
    };
    let recv_type = receiver_type_key(recv_ty);
    if let Some(prev) = type_receiver.get(&recv_type) {
        if prev != &name.name {
            failures.push(Failure {
                rule: "receiver-naming",
                pos,
                message: format!(
                    "receiver name {} should be consistent with previous receiver name {} for {}",
                    name.name, prev, recv_type
                ),
            });
        }
        return;
    }
    type_receiver.insert(recv_type, name.name.clone());
}
