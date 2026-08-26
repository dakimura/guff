//! S1024 — replace x.Sub(time.Now()) with time.Until(x).
//!
//! Port of `honnef.co/go/tools/simple/s1024`.

use std::sync::OnceLock;

use guff::ast::{Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::code;
use guff_analysis::passes::{inspect, typeindex};
use guff_analysis::{
    match_pos, matches, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};

use crate::render::render_node;

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(CallExpr (Symbol "(time.Time).Sub") [(CallExpr (Symbol "time.Now") [])])"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1024 requires inspect analyzer".to_string())?
        .clone();
    let _ = pass
        .result_of::<typeindex::Index>(typeindex::analyzer())
        .ok_or_else(|| "S1024 requires typeindex analyzer".to_string())?;

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        // `pattern.NodeToAST` builds `time.Until(sel.X)` from the *receiver* of
        // the `.Sub` selector — so `deadline.Sub(time.Now())` becomes
        // `time.Until(deadline)`. A callee that is not a selector cannot supply
        // that receiver, and upstream reports it without a fix.
        //
        // As in S1037, the replacement spells the package `(Ident "time")`
        // literally rather than resolving the import, so an aliased `time`
        // makes upstream write a name the file does not have.
        let edit = match node {
            NodeRef::CallExpr(call) => match &*call.fun {
                Expr::SelectorExpr(SelectorExpr { x, .. }) => {
                    render_node(pass, x).map(|recv| TextEdit {
                        pos: call.pos().0 as u32,
                        end: call.end().0 as u32,
                        new_text: format!("time.Until({recv})"),
                    })
                }
                _ => None,
            },
            _ => None,
        };
        pending.push((
            match_pos(node),
            "should use time.Until instead of t.Sub(time.Now())".into(),
            edit,
        ));
        true
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with call to time.Until".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1024_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1024",
        doc: "replace x.Sub(time.Now()) with time.Until(x)",
        url: "https://staticcheck.dev/docs/checks/#S1024",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer(), typeindex::analyzer()],
        fact_types: vec![],
    }
}

/// S1024 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1024_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1024_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
