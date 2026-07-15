//! `epoch-naming` — enforce suffixes on variables assigned from `time.Time` epoch methods.

use guff::ast::{AssignStmt, Expr, Ident, SelectorExpr, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_named_type, unparen};

const EPOCH_UNITS: &[(&str, &[&str])] = &[
    ("Unix", &["Sec", "Second", "Seconds"]),
    ("UnixMilli", &["Milli", "Ms"]),
    ("UnixMicro", &["Micro", "Microsecond", "Microseconds", "Us"]),
    ("UnixNano", &["Nano", "Ns"]),
];

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(NodeRef::ValueSpec(spec)) => {
                    for (i, name) in spec.names.iter().enumerate() {
                        if let Some(value) = spec.values.get(i) {
                            check_name(pass, name, value, &mut failures);
                        }
                    }
                }
                Some(NodeRef::AssignStmt(assign))
                    if matches!(assign.tok, Some(Token::DEFINE) | Some(Token::ASSIGN)) =>
                {
                    for (i, lhs) in assign.lhs.iter().enumerate() {
                        let Expr::Ident(name) = unparen(lhs) else {
                            continue;
                        };
                        if name.name == "_" {
                            continue;
                        }
                        if let Some(rhs) = assign.rhs.get(i) {
                            check_name(pass, name, rhs, &mut failures);
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn check_name(pass: &Pass<'_>, name: &Ident, value: &Expr, failures: &mut Vec<Failure>) {
    let Expr::CallExpr(call) = unparen(value) else {
        return;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(&call.fun) else {
        return;
    };
    let Some(recv_type) = pass.types_info().and_then(|info| info.types.get(&x.id())) else {
        return;
    };
    if !is_named_type(pass, recv_type.typ, "time", "Time") {
        return;
    }
    let Some((_, suffixes)) = EPOCH_UNITS.iter().find(|(m, _)| *m == sel.name) else {
        return;
    };
    if !has_any_suffix(&name.name, suffixes) {
        failures.push(Failure {
            rule: "epoch-naming",
            pos: name.name_pos.0 as u32,
            message: format!(
                "var {} should have one of these suffixes: {}",
                name.name,
                suffixes.join(", ")
            ),
        });
    }
}

fn has_any_suffix(name: &str, suffixes: &[&str]) -> bool {
    let lower = name.to_ascii_lowercase();
    suffixes
        .iter()
        .any(|suffix| lower.ends_with(&suffix.to_ascii_lowercase()))
}

#[allow(unused_imports)]
use guff::ast::ValueSpec as _;
