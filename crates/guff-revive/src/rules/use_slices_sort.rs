//! `use-slices-sort` — suggest `slices` package over `sort` helpers.

use guff::ast::{CallExpr, Expr, Ident, SelectorExpr};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::unparen;

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
                    if let Some((sort_method, slices_method)) = sort_replacement(&call.fun) {
                        self.failures.push(Failure {
                            rule: "use-slices-sort",
                            pos: call.fun.pos().0 as u32,
                            message: format!("replace sort.{sort_method} by slices.{slices_method}"),
                            ..Failure::default()
                        });
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


fn sort_replacement(fun: &Expr) -> Option<(&str, &'static str)> {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return None;
    };
    if !matches!(unparen(x), Expr::Ident(Ident { name, .. }) if name == "sort") {
        return None;
    }
    let method = sel.name.as_str();
    match method {
        "Float64s" | "Ints" | "Strings" => Some((method, "Sort")),
        "Slice" | "Sort" => Some((method, "SortFunc")),
        "SliceStable" | "Stable" => Some((method, "SortStableFunc")),
        "Float64sAreSorted" | "IntsAreSorted" | "StringsAreSorted" => Some((method, "IsSorted")),
        "IsSorted" | "SliceIsSorted" => Some((method, "IsSortedFunc")),
        _ => None,
    }
}
