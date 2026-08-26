//! SA4026 — Go constants cannot express negative zero
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4026`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{
    match_pos, matches, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};




static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(UnaryExpr "-" (BasicLit "FLOAT" "0.0"))"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4026 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        // `edit.ReplaceWithString(node, "math.Copysign(0, -1)")`. Upstream does
        // not add the `math` import, so a file that lacks it stops compiling —
        // its own choice, and matching it means making the same one.
        let end = match node {
            guff::walk::NodeRef::UnaryExpr(u) => u.x.end().0 as u32,
            _ => 0,
        };
        pending.push((
            match_pos(node),
            end,
            "in Go, the floating-point literal '-0.0' is the same as '0.0', it does not produce a negative zero".into(),
        ));
        true
    });
    for (pos, end, message) in pending {
        if end == 0 {
            pass.reportf(pos, message);
            continue;
        }
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Use math.Copysign to create negative zero".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: "math.Copysign(0, -1)".into(),
                }],
            }],
            ..Diagnostic::default()
        });
    }
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
