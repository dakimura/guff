//! SA4024 — builtin len/cap does not return negative values
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4024`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff::walk::NodeRef;
use guff_analysis::code::is_integer_literal;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};




static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(Or (BinaryExpr (IntegerLiteral "0") ">" (CallExpr builtin@(Builtin (Or "len" "cap")) _)) (BinaryExpr (CallExpr builtin@(Builtin (Or "len" "cap")) _) "<" (IntegerLiteral "0")))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4024 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, m| {
        let NodeRef::BinaryExpr(bin) = node else { return true };
        let is_negative_len = is_integer_literal(pass, &bin.y, 0) && bin.op == guff::token::Token::LSS
            || is_integer_literal(pass, &bin.x, 0) && bin.op == guff::token::Token::GTR;
        if !is_negative_len {
            return true;
        }
        let name = m.state.get("builtin").and_then(|v| v.as_ident()).map(|i| i.name.as_str()).unwrap_or("len");
        pending.push((match_pos(node), format!("builtin function {name} does not return negative values")));
        true
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4024_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4024",
        doc: "builtin len/cap does not return negative values",
        url: "https://staticcheck.dev/docs/checks/#SA4024",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4024_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4024_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
