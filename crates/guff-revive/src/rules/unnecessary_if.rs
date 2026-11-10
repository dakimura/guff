//! `unnecessary-if` — replace if/else with boolean expressions when possible.

use guff::ast::{Decl, Expr, File, Ident, IfStmt, Stmt};
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

    /// Upstream walks each `FuncDecl` body only (so a closure in a package
    /// `var` is never seen), and returns `nil` after a report (so an `if`
    /// nested in a reported one is not). Neither fits a node-at-a-time
    /// visitor; the whole walk happens here.
    pub fn on_file(&mut self, file: &File) {
        check_file(file, &mut self.failures);
    }

    pub fn visit(&mut self, _n: NodeRef<'_>) {}

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        check_file(file, &mut failures);
    }
    failures
}

fn check_file(file: &File, failures: &mut Vec<Failure>) {
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else {
            continue;
        };
        let Some(body) = &fd.body else {
            continue;
        };
        walk::inspect(NodeRef::BlockStmt(body), |n| {
            let Some(NodeRef::IfStmt(if_stmt)) = n else {
                return true;
            };
            let Some(msg) = check_if(if_stmt) else {
                return true;
            };
            failures.push(Failure {
                rule: "unnecessary-if",
                pos: if_stmt.if_.0 as u32,
                message: msg,
                ..Failure::default()
            });
            false
        });
    }
}

fn check_if(if_stmt: &IfStmt) -> Option<String> {
    if if_stmt.init.is_some() {
        return None;
    }
    let Stmt::BlockStmt(else_block) = if_stmt.else_.as_deref()? else {
        return None;
    };
    if if_stmt.body.list.len() != 1 || else_block.list.len() != 1 {
        return None;
    }
    let (replacement, then_bool) = replacement_for_then(&if_stmt.body.list[0], &else_block.list[0])?;
    let cond = cond_as_string(&if_stmt.cond, !then_bool);
    Some(format!("replace this conditional by: {replacement} {cond}"))
}

/// revive v1.17.1 `replacementFor{Return,Assignment}Stmt`: both branches must
/// hold a single Boolean literal and the two must be **complementary**; an
/// assignment also needs the same token on both sides (`:=` is not `=`) and
/// exactly one left-hand side on each.
fn replacement_for_then(then_stmt: &Stmt, else_stmt: &Stmt) -> Option<(String, bool)> {
    match then_stmt {
        Stmt::ReturnStmt(then_ret) => {
            let then_value = single_bool_literal(&then_ret.results)?;
            let Stmt::ReturnStmt(else_ret) = else_stmt else {
                return None;
            };
            let else_value = single_bool_literal(&else_ret.results)?;
            if then_value == else_value {
                return None;
            }
            Some(("return".into(), then_value))
        }
        Stmt::AssignStmt(then_assign) => {
            let then_value = single_bool_literal(&then_assign.rhs)?;
            let Stmt::AssignStmt(else_assign) = else_stmt else {
                return None;
            };
            if then_assign.tok != else_assign.tok
                || then_assign.lhs.len() != 1
                || else_assign.lhs.len() != 1
            {
                return None;
            }
            let then_lhs = expr_fmt(&then_assign.lhs[0]);
            if then_lhs != expr_fmt(&else_assign.lhs[0]) {
                return None;
            }
            let else_value = single_bool_literal(&else_assign.rhs)?;
            if then_value == else_value {
                return None;
            }
            let tok = then_assign.tok?.as_str();
            Some((format!("{then_lhs} {tok}"), then_value))
        }
        _ => None,
    }
}

/// `isSingleBooleanLiteral`: a bare `*ast.Ident` — `(true)` is not one.
fn single_bool_literal(exprs: &[Expr]) -> Option<bool> {
    if exprs.len() != 1 {
        return None;
    }
    let Expr::Ident(Ident { name, .. }) = &exprs[0] else {
        return None;
    };
    match name.as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// Upstream `condAsString`: `GoFmt(cond)`, negated naively as `!(…)`, or —
/// when the condition itself (not unwrapped) is a relational `BinaryExpr` —
/// `GoFmt` of the same expression with the opposite operator.
fn cond_as_string(cond: &Expr, must_negate: bool) -> String {
    if must_negate {
        // `cond.(*ast.BinaryExpr)`: a parenthesized condition is negated naively.
        if let Expr::BinaryExpr(bin) = cond {
            if let Some(opposite) = relational_opposite(bin.op) {
                let mut flipped = bin.clone();
                flipped.op = opposite;
                return expr_fmt(&Expr::BinaryExpr(flipped));
            }
        }
        format!("!({})", expr_fmt(cond))
    } else {
        expr_fmt(cond)
    }
}

fn relational_opposite(op: Token) -> Option<Token> {
    match op {
        Token::EQL => Some(Token::NEQ),
        Token::NEQ => Some(Token::EQL),
        Token::LSS => Some(Token::GEQ),
        Token::LEQ => Some(Token::GTR),
        Token::GTR => Some(Token::LEQ),
        Token::GEQ => Some(Token::LSS),
        _ => None,
    }
}

/// Upstream renders every one of these with `astutils.GoFmt`, the same printer
/// its other rules use — so this one does too, rather than keeping a smaller
/// copy beside it.
///
/// The copy that used to live here knew `Ident`, `BinaryExpr`, `UnaryExpr` and
/// `ParenExpr`, and answered `"<expr>"` for everything else. That reached the
/// message (`b = n > <expr>` for `if n > 1`), and worse: the then/else
/// left-hand sides are compared **as text**, so `s.Force` and `s.Disable` both
/// rendered `"<expr>"`, compared equal, and fiber's
/// `if colors { cfg.ForceColors = true } else { cfg.DisableColors = true }`
/// became a finding upstream does not make.
///
/// `GoFmt` keeps parentheses: `if (ok)` prints as `(ok)`.
fn expr_fmt(expr: &Expr) -> String {
    crate::astfmt::expr_gofmt(expr)
}
