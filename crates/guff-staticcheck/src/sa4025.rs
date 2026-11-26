//! SA4025 — integer division of literals that results in zero
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4025`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::walk::NodeRef;
use guff_analysis::code::expr_to_int;

use crate::render::render_expr;

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(BinaryExpr (IntegerLiteral _) "/" (IntegerLiteral _))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4025 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        let NodeRef::BinaryExpr(bin) = node else { return true };
        let Some(x) = expr_to_int(pass, &bin.x) else { return true };
        let Some(y) = expr_to_int(pass, &bin.y) else { return true };
        if y == 0 { return true; }
        if x / y == 0 {
            pending.push((match_pos(node), format!("the integer division '{}' results in zero", render_expr(&guff::ast::Expr::BinaryExpr(bin.clone())))));
        }
        true
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4025_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4025",
        doc: "integer division of literals that results in zero",
        url: "https://staticcheck.dev/docs/checks/#SA4025",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4025_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4025_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
