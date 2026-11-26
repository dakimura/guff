//! `receiver-naming` — receiver names should be short and consistent.

use std::collections::HashMap;

use guff::ast::{File, FuncDecl};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank, receiver_type_key};

pub struct Checker {
    type_receiver: HashMap<String, String>,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            type_receiver: HashMap::new(),
            failures: Vec::new(),
        }
    }

    pub fn on_file(&mut self, _file: &File) {
        self.type_receiver.clear();
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_method(f, &mut self.type_receiver, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_method(
    f: &FuncDecl,
    type_receiver: &mut HashMap<String, String>,
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
    // Upstream's failures carry the FuncDecl, so every receiver-naming report
    // lands on the `func` keyword rather than on the receiver name.
    let pos = f.ty.func.0 as u32;

    if is_blank(name) {
        failures.push(Failure {
            rule: "receiver-naming",
            pos,
            message: "receiver name should not be an underscore, omit the name if it is unused"
                .into(),
                ..Failure::default()
            });
        return;
    }

    if name.name == "this" || name.name == "self" {
        failures.push(Failure {
            rule: "receiver-naming",
            pos,
            message: "receiver name should be a reflection of its identity; don't use generic names such as \"this\" or \"self\"".into(),
            ..Failure::default()
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
                ..Failure::default()
            });
        }
        return;
    }
    type_receiver.insert(recv_type, name.name.clone());
}
