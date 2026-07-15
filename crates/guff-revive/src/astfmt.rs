//! Canonical AST formatting for revive `identical-*` rules.

use guff::ast::{
    AssignStmt, BasicLit, BinaryExpr, BlockStmt, BranchStmt, CallExpr, Expr, Ident, IfStmt,
    IndexExpr, ReturnStmt, SelectorExpr, StarExpr, Stmt, UnaryExpr,
};
use guff::token::Token;

use crate::util::unparen;

/// Returns a stable string representation of `stmts` for equality checks.
pub fn stmts_fmt(stmts: &[Stmt]) -> String {
    stmts
        .iter()
        .map(stmt_fmt)
        .collect::<Vec<_>>()
        .join(";\n")
}

pub fn stmt_fmt(stmt: &Stmt) -> String {
    match stmt {
        Stmt::BlockStmt(b) => block_fmt(b),
        Stmt::ExprStmt(e) => expr_fmt(&e.x),
        Stmt::AssignStmt(a) => assign_fmt(a),
        Stmt::ReturnStmt(r) => return_fmt(r),
        Stmt::BranchStmt(b) => branch_fmt(b),
        Stmt::IfStmt(i) => if_fmt(i),
        Stmt::DeclStmt(d) => format!("decl:{:?}", d.decl),
        Stmt::IncDecStmt(i) => format!("{} {}", expr_fmt(&i.x), i.tok.as_str()),
        Stmt::GoStmt(g) => format!("go {}", expr_fmt(&g.call.fun)),
        Stmt::DeferStmt(d) => format!("defer {}", expr_fmt(&d.call.fun)),
        Stmt::EmptyStmt(_) => ";".into(),
        Stmt::LabeledStmt(l) => format!("{}: {}", l.label.name, stmt_fmt(&l.stmt)),
        other => format!("{other:?}"),
    }
}

pub fn block_fmt(block: &BlockStmt) -> String {
    stmts_fmt(&block.list)
}

fn assign_fmt(a: &AssignStmt) -> String {
    let lhs = a
        .lhs
        .iter()
        .map(expr_fmt)
        .collect::<Vec<_>>()
        .join(", ");
    let rhs = a
        .rhs
        .iter()
        .map(expr_fmt)
        .collect::<Vec<_>>()
        .join(", ");
    format!("{lhs} {} {rhs}", a.tok.map(|t| t.as_str()).unwrap_or("="))
}

fn return_fmt(r: &ReturnStmt) -> String {
    if r.results.is_empty() {
        "return".into()
    } else {
        format!(
            "return {}",
            r.results.iter().map(expr_fmt).collect::<Vec<_>>().join(", ")
        )
    }
}

fn branch_fmt(b: &BranchStmt) -> String {
    match b.label.as_ref() {
        Some(label) => format!("{} {}", b.tok.as_str(), label.name),
        None => b.tok.as_str().to_string(),
    }
}

fn if_fmt(i: &IfStmt) -> String {
    format!(
        "if {} {}",
        expr_fmt(&i.cond),
        block_fmt(&i.body)
    )
}

pub fn expr_fmt(expr: &Expr) -> String {
    match unparen(expr) {
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
