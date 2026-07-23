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

pub struct Checker {
    style: MapStyle,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn try_new(pass: &Pass<'_>) -> Option<Self> {
        let style = map_style(pass);
        if style == MapStyle::Any {
            return None;
        }
        Some(Self {
            style,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::CompositeLit(lit) if self.style == MapStyle::Make => {
                if lit.ty.as_deref().is_some_and(is_map_type) && lit.elts.is_empty() {
                    self.failures.push(Failure {
                        rule: "enforce-map-style",
                        pos: lit.lbrace.0 as u32,
                        message: "use make(map[type]type) instead of map[type]type{}".into(),
                        confidence: None,
                    });
                }
            }
            NodeRef::CallExpr(call) if self.style == MapStyle::Literal => {
                if is_ident(&call.fun, "make")
                    && call.args.len() == 1
                    && is_map_type(&call.args[0])
                {
                    self.failures.push(Failure {
                        rule: "enforce-map-style",
                        pos: call.args[0].pos().0 as u32,
                        message: "use map[type]type{} instead of make(map[type]type)".into(),
                        confidence: None,
                    });
                }
            }
            _ => {}
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
