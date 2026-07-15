//! `context-keys-type` — disallow basic types as `context.WithValue` keys.

use guff::ast::CallExpr;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, type_of};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            check_call(pass, call, &mut failures);
            true
        });
    }
    failures
}

fn check_call(pass: &Pass<'_>, call: &CallExpr, failures: &mut Vec<Failure>) {
    if !is_pkg_dot_name(&call.fun, "context", "WithValue") || call.args.len() != 3 {
        return;
    }
    let Some(typ) = type_of(pass, &call.args[1]) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let u = typ.underlying(&artifacts.types);
    let TypeData::Basic(b) = artifacts.types.get(u) else {
        return;
    };
    if b.kind() == BasicKind::Invalid {
        return;
    }
    failures.push(Failure {
        rule: "context-keys-type",
        pos: call.args[1].pos().0 as u32,
        message: format!(
            "should not use basic type {} as key in context.WithValue",
            crate::util::type_string(pass, typ)
        ),
    });
}
