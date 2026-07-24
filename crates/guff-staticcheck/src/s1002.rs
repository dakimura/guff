//! S1002 — omit comparison with boolean constant.
//!
//! Port of `honnef.co/go/tools/simple/s1002`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn};
use guff_analysis::Pass;
use guff_constant::{bool_val, Kind};
use guff_types::predicates::is_boolean;

fn is_bool_const(pass: &Pass<'_>, expr: &Expr) -> Option<bool> {
    if let Expr::BasicLit(lit) = expr {
        return match lit.value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
    }
    let info = pass.types_info()?;
    let tav = info.types.get(&expr.id())?;
    let val = tav.val.as_ref()?;
    if val.kind() != Kind::Bool {
        return None;
    }
    Some(bool_val(val))
}

fn expr_is_bool(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let tav = match info.types.get(&expr.id()) {
        Some(t) => t,
        None => return false,
    };
    is_boolean(
        &artifacts.types,
        tav.typ.underlying(&artifacts.types),
    )
}

fn render_expr(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::ParenExpr(p) => format!("({})", render_expr(&p.x)),
        Expr::UnaryExpr(u) if u.op == Token::NOT => format!("!{}", render_expr(&u.x)),
        Expr::SelectorExpr(s) => format!("{}.{}", render_expr(&s.x), s.sel.name),
        Expr::CallExpr(c) => {
            let mut s = render_expr(&c.fun);
            s.push('(');
            for (i, arg) in c.args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&render_expr(arg));
            }
            s.push(')');
            s
        }
        _ => "<expr>".to_string(),
    }
}

fn simplified_condition(op: Token, const_val: bool, other: &Expr) -> String {
    let negate = matches!(
        (op, const_val),
        (Token::EQL, false) | (Token::NEQ, true)
    );
    let rendered = format!(
        "{}{}",
        if negate { "!" } else { "" },
        render_expr(other)
    );
    let orig_len = rendered.len();
    let trimmed = rendered.trim_start_matches('!');
    let leading_bangs = orig_len - trimmed.len();
    if leading_bangs % 2 == 1 {
        format!("!{trimmed}")
    } else {
        trimmed.to_string()
    }
}

fn check_binary(pass: &Pass<'_>, expr: &BinaryExpr) -> Option<(u32, String)> {
    if expr.op != Token::EQL && expr.op != Token::NEQ {
        return None;
    }
    let x_const = is_bool_const(pass, &expr.x);
    let y_const = is_bool_const(pass, &expr.y);
    if x_const.is_none() && y_const.is_none() {
        return None;
    }
    let (other, val) = if let Some(v) = x_const {
        (&expr.y, v)
    } else {
        (&expr.x, y_const?)
    };
    if !expr_is_bool(pass, other) {
        return None;
    }
    let simplified = simplified_condition(expr.op, val, other);
    Some((
        expr.op_pos.0 as u32,
        format!(
            "should omit comparison to bool constant, can be simplified to {simplified}"
        ),
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1002 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    // Upstream `simple/s1002` skips `_test.go` (`code.IsInTest`).
    let compiled = &pass.pkg().compiled_go_files;
    for (fi, file) in pass.files().iter().enumerate() {
        if compiled
            .get(fi)
            .is_some_and(|p| p.to_string_lossy().ends_with("_test.go"))
        {
            continue;
        }
        inspect.preorder(std::slice::from_ref(file), |n| {
            let NodeRef::BinaryExpr(expr) = n else {
                return;
            };
            if let Some((pos, msg)) = check_binary(pass, expr) {
                pending.push((pos, msg));
            }
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn s1002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1002",
        doc: "omit comparison with boolean constant",
        url: "https://staticcheck.dev/docs/checks/#S1002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1002 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn simplified_condition_examples() {
        use guff::ast::Ident;
        let x = Expr::Ident(Ident::new_ident("x"));
        assert_eq!(
            simplified_condition(Token::EQL, true, &x),
            "x"
        );
        assert_eq!(
            simplified_condition(Token::EQL, false, &x),
            "!x"
        );
        assert_eq!(
            simplified_condition(Token::NEQ, true, &x),
            "!x"
        );
    }
}
