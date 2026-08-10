//! `use-fmt-print` — suggest `fmt.Fprint*` instead of built-in `print`/`println`.

use guff::ast::{CallExpr, Decl, Expr, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::astfmt::expr_fmt;
use crate::failure::Failure;
use crate::util::unparen;

pub struct Checker {
    redefines_print: bool,
    redefines_println: bool,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
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
        Self {
            redefines_print,
            redefines_println,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        check_call(
            call,
            self.redefines_print,
            self.redefines_println,
            &mut self.failures,
        );
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
        ..Failure::default()
    });
}
