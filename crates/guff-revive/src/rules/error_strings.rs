//! `error-strings` — error strings should not be capitalized or end with punctuation.

use guff::ast::{CallExpr, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{basic_lit_string, is_pkg_dot_name, unparen};

const MESSAGE: &str =
    "error strings should not be capitalized or end with punctuation or a newline";

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
            
                    if let NodeRef::CallExpr(call) = n {
                        check_call(call, &mut self.failures);
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


fn matches_error_fn(fun: &Expr) -> bool {
    is_pkg_dot_name(fun, "fmt", "Errorf")
        || is_pkg_dot_name(fun, "errors", "New")
        || is_pkg_dot_name(fun, "errors", "Errorf")
        || is_pkg_dot_name(fun, "errors", "WithMessage")
        || is_pkg_dot_name(fun, "errors", "Wrap")
        || is_pkg_dot_name(fun, "errors", "WithMessagef")
        || is_pkg_dot_name(fun, "errors", "Wrapf")
}

fn check_call(call: &CallExpr, failures: &mut Vec<Failure>) {
    if !matches_error_fn(&call.fun) {
        return;
    }
    let msg = call
        .args
        .first()
        .and_then(|a| match unparen(a) {
            Expr::BasicLit(lit) => Some(lit),
            _ => None,
        })
        .or_else(|| {
            call.args.get(1).and_then(|a| match unparen(a) {
                Expr::BasicLit(lit) => Some(lit),
                _ => None,
            })
        });
    let Some(lit) = msg else {
        return;
    };
    if lit.kind != Some(Token::STRING) {
        return;
    }
    let Some(s) = basic_lit_string(lit) else {
        return;
    };
    if s.is_empty() {
        return;
    }
    let (clean, conf) = lint_error_string(s);
    if clean {
        return;
    }
    failures.push(Failure::with_confidence(
        "error-strings",
        lit.pos().0 as u32,
        MESSAGE,
        conf,
    ));
}

fn lint_error_string(s: &str) -> (bool, f64) {
    /// Upstream: basicConfidence = 0.8, capConfidence = 0.6.
    const BASIC: f64 = 0.8;
    const CAP: f64 = 0.6;

    let Some(last) = s.chars().last() else {
        return (true, 0.0);
    };
    if last == '.' || last == ':' || last == '!' || last == '\n' {
        return (false, BASIC);
    }
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return (true, 0.0);
    };
    if !first.is_uppercase() {
        return (true, 0.0);
    }
    for c in chars {
        if c.is_whitespace() {
            break;
        }
        if c.is_uppercase() || c.is_ascii_digit() {
            return (true, 0.0);
        }
    }
    // Capitalization-only: confidence 0.6 is below the default 0.8 threshold.
    (false, CAP)
}
