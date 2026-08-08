//! S1010 — omit default slice index in slice expressions.
//!
//! Port of `honnef.co/go/tools/simple/s1010`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{entry_mask, match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(SliceExpr x@(Object _) low (CallExpr (Builtin "len") [x]) nil)"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1010 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(entry_mask(pat()), pass.files(), |node| {
        if match_pattern(pass, pat(), node).is_some() {
            // Upstream reports the redundant high expression (`len(s)`), not
            // the slice expression it sits in.
            let pos = match node {
                guff::walk::NodeRef::SliceExpr(s) => s
                    .high
                    .as_ref()
                    .map(|h| h.pos().0 as u32)
                    .unwrap_or_else(|| match_pos(node)),
                _ => match_pos(node),
            };
            pending.push((pos, "should omit second index in slice, s[a:len(s)] is identical to s[a:]".into()));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1010",
        doc: "omit default slice index in slice expressions",
        url: "https://staticcheck.dev/docs/checks/#S1010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1010 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
