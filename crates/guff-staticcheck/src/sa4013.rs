//! SA4013 — negating a boolean twice has no effect
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4013`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};




static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(UnaryExpr "!" (UnaryExpr "!" _))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4013 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        pending.push((match_pos(node), "negating a boolean twice has no effect; is this a typo?".into()));
        true
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4013_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4013",
        doc: "negating a boolean twice has no effect",
        url: "https://staticcheck.dev/docs/checks/#SA4013",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4013_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4013_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
