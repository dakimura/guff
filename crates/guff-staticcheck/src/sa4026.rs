//! SA4026 — Go constants cannot express negative zero
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4026`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};




static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(UnaryExpr "-" (BasicLit "FLOAT" "0.0"))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4026 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        pending.push((match_pos(node), "in Go, the floating-point literal '-0.0' is the same as '0.0', it does not produce a negative zero".into()));
        true
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4026_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4026",
        doc: "Go constants cannot express negative zero",
        url: "https://staticcheck.dev/docs/checks/#SA4026",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4026_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4026_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
