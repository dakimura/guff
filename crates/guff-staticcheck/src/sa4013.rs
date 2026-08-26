//! SA4013 — negating a boolean twice has no effect
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4013`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{
    match_pos, matches, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};

use crate::render::render_node;




static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    // `single` and `x` are bound because both are replacements: upstream
    // offers "Turn into single negation" and "Remove double negation" over the
    // same node.
    PAT.get_or_init(|| must_parse(r#"(UnaryExpr "!" single@(UnaryExpr "!" x))"#))
}

/// `render_node` for a matched node that arrived as a `NodeRef`.
fn render_node_ref(pass: &Pass<'_>, node: guff::walk::NodeRef<'_>) -> Option<String> {
    match node {
        guff::walk::NodeRef::UnaryExpr(u) => {
            render_node(pass, &guff::ast::Expr::UnaryExpr(u.clone()))
        }
        guff::walk::NodeRef::Ident(i) => Some(i.name.clone()),
        _ => None,
    }
}

/// The matched node's byte span.
fn node_span(node: guff::walk::NodeRef<'_>) -> (u32, u32) {
    match node {
        guff::walk::NodeRef::UnaryExpr(u) => (u.op_pos.0 as u32, u.x.end().0 as u32),
        _ => (0, 0),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4013 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String, Vec<SuggestedFix>)> = Vec::new();
    matches(pass, &inspect, pat(), |node, m| {
        // Two fixes over one span, exactly as upstream. They conflict, so
        // golangci's fixer drops every staticcheck edit for the file and the
        // code is left alone — offering only one of them would rewrite a file
        // upstream does not (COMPAT-HARDENING 続き 66).
        let span = node_span(node);
        let fixes: Vec<SuggestedFix> = [("single", "Turn into single negation"), ("x", "Remove double negation")]
            .iter()
            .filter_map(|(name, label)| {
                // A `name@(UnaryExpr …)` binding arrives as a `Node`, not an
                // `Expr`, so `as_expr()` alone silently yields nothing — the
                // same trap as testifylint's `recv@` binding.
                let text = match m.state.get(*name)? {
                    v if v.as_expr().is_some() => render_node(pass, v.as_expr()?)?,
                    guff_pattern::MatchValue::Node(n) => render_node_ref(pass, *n)?,
                    _ => return None,
                };
                Some(SuggestedFix {
                    message: (*label).to_string(),
                    text_edits: vec![TextEdit {
                        pos: span.0,
                        end: span.1,
                        new_text: text,
                    }],
                })
            })
            .collect();
        pending.push((
            match_pos(node),
            "negating a boolean twice has no effect; is this a typo?".into(),
            fixes,
        ));
        true
    });
    for (pos, message, suggested_fixes) in pending {
        if suggested_fixes.is_empty() {
            pass.reportf(pos, message);
            continue;
        }
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes,
            ..Diagnostic::default()
        });
    }
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
