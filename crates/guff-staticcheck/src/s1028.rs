//! S1028 — simplify error construction with fmt.Errorf.
//!
//! Port of `honnef.co/go/tools/simple/s1028`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(CallExpr (Symbol "errors.New") [(CallExpr (Symbol "fmt.Sprintf") args)])"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1028 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        if match_pattern(pass, pat(), node).is_some() {
            pending.push((match_pos(node), "should use fmt.Errorf(...) instead of errors.New(fmt.Sprintf(...))".into()));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1028_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1028",
        doc: "simplify error construction with fmt.Errorf",
        url: "https://staticcheck.dev/docs/checks/#S1028",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1028 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1028_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1028_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
