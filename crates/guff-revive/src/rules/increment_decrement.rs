//! `increment-decrement` — suggest `++`/`--` over `+= 1`/`-= 1`.

use guff::ast::{AssignStmt, BasicLit, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

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
            
                    if let NodeRef::AssignStmt(assign) = n {
                        check_assign(assign, &mut self.failures);
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


fn is_one(expr: &Expr) -> bool {
    matches!(
        expr,
        Expr::BasicLit(BasicLit {
            kind: Some(Token::INT),
            value,
            ..
        }) if value == "1"
    )
}

fn check_assign(assign: &AssignStmt, failures: &mut Vec<Failure>) {
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return;
    }
    if !is_one(&assign.rhs[0]) {
        return;
    }
    let (op_text, suffix) = match assign.tok {
        Some(Token::AddAssign) => ("+= 1", "++"),
        Some(Token::SubAssign) => ("-= 1", "--"),
        _ => return,
    };
    let lhs = match &assign.lhs[0] {
        Expr::Ident(id) => id.name.clone(),
        _ => return,
    };
    failures.push(Failure {
        rule: "increment-decrement",
        pos: assign.tok_pos.0 as u32,
        message: format!("should replace {lhs} {op_text} with {lhs}{suffix}"),
        confidence: None,
    });
}
