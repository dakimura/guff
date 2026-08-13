//! `unnecessary-format` — warn on formatting functions without format verbs.

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{basic_lit_string_value, is_pkg_dot_name, unparen};

struct FormatSpec {
    format_arg: usize,
    alternative: &'static str,
}

fn formatting_spec(fun: &Expr) -> Option<FormatSpec> {
    if is_pkg_dot_name(fun, "fmt", "Errorf") {
        return Some(FormatSpec {
            format_arg: 0,
            alternative: "\"errors.New\"",
        });
    }
    if is_pkg_dot_name(fun, "fmt", "Printf") {
        return Some(FormatSpec {
            format_arg: 0,
            alternative: "\"fmt.Print\" or \"fmt.Println\"",
        });
    }
    if is_pkg_dot_name(fun, "fmt", "Sprintf") {
        return Some(FormatSpec {
            format_arg: 0,
            alternative: "\"fmt.Sprint\" or just the string itself",
        });
    }
    if is_pkg_dot_name(fun, "fmt", "Fprintf") {
        return Some(FormatSpec {
            format_arg: 1,
            alternative: "\"fmt.Fprint\"",
        });
    }
    if is_pkg_dot_name(fun, "log", "Printf") {
        return Some(FormatSpec {
            format_arg: 0,
            alternative: "\"log.Print\"",
        });
    }
    None
}

/// Does printing `fun` put a parenthesis in the callee's rendered name?
fn has_parens(fun: &Expr) -> bool {
    match fun {
        Expr::ParenExpr(_) => true,
        Expr::SelectorExpr(sel) => matches!(sel.x.as_ref(), Expr::ParenExpr(_)),
        _ => false,
    }
}

fn func_label(fun: &Expr) -> String {
    match unparen(fun) {
        Expr::SelectorExpr(sel) => {
            let pkg = match unparen(&sel.x) {
                Expr::Ident(id) => id.name.clone(),
                _ => "?".into(),
            };
            format!("{pkg}.{}", sel.sel.name)
        }
        Expr::Ident(id) => id.name.clone(),
        _ => "?".into(),
    }
}

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
                    let NodeRef::CallExpr(call) = n else { return; };
                    check_call(call, &mut self.failures);
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


fn check_call(call: &CallExpr, failures: &mut Vec<Failure>) {
    if call.args.is_empty() {
        return;
    }
    // Upstream keys the table on `astutils.GoFmt(ce.Fun)` — the *printed*
    // callee — so a parenthesis anywhere in it (`(fmt.Errorf)(…)`,
    // `(fmt).Errorf(…)`) renders to something that is not a key and the rule
    // stays silent. And `astutils.IsStringLiteral` is a bare
    // `e.(*ast.BasicLit)` with no unwrapping, so a parenthesized format string
    // is not a string literal to it either.
    //
    // This is the mirror image of the staticcheck rules in the same session:
    // honnef matches through `pattern`, which strips parentheses at every
    // level, and revive asserts, which never does. The polarity has to be read
    // off the upstream matcher per linter — guff had it the staticcheck way
    // here, and reported `fmt.Errorf(("clean error"))` where upstream does not
    // (`compat/fuzz.py --allow-dirty-seeds --case revive`, COMPAT-HARDENING §4
    // 2026-08-13).
    if has_parens(&call.fun) {
        return;
    }
    let Some(spec) = formatting_spec(&call.fun) else {
        return;
    };
    if call.args.len() <= spec.format_arg {
        return;
    }
    let Expr::BasicLit(lit) = &call.args[spec.format_arg] else {
        return;
    };
    let Some(format) = basic_lit_string_value(lit) else {
        return;
    };
    if format.contains('%') {
        return;
    }
    let func_name = func_label(&call.fun);
    failures.push(Failure {
        rule: "unnecessary-format",
        pos: call.fun.pos().0 as u32,
        message: format!(
            "unnecessary use of formatting function \"{func_name}\", you can replace it with {}",
            spec.alternative
        ),
        ..Failure::default()
    });
}
