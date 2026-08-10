//! `string-of-int` — warn on integer-to-string conversion via `string(...)`.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_integer_type, is_string_type, type_of, unparen};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::CallExpr(call) = n else { return; };
                    check_call(self.pass, call, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
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


fn check_call(pass: &Pass<'_>, call: &CallExpr, failures: &mut Vec<Failure>) {
    let Some(fun_ty) = type_of(pass, &call.fun) else {
        return;
    };
    if !is_string_type(pass, fun_ty) {
        return;
    }
    if call.args.len() != 1 {
        return;
    }
    let Some(arg_ty) = type_of(pass, &call.args[0]) else {
        return;
    };
    if !is_integer_type(pass, arg_ty) {
        return;
    }
    // Builtin string conversion: fun is the identifier "string".
    if !matches!(unparen(&call.fun), Expr::Ident(id) if id.name == "string") {
        return;
    }
    failures.push(Failure {
        rule: "string-of-int",
        pos: call.fun.pos().0 as u32,
        message: "dubious conversion of an integer into a string, use strconv.Itoa".into(),
        ..Failure::default()
    });
}
