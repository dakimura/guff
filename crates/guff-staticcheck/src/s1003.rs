//! S1003 — replace `strings.Index` with `strings.Contains`.
//!
//! Port of `honnef.co/go/tools/simple/s1003`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr, Ident, SelectorExpr, UnaryExpr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::expr_to_int;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::render::render_expr;

fn allowed_negation(value: i64, op: Token) -> Option<bool> {
    static ALLOWED: OnceLock<HashMap<i64, HashMap<Token, bool>>> = OnceLock::new();
    let allowed = ALLOWED.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert(
            -1,
            HashMap::from([(Token::GTR, true), (Token::NEQ, true), (Token::EQL, false)]),
        );
        m.insert(
            0,
            HashMap::from([(Token::GEQ, true), (Token::LSS, false)]),
        );
        m
    });
    allowed.get(&value)?.get(&op).copied()
}

fn index_replacement(pkg: &str, fun: &str) -> Option<&'static str> {
    match (pkg, fun) {
        ("strings", "Index") | ("bytes", "Index") => Some("Contains"),
        ("strings", "IndexRune") | ("bytes", "IndexRune") => Some("ContainsRune"),
        ("strings", "IndexAny") | ("bytes", "IndexAny") => Some("ContainsAny"),
        _ => None,
    }
}

fn check_binary(pass: &Pass<'_>, expr: &BinaryExpr) -> Option<(u32, String)> {
    let value = expr_to_int(pass, &expr.y)?;
    let positive = allowed_negation(value, expr.op)?;

    let Expr::CallExpr(call) = &*expr.x else {
        return None;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = &*call.fun else {
        return None;
    };
    let Expr::Ident(Ident { name: pkg_name, .. }) = &**x else {
        return None;
    };
    if pkg_name != "strings" && pkg_name != "bytes" {
        return None;
    }
    let replacement = index_replacement(pkg_name, &sel.name)?;

    let fun = Expr::SelectorExpr(SelectorExpr {
        x: x.clone(),
        sel: Ident::new_ident(replacement),
        id: 0,
    });
    let mut replacement_expr = Expr::CallExpr(CallExpr {
        fun: Box::new(fun),
        lparen: Default::default(),
        args: call.args.clone(),
        ellipsis: Default::default(),
        rparen: Default::default(),
        id: 0,
    });
    if !positive {
        replacement_expr = Expr::UnaryExpr(UnaryExpr {
            op_pos: Default::default(),
            op: Token::NOT,
            x: Box::new(replacement_expr),
            id: 0,
        });
    }

    Some((
        expr.op_pos.0 as u32,
        format!(
            "should use {} instead",
            render_expr(&replacement_expr)
        ),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1003 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::BinaryExpr(expr) = n else {
            return;
        };
        if let Some((pos, msg)) = check_binary(pass, expr) {
            pending.push((pos, msg));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1003",
        doc: "replace call to strings.Index with strings.Contains",
        url: "https://staticcheck.dev/docs/checks/#S1003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1003 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn allowed_negation_table() {
        assert_eq!(allowed_negation(-1, Token::NEQ), Some(true));
        assert_eq!(allowed_negation(-1, Token::EQL), Some(false));
        assert_eq!(allowed_negation(0, Token::GEQ), Some(true));
        assert_eq!(allowed_negation(1, Token::EQL), None);
    }
}
