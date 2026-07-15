//! `enforce-slice-style` — enforce `make([]type, 0)`, `[]type{}`, or `var []type`.

use guff::ast::{ArrayType, BasicLit, CallExpr, CompositeLit, Expr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::util::{is_ident, unparen};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SliceStyle {
    Any,
    Make,
    Literal,
    Nil,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let style = slice_style(pass);
    if style == SliceStyle::Any {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::CompositeLit(lit))
                    if matches!(style, SliceStyle::Make | SliceStyle::Nil) =>
                {
                    if lit.ty.as_deref().is_some_and(is_slice_type) && lit.elts.is_empty() {
                        let message = if style == SliceStyle::Nil {
                            "use nil slice declaration (e.g. var args []type) instead of []type{}"
                        } else {
                            "use make([]type) instead of []type{} (or declare nil slice)"
                        };
                        failures.push(Failure {
                            rule: "enforce-slice-style",
                            pos: lit.lbrace.0 as u32,
                            message: message.into(),
                        });
                    }
                }
                Some(NodeRef::CallExpr(call))
                    if matches!(style, SliceStyle::Literal | SliceStyle::Nil) =>
                {
                    if !is_ident(&call.fun, "make") || call.args.len() < 2 {
                        return true;
                    }
                    if !is_slice_type(&call.args[0]) {
                        return true;
                    }
                    let Expr::BasicLit(BasicLit { value, .. }) = unparen(&call.args[1]) else {
                        return true;
                    };
                    if value != "0" {
                        return true;
                    }
                    if call.args.len() > 2 {
                        let Expr::BasicLit(BasicLit { value: cap, .. }) =
                            unparen(&call.args[2])
                        else {
                            return true;
                        };
                        if cap != "0" {
                            return true;
                        }
                    }
                    let message = if style == SliceStyle::Nil {
                        "use nil slice declaration (e.g. var args []type) instead of make([]type, 0)"
                    } else {
                        "use []type{} instead of make([]type, 0) (or declare nil slice)"
                    };
                    failures.push(Failure {
                        rule: "enforce-slice-style",
                        pos: call.args[0].pos().0 as u32,
                        message: message.into(),
                    });
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn slice_style(pass: &Pass<'_>) -> SliceStyle {
    match config::rule_arg_string(pass, "enforce-slice-style", 0).as_deref() {
        Some("make") => SliceStyle::Make,
        Some("literal") => SliceStyle::Literal,
        Some("nil") => SliceStyle::Nil,
        _ => SliceStyle::Any,
    }
}

fn is_slice_type(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::ArrayType(ArrayType { len: None, .. }))
}
