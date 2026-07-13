//! SA6005 — inefficient string comparison with `strings.ToLower`/`ToUpper`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6005`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_call_to_any;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn is_to_lower_or_upper(pass: &Pass<'_>, call: &CallExpr) -> bool {
    is_call_to_any(pass, call, &["strings.ToLower", "strings.ToUpper"])
}

fn same_to_lower_or_upper(pass: &Pass<'_>, left: &CallExpr, right: &CallExpr) -> bool {
    if !is_to_lower_or_upper(pass, left) || !is_to_lower_or_upper(pass, right) {
        return false;
    }
    guff_analysis::code::call_name(pass, &left.fun) == guff_analysis::code::call_name(pass, &right.fun)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::BinaryExpr(expr) = node else {
            return;
        };
        if expr.op != Token::EQL && expr.op != Token::NEQ {
            return;
        }
        let Expr::CallExpr(left) = expr.x.as_ref() else {
            return;
        };
        let Expr::CallExpr(right) = expr.y.as_ref() else {
            return;
        };
        if !same_to_lower_or_upper(pass, left, right) {
            return;
        }
        let method = if expr.op == Token::NEQ {
            "!strings.EqualFold"
        } else {
            "strings.EqualFold"
        };
        pending.push((
            match_pos(node),
            format!("should use {method} instead"),
        ));
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa6005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6005",
        doc: "inefficient string comparison with strings.ToLower or strings.ToUpper",
        url: "https://staticcheck.dev/docs/checks/#SA6005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
