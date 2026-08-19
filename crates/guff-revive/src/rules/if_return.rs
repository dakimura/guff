//! `if-return` — warn on redundant `if err != nil { return err }; return nil`.

use guff::ast::{BlockStmt, Expr, Ident, IfStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{scan_comments, ScannedComment};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    file_index: usize,
    /// Comments of the file being walked, scanned on first use.
    ///
    /// The rule needs them only once it has a complete `if err != nil { return
    /// err }` / `return nil` pair in hand, which is rare, so paying for the
    /// scan up front would be paying it on almost every file for nothing.
    comments: Option<Vec<ScannedComment>>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            file_index: 0,
            comments: None,
            failures: Vec::new(),
        }
    }

    pub fn on_file(&mut self, index: usize) {
        self.file_index = index;
        self.comments = None;
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::BlockStmt(block) = n else {
            return;
        };
        self.check_block(block);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }

    fn check_block(&mut self, block: &BlockStmt) {
        let stmts = &block.list;
        for i in 0..stmts.len().saturating_sub(1) {
            let Stmt::IfStmt(if_stmt) = &stmts[i] else {
                continue;
            };
            let Some(ret_pos) = matches_if_return(if_stmt, &stmts[i + 1]) else {
                continue;
            };
            // A comment between the `if` and the `return nil` is upstream's
            // signal that the construct is deliberate.
            if self.contains_comments(if_stmt.if_.0 as u32, ret_pos) {
                continue;
            }
            self.failures.push(Failure {
                rule: "if-return",
                pos: if_stmt.if_.0 as u32,
                message: "redundant if ...; err != nil check, just return error instead.".into(),
                ..Failure::default()
            });
        }
    }

    /// `containsComments`: any comment in `[start, end)` that is not one of
    /// revive's own `// MATCH ` test markers.
    fn contains_comments(&mut self, start: u32, end: u32) -> bool {
        let comments = self
            .comments
            .get_or_insert_with(|| scan_comments(self.pass, self.file_index).unwrap_or_default());
        comments
            .iter()
            .any(|c| c.pos >= start && c.pos < end && !c.text.starts_with("// MATCH "))
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for (index, file) in pass.files().iter().enumerate() {
        c.on_file(index);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

/// Returns the position of the trailing `return nil` when `if_stmt` / `next`
/// form the redundant pair.
///
/// Every test here is on the raw node, as upstream's are: revive asserts
/// `expr.X.(*ast.Ident)` and friends directly, so a parenthesised operand
/// simply fails the assertion rather than being unwrapped first.
fn matches_if_return(if_stmt: &IfStmt, next: &Stmt) -> Option<u32> {
    if if_stmt.else_.is_some() || if_stmt.body.list.len() != 1 {
        return None;
    }
    let Stmt::AssignStmt(assign) = if_stmt.init.as_deref()? else {
        return None;
    };
    if assign.lhs.len() != 1 {
        return None;
    }
    if !matches!(assign.tok, Some(Token::DEFINE) | Some(Token::ASSIGN)) {
        return None;
    }
    let Expr::Ident(id) = &assign.lhs[0] else {
        return None;
    };
    let Expr::BinaryExpr(cond) = &if_stmt.cond else {
        return None;
    };
    if cond.op != Token::NEQ {
        return None;
    }
    let Expr::Ident(lhs) = cond.x.as_ref() else {
        return None;
    };
    if lhs.name != id.name {
        return None;
    }
    if !matches!(cond.y.as_ref(), Expr::Ident(Ident { name, .. }) if name == "nil") {
        return None;
    }
    let Stmt::ReturnStmt(ret) = if_stmt.body.list.first()? else {
        return None;
    };
    if ret.results.len() != 1 {
        return None;
    }
    let Expr::Ident(ret_id) = &ret.results[0] else {
        return None;
    };
    if ret_id.name != id.name {
        return None;
    }
    let Stmt::ReturnStmt(next_ret) = next else {
        return None;
    };
    if next_ret.results.len() != 1 {
        return None;
    }
    if !matches!(&next_ret.results[0], Expr::Ident(Ident { name, .. }) if name == "nil") {
        return None;
    }
    Some(next_ret.return_.0 as u32)
}
