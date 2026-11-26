//! SA4028 — x % 1 is always zero
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4028`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_int, is_integer_literal};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};



static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(BinaryExpr _ "%" (IntegerLiteral "1"))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4028 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        let NodeRef::BinaryExpr(bin) = node else { return true };
        // The pattern already asked `(IntegerLiteral "1")`, and upstream asks
        // nothing further. The belt-and-braces `expr_to_int == Some(1)` here
        // was an *or*, so it let a named constant back in after the pattern was
        // tightened.
        let _ = &bin.y;
        pending.push((match_pos(node), "x % 1 is always zero".into()));
        true
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4028_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4028",
        doc: "x % 1 is always zero",
        url: "https://staticcheck.dev/docs/checks/#SA4028",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4028_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4028_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
