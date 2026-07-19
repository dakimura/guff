//! `unchecked-type-assertion` — warn on unchecked type assertions.

use guff::ast::{
    AssignStmt, Expr, IfStmt, RangeStmt, ReturnStmt, SendStmt, TypeAssertExpr,
};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank_ident, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::RangeStmt(s) => require_no_type_assert(&s.x, &mut failures),
                NodeRef::SwitchStmt(s) => {
                    if let Some(tag) = &s.tag {
                        require_no_type_assert(tag, &mut failures);
                        require_binary_without_type_assert(tag, &mut failures);
                    }
                }
                NodeRef::ReturnStmt(r) => {
                    for e in &r.results {
                        require_no_type_assert(e, &mut failures);
                    }
                }
                NodeRef::AssignStmt(a) => handle_assign(a, &mut failures),
                NodeRef::IfStmt(i) => {
                    if let Expr::BinaryExpr(b) = unparen(&i.cond) {
                        require_no_type_assert(&b.x, &mut failures);
                        require_no_type_assert(&b.y, &mut failures);
                    }
                }
                NodeRef::CaseClause(c) => {
                    for e in &c.list {
                        require_no_type_assert(e, &mut failures);
                        require_binary_without_type_assert(e, &mut failures);
                    }
                }
                NodeRef::SendStmt(s) => require_no_type_assert(&s.value, &mut failures),
                _ => {}
            }
            true
        });
    }
    failures
}

fn is_type_switch(e: &TypeAssertExpr) -> bool {
    e.ty.is_none()
}

fn add_failure(e: &TypeAssertExpr, why: &str, failures: &mut Vec<Failure>) {
    failures.push(Failure {
        rule: "unchecked-type-assertion",
        pos: e.x.pos().0 as u32,
        message: format!("type cast result is unchecked - {why}"),
            confidence: None,
        });
}

fn require_no_type_assert(expr: &Expr, failures: &mut Vec<Failure>) {
    let Expr::TypeAssertExpr(e) = unparen(expr) else {
        return;
    };
    if !is_type_switch(e) {
        add_failure(e, "type assertion will panic if not matched", failures);
    }
}

fn require_binary_without_type_assert(expr: &Expr, failures: &mut Vec<Failure>) {
    let Expr::BinaryExpr(b) = unparen(expr) else {
        return;
    };
    require_no_type_assert(&b.x, failures);
    require_no_type_assert(&b.y, failures);
}

fn handle_assign(assign: &AssignStmt, failures: &mut Vec<Failure>) {
    if assign.rhs.is_empty() {
        return;
    }
    let Expr::TypeAssertExpr(e) = unparen(&assign.rhs[0]) else {
        return;
    };
    if is_type_switch(e) {
        return;
    }
    if assign.lhs.len() == 1 {
        add_failure(e, "type assertion will panic if not matched", failures);
    } else if assign.lhs.len() == 2 && is_blank_ident(&assign.lhs[1]) {
        add_failure(e, "type assertion result ignored", failures);
    }
}
