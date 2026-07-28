//! S1007 — simplify regular expression by using raw string literal.
//!
//! Port of `honnef.co/go/tools/simple/s1007`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn should_use_raw_string(val: &str) -> bool {
    if val.contains('`') {
        return false;
    }
    val.contains('\\')
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1007 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<u32> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to_any(pass, call, &["regexp.Compile", "regexp.MustCompile"])
            || call.args.len() != 1
        {
            return;
        }
        let Expr::BasicLit(lit) = &call.args[0] else {
            return;
        };
        if lit.kind != Some(Token::STRING) || lit.value.starts_with('`') {
            return;
        }
        let Some(val) = expr_to_string(pass, &call.args[0]) else {
            return;
        };
        if !should_use_raw_string(&val) {
            return;
        }
        pending.push(lit.value_pos.0 as u32);
    });
    for pos in pending {
        pass.report_unless_generated(
            pos,
            "should use raw string (`...`) with regexp.Compile to avoid having to escape twice",
        );
    }
    Ok(None)
}

fn s1007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1007",
        doc: "simplify regular expression by using raw string literal",
        url: "https://staticcheck.dev/docs/checks/#S1007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn raw_string_heuristic() {
        assert!(should_use_raw_string(r"\A\w+"));
        assert!(!should_use_raw_string("\n"));
    }
}
