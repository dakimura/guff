//! `unchecked-type-assertion` — warn on unchecked type assertions.

use guff::ast::{
    AssignStmt, Expr, IfStmt, RangeStmt, ReturnStmt, SendStmt, TypeAssertExpr,
};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_blank_ident, unparen};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let pass = self.pass;
        match n {
            NodeRef::RangeStmt(s) => require_no_type_assert(pass, &s.x, &mut self.failures),
            NodeRef::SwitchStmt(s) => {
                if let Some(tag) = &s.tag {
                    require_no_type_assert(pass, tag, &mut self.failures);
                    require_binary_without_type_assert(pass, tag, &mut self.failures);
                }
            }
            NodeRef::ReturnStmt(r) => {
                for e in &r.results {
                    require_no_type_assert(pass, e, &mut self.failures);
                }
            }
            NodeRef::AssignStmt(a) => handle_assign(pass, a, &mut self.failures),
            NodeRef::IfStmt(i) => {
                if let Expr::BinaryExpr(b) = unparen(&i.cond) {
                    require_no_type_assert(pass, &b.x, &mut self.failures);
                    require_no_type_assert(pass, &b.y, &mut self.failures);
                }
            }
            NodeRef::CaseClause(c) => {
                for e in &c.list {
                    require_no_type_assert(pass, e, &mut self.failures);
                    require_binary_without_type_assert(pass, e, &mut self.failures);
                }
            }
            NodeRef::SendStmt(s) => require_no_type_assert(pass, &s.value, &mut self.failures),
            _ => {}
        }
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

fn is_type_switch(e: &TypeAssertExpr) -> bool {
    e.ty.is_none()
}

fn add_failure(pass: &Pass<'_>, e: &TypeAssertExpr, why: &str, failures: &mut Vec<Failure>) {
    failures.push(Failure {
        rule: "unchecked-type-assertion",
        pos: e.x.pos().0 as u32,
        // Upstream prints the assertion itself: "…unchecked in v.(int) - …".
        message: format!(
            "type cast result is unchecked in {} - {why}",
            format_expr(pass, &Expr::TypeAssertExpr(e.clone()))
        ),
        ..Failure::default()
    });
}

fn require_no_type_assert(pass: &Pass<'_>, expr: &Expr, failures: &mut Vec<Failure>) {
    let Expr::TypeAssertExpr(e) = unparen(expr) else {
        return;
    };
    if !is_type_switch(e) {
        add_failure(pass, e, "type assertion will panic if not matched", failures);
    }
}

fn require_binary_without_type_assert(pass: &Pass<'_>, expr: &Expr, failures: &mut Vec<Failure>) {
    let Expr::BinaryExpr(b) = unparen(expr) else {
        return;
    };
    require_no_type_assert(pass, &b.x, failures);
    require_no_type_assert(pass, &b.y, failures);
}

fn handle_assign(pass: &Pass<'_>, assign: &AssignStmt, failures: &mut Vec<Failure>) {
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
        add_failure(pass, e, "type assertion will panic if not matched", failures);
    } else if assign.lhs.len() == 2 && is_blank_ident(&assign.lhs[1]) {
        add_failure(pass, e, "type assertion result ignored", failures);
    }
}

/// `astutils.GoFmt`: the node as `go/printer` renders it.
fn format_expr(pass: &Pass<'_>, e: &Expr) -> String {
    let mut buf: Vec<u8> = Vec::new();
    match guff::printer::fprint(&mut buf, pass.fset(), guff::printer::PrintNode::Expr(e)) {
        Ok(()) => String::from_utf8(buf).unwrap_or_default(),
        Err(_) => String::new(),
    }
}
