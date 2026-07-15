//! `use-fmt-print` — suggest `fmt.Fprint*` instead of built-in `print`/`println`.

use guff::ast::{CallExpr, Decl, Expr, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;
use crate::util::unparen;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut redefines_print = false;
    let mut redefines_println = false;
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if f.recv.is_some() {
                continue;
            }
            match f.name.name.as_str() {
                "print" => redefines_print = true,
                "println" => redefines_println = true,
                _ => {}
            }
        }
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::CallExpr(call)) = n else {
                return true;
            };
            check_call(call, redefines_print, redefines_println, &mut failures);
            true
        });
    }
    failures
}

fn check_call(
    call: &CallExpr,
    redefines_print: bool,
    redefines_println: bool,
    failures: &mut Vec<Failure>,
) {
    let Expr::Ident(Ident { name, .. }) = unparen(&call.fun) else {
        return;
    };
    let builtin = match name.as_str() {
        "print" if !redefines_print => "print",
        "println" if !redefines_println => "println",
        _ => return,
    };
    let args = call
        .args
        .iter()
        .map(expr_fmt)
        .collect::<Vec<_>>()
        .join(", ");
    failures.push(Failure {
        rule: "use-fmt-print",
        pos: call.fun.pos().0 as u32,
        message: format!(
            "avoid using built-in function \"{builtin}\", replace it by \"fmt.F{builtin}(os.Stderr, {args})\""
        ),
    });
}
