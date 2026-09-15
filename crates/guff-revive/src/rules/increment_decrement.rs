//! `increment-decrement` — suggest `++`/`--` over `+= 1`/`-= 1`.

use guff::ast::{AssignStmt, BasicLit, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::render_node;

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
        if let NodeRef::AssignStmt(assign) = n {
            check_assign(self.pass, assign, &mut self.failures);
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

fn check_assign(pass: &Pass<'_>, assign: &AssignStmt, failures: &mut Vec<Failure>) {
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
    // `fmt.Sprintf("should replace %s with %s%s", w.file.Render(as),
    // w.file.Render(as.Lhs[0]), suffix)` — upstream renders whatever the
    // left-hand side is. guff accepted an `Ident` and nothing else, so a field
    // (`m.LinkViews += 1`) or an index was silently not a finding: three of
    // photoprism's `internal/entity` findings, and the rule never fires on a
    // method's own state, which is most of where `+= 1` is written.
    let lhs = render_node(pass, &assign.lhs[0]);
    if lhs.is_empty() {
        return;
    }
    failures.push(Failure {
        rule: "increment-decrement",
        // Upstream's node is the `*ast.AssignStmt`, which starts at its LHS.
        pos: assign
            .lhs
            .first()
            .map(|e| e.pos().0)
            .unwrap_or(assign.tok_pos.0) as u32,
        message: format!("should replace {lhs} {op_text} with {lhs}{suffix}"),
        ..Failure::default()
    });
}
