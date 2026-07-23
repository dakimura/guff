//! `errorf` — prefer `fmt.Errorf` over `errors.New(fmt.Sprintf(...))`.

use guff::ast::{CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, type_of, unparen};

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
    if call.args.len() != 1 {
        return;
    }
    let is_errors_new = is_pkg_dot_name(&call.fun, "errors", "New");
    let mut prefix = "fmt".to_string();
    let mut render_target = "errors.New".to_string();
    let is_testing_error = if let Expr::SelectorExpr(sel) = unparen(&call.fun) {
        if sel.sel.name == "Error" {
            if let Some(typ) = type_of(pass, &sel.x) {
                let s = crate::util::type_string(pass, typ);
                if s == "*testing.T" {
                    prefix = match unparen(&sel.x) {
                        Expr::Ident(id) => id.name.clone(),
                        _ => "t".into(),
                    };
                    render_target = format!("{prefix}.Error");
                    true
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };
    if !is_errors_new && !is_testing_error {
        return;
    }
    let Expr::CallExpr(inner) = unparen(&call.args[0]) else {
        return;
    };
    if !is_pkg_dot_name(&inner.fun, "fmt", "Sprintf") {
        return;
    }
    failures.push(Failure {
        rule: "errorf",
        pos: call.fun.pos().0 as u32,
        message: format!(
            "should replace {render_target}(fmt.Sprintf(...)) with {prefix}.Errorf(...)"
        ),
            confidence: None,
        });
}
