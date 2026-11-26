//! `nested-structs` — warn on struct types nested inside other structs.

use guff::ast::{ArrayType, Expr, StructType, TypeSpec};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::unparen;

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
                    let NodeRef::StructType(st) = n else { return; };
                    for field in &st.fields.list {
                        if let Some(ty) = &field.ty {
                            check_field_type(ty, &mut self.failures);
                        }
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


fn check_field_type(ty: &Expr, failures: &mut Vec<Failure>) {
    match unparen(ty) {
        Expr::StructType(st) => {
            failures.push(Failure {
                rule: "nested-structs",
                pos: st.struct_.0 as u32,
                message: "no nested structs are allowed".into(),
                ..Failure::default()
            });
        }
        Expr::ArrayType(ArrayType { elt, .. }) => {
            if matches!(unparen(elt), Expr::StructType(_)) {
                failures.push(Failure {
                    rule: "nested-structs",
                    pos: elt.pos().0 as u32,
                    message: "no nested structs are allowed".into(),
                    ..Failure::default()
                });
            }
        }
        _ => {}
    }
}
