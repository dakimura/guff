//! SA4022 — comparing the address of a variable against nil
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4022`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(BinaryExpr (UnaryExpr "&" _) (Or "==" "!=") (Or nil (Ident "nil")))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4022 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    // Upstream is the pattern and nothing else. guff also carried a hand-rolled
    // BinaryExpr walk that matched the same shape and reported it a second time
    // at `op_pos`, so every finding appeared twice — once correctly and once on
    // the operator. `issues.uniq-by-line` (on by default) collapsed the pair,
    // which is why no gate saw it until the golden tier turned that off.
    matches(pass, &inspect, pat(), |node, _| {
        pending.push((match_pos(node), "the address of a variable cannot be nil".into()));
        true
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4022_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4022",
        doc: "comparing the address of a variable against nil",
        url: "https://staticcheck.dev/docs/checks/#SA4022",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4022_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4022_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
