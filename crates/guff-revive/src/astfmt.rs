//! Canonical AST formatting for revive `identical-*` rules.

use std::sync::Arc;

use guff::ast::{
    BasicLit, BinaryExpr, BlockStmt, CallExpr, Expr, Ident, IndexExpr, SelectorExpr, StarExpr, Stmt,
    UnaryExpr,
};
use guff::position::FileSet;
use guff::printer::{Config, PrintNode};
use guff::token::Token;

use crate::util::unparen;

/// `astutils.GoFmt`: `printer.Config{Tabwidth: 8}.Fprint` into a **fresh**
/// `token.FileSet`.
///
/// The empty file set is the interesting half. With no file registered, every
/// `Pos` resolves to the zero `Position`, so the printer sees no line
/// information and lays the node out canonically — which makes the comparison
/// blind to how the branch happened to be wrapped in the source. That is what
/// the `identical-*` rules want, and it is why they can afford to compare
/// printed text at all.
///
/// This replaced a hand-written renderer that walked a dozen statement kinds
/// and fell back to `{:?}` for the rest. Two ways that leaked: `if` dropped its
/// init statement and its `else`, so `if err := A(); err != nil` and
/// `if err := B(); err != nil` printed the same string — six false
/// `identical-branches` on gitea alone; and the `{:?}` fallback embedded node
/// positions, so two genuinely identical `for`/`range`/`switch` branches could
/// never compare equal.
fn go_fmt(node: PrintNode<'_>) -> String {
    let fset = Arc::new(FileSet::new());
    let mut buf = Vec::new();
    let cfg = Config {
        tabwidth: 8,
        ..Config::default()
    };
    match cfg.fprint(&mut buf, &fset, node) {
        Ok(()) => String::from_utf8_lossy(&buf).into_owned(),
        // A print into a `Vec` has no I/O to fail at, so this is the printer
        // itself giving up. Keep whatever it managed to write and prefix a byte
        // no Go source contains, so the result is still a description of *this*
        // node rather than one constant every failure collapses onto.
        Err(e) => format!("\u{0}unprintable:{e}:{}", String::from_utf8_lossy(&buf)),
    }
}

/// Returns a stable string representation of `stmts` for equality checks.
///
/// Upstream wraps the statement list in a synthetic `*ast.BlockStmt` before
/// hashing it. The braces are the same on both sides of every comparison, so
/// printing the list directly puts nodes in exactly the same equality classes
/// without the clone.
pub fn stmts_fmt(stmts: &[Stmt]) -> String {
    go_fmt(PrintNode::Stmts(stmts))
}

pub fn stmt_fmt(stmt: &Stmt) -> String {
    match stmt {
        // `identical-ifelseif-branches` compares `if` bodies against a trailing
        // `else` block, so a block reached as a statement has to render the
        // same way `block_fmt` renders it.
        Stmt::BlockStmt(b) => block_fmt(b),
        other => go_fmt(PrintNode::Stmt(other)),
    }
}

pub fn block_fmt(block: &BlockStmt) -> String {
    stmts_fmt(&block.list)
}

/// Render an expression the way `astutils.GoFmt` does — i.e. the way
/// `go/printer` does, which **keeps** parentheses.
///
/// This used to open with `unparen(expr)`, which made the `ParenExpr` arm below
/// unreachable and quietly dropped a level of parentheses from every message
/// built out of it. `use-fmt-print` printed
/// `replace it by "fmt.Fprintln(os.Stderr, "ok")"` where upstream prints
/// `…, ("ok"))` for `println(("ok"))`. Same family as the `unnecessary-format`
/// fix in the same session: revive never unwraps parentheses, in matching or in
/// rendering (COMPAT-HARDENING §4, 2026-08-13).
pub fn expr_fmt(expr: &Expr) -> String {
    match expr {
        Expr::Ident(Ident { name, .. }) => name.clone(),
        Expr::BasicLit(BasicLit { value, kind, .. }) => {
            if *kind == Some(Token::STRING) {
                value.clone()
            } else {
                value.clone()
            }
        }
        Expr::BinaryExpr(BinaryExpr { op, x, y, .. }) => {
            format!("{} {} {}", expr_fmt(x), op.as_str(), expr_fmt(y))
        }
        Expr::UnaryExpr(UnaryExpr { op, x, .. }) => format!("{}{}", op.as_str(), expr_fmt(x)),
        Expr::ParenExpr(p) => format!("({})", expr_fmt(&p.x)),
        Expr::CallExpr(CallExpr { fun, args, .. }) => {
            let args = args.iter().map(expr_fmt).collect::<Vec<_>>().join(", ");
            format!("{}({args})", expr_fmt(fun))
        }
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            format!("{}.{}", expr_fmt(x), sel.name)
        }
        Expr::StarExpr(StarExpr { x, .. }) => format!("*{}", expr_fmt(x)),
        Expr::IndexExpr(IndexExpr { x, index, .. }) => {
            format!("{}[{}]", expr_fmt(x), expr_fmt(index))
        }
        Expr::InterfaceType(it) if it.methods.list.is_empty() => "interface{}".into(),
        Expr::FuncLit(f) => format!("func() {}", block_fmt(&f.body)),
        other => format!("{other:?}"),
    }
}
