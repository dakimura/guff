//! S1004 — replace `bytes.Compare` with `bytes.Equal`.
//!
//! Port of `honnef.co/go/tools/simple/s1004`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_integer_literal};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_compare(pass: &Pass<'_>, expr: &BinaryExpr) -> Option<String> {
    let Expr::CallExpr(call) = &*expr.x else {
        return None;
    };
    if !is_call_to(pass, call, "bytes.Compare") {
        return None;
    }
    if !is_integer_literal(pass, &expr.y, 0) {
        return None;
    }
    let prefix = match expr.op {
        Token::NEQ => "!",
        Token::EQL => "",
        _ => return None,
    };
    Some(format!(
        "should use {prefix}bytes.Equal(...) instead of bytes.Compare(...) == 0"
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg == "bytes" || pkg == "bytes_test" {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(expr) = node else {
            return;
        };
        if let Some(msg) = check_compare(pass, expr) {
            pending.push((expr.op_pos.0 as u32, msg));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1004",
        doc: "replace call to bytes.Compare with bytes.Equal",
        url: "https://staticcheck.dev/docs/checks/#S1004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
