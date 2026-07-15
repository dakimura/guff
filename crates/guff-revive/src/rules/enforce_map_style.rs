//! `enforce-map-style` — enforce `make(map[type]type)` or `map[type]type{}`.

use guff::ast::{CallExpr, CompositeLit, Expr, MapType};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::util::{is_ident, unparen};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MapStyle {
    Any,
    Make,
    Literal,
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let style = map_style(pass);
    if style == MapStyle::Any {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::CompositeLit(lit)) if style == MapStyle::Make => {
                    if lit.ty.as_deref().is_some_and(is_map_type) && lit.elts.is_empty() {
                        failures.push(Failure {
                            rule: "enforce-map-style",
                            pos: lit.lbrace.0 as u32,
                            message: "use make(map[type]type) instead of map[type]type{}".into(),
                        });
                    }
                }
                Some(NodeRef::CallExpr(call)) if style == MapStyle::Literal => {
                    if is_ident(&call.fun, "make")
                        && call.args.len() == 1
                        && is_map_type(&call.args[0])
                    {
                        failures.push(Failure {
                            rule: "enforce-map-style",
                            pos: call.args[0].pos().0 as u32,
                            message: "use map[type]type{} instead of make(map[type]type)".into(),
                        });
                    }
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn map_style(pass: &Pass<'_>) -> MapStyle {
    match config::rule_arg_string(pass, "enforce-map-style", 0).as_deref() {
        Some("make") => MapStyle::Make,
        Some("literal") => MapStyle::Literal,
        _ => MapStyle::Any,
    }
}

fn is_map_type(expr: &Expr) -> bool {
    matches!(unparen(expr), Expr::MapType(_))
}
