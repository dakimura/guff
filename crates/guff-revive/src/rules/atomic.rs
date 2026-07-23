//! `atomic` — check for mistaken direct assignment to atomic values.

use guff::ast::{AssignStmt, CallExpr, Expr, StarExpr, UnaryExpr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;
use guff_analysis::code::call_name;

use crate::failure::Failure;
use crate::util::{expr_equal, imports_package, unparen};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn try_new(pass: &'a Pass<'a>) -> Option<Self> {
        if !imports_package(pass, "sync/atomic") {
            return None;
        }
        Some(Self {
            pass,
            failures: Vec::new(),
        })
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::AssignStmt(assign) = n else {
            return;
        };
        check_assign(self.pass, assign, &mut self.failures);
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

fn is_atomic_add(pass: &Pass<'_>, fun: &Expr) -> bool {
    let Some(name) = call_name(pass, fun) else {
        return false;
    };
    matches!(
        name.as_str(),
        "sync/atomic.AddInt32"
            | "sync/atomic.AddInt64"
            | "sync/atomic.AddUint32"
            | "sync/atomic.AddUint64"
            | "sync/atomic.AddUintptr"
    )
}

fn check_assign(pass: &Pass<'_>, assign: &AssignStmt, failures: &mut Vec<Failure>) {
    if assign.lhs.len() != assign.rhs.len() {
        return;
    }
    if assign.lhs.len() == 1 && assign.tok == Some(Token::DEFINE) {
        return;
    }
    for (left, right) in assign.lhs.iter().zip(&assign.rhs) {
        let Expr::CallExpr(call) = right else {
            continue;
        };
        if !is_atomic_add(pass, &call.fun) || call.args.len() != 2 {
            continue;
        }
        let arg = &call.args[0];
        let broken = match unparen(arg) {
            Expr::UnaryExpr(UnaryExpr { op: Token::AND, x, .. }) => expr_equal(left, x),
            _ => match unparen(left) {
                Expr::StarExpr(StarExpr { x, .. }) => expr_equal(x, arg),
                _ => false,
            },
        };
        if broken {
            failures.push(Failure {
                rule: "atomic",
                pos: left.pos().0 as u32,
                message: "direct assignment to atomic value".into(),
            confidence: None,
        });
        }
    }
}
