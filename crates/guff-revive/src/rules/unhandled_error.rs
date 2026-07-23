//! `unhandled-error` — warn on unhandled errors returned by function calls.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_analysis::code::{call_name, type_with_name};
use guff_types::arena::TypeData;
use guff_types::TypeId;

use crate::failure::Failure;
use crate::util::type_of;

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if pass.types_info().is_none() {
            return None;
        }
        Some(Self {
            pass,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::ExprStmt(stmt) = n else {
            return;
        };
        let Expr::CallExpr(call) = &stmt.x else {
            return;
        };
        if returns_error(self.pass, call) {
            let name = call_name(self.pass, &call.fun).unwrap_or_else(|| "<unknown>".into());
            self.failures.push(Failure {
                rule: "unhandled-error",
                pos: call.fun.pos().0 as u32,
                message: format!("Unhandled error in call to function {name}"),
                confidence: None,
            });
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

fn returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(typ) = type_of(pass, &Expr::CallExpr(call.clone())) else {
        return false;
    };
    match result_type_errors(pass, typ) {
        Some(flags) => flags.iter().any(|&b| b),
        None => false,
    }
}

fn result_type_errors(pass: &Pass<'_>, typ: TypeId) -> Option<Vec<bool>> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.types.get(typ) {
        TypeData::Tuple(t) => Some(
            (0..t.len())
                .map(|i| {
                    t.at(i)
                        .typ(&artifacts.objects)
                        .is_some_and(|rt| type_with_name(pass, rt, "error"))
                })
                .collect(),
        ),
        _ => Some(vec![type_with_name(pass, typ, "error")]),
    }
}
