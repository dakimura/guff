//! S1028 — simplify error construction with fmt.Errorf.
//!
//! Port of `honnef.co/go/tools/simple/s1028`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::{inspect, typeindex};
use guff::ast::Expr;
use guff::walk::NodeRef;
use guff_analysis::code;
use guff_analysis::{
    match_pos, matches, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix,
    TextEdit,
};

use crate::render::render_node;

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(CallExpr (Symbol "errors.New") [(CallExpr (Symbol "fmt.Sprintf") args)])"#))
}

/// `fmt.Errorf(<the Sprintf call's arguments>)`, spanning the outer
/// `errors.New` call.
fn errorf_edit(pass: &Pass<'_>, node: NodeRef<'_>) -> Option<TextEdit> {
    let NodeRef::CallExpr(outer) = node else {
        return None;
    };
    let Expr::CallExpr(sprintf) = outer.args.first()? else {
        return None;
    };
    let mut args = Vec::with_capacity(sprintf.args.len());
    for a in &sprintf.args {
        args.push(render_node(pass, a)?);
    }
    Some(TextEdit {
        pos: outer.pos().0 as u32,
        end: outer.end().0 as u32,
        new_text: format!("fmt.Errorf({})", args.join(", ")),
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1028 requires inspect analyzer".to_string())?
        .clone();
    let _ = pass
        .result_of::<typeindex::Index>(typeindex::analyzer())
        .ok_or_else(|| "S1028 requires typeindex analyzer".to_string())?;

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _| {
        // `code.EditMatch` replaces the matched `errors.New(fmt.Sprintf(…))`
        // with the replacement pattern re-printed: `fmt.Errorf(…)` carrying the
        // Sprintf call's own arguments.
        let edit = errorf_edit(pass, node);
        pending.push((
            match_pos(node),
            "should use fmt.Errorf(...) instead of errors.New(fmt.Sprintf(...))".into(),
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
                message: "Use fmt.Errorf".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
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
        requires: vec![inspect::analyzer(), typeindex::analyzer()],
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
