//! S1012 — replace time.Now().Sub with time.Since.
//!
//! Port of `honnef.co/go/tools/simple/s1012`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{self, is_call_to, type_func_name};

use crate::render::render_node;
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

fn is_now_sub(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = &*call.fun else {
        return false;
    };
    if sel.name != "Sub" {
        return false;
    }
    let Expr::CallExpr(now) = &**x else {
        return false;
    };
    if !is_call_to(pass, now, "time.Now") {
        return false;
    }
    let Some(obj) = pass.types_info().and_then(|info| info.uses.get(&sel.id).copied()) else {
        return false;
    };
    let Some(a) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    type_func_name(&a.types, &a.objects, &a.packages, obj) == "(time.Time).Sub"
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1012 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if is_now_sub(pass, call) {
            // `code.EditMatch` replaces the matched node with the replacement
            // pattern re-printed, so the whole `time.Now().Sub(x)` call goes and
            // `time.Since(x)` takes its place, with `x` printed from the AST.
            // `render_node`, not the message renderer: upstream's `EditMatch`
            // prints through `format.Node`, and a hand-rolled walker spaces
            // binary operators differently. That is invisible in a message and
            // is a byte difference on disk.
            let edit = call
                .args
                .first()
                .and_then(|arg| render_node(pass, arg))
                .map(|arg| TextEdit {
                    pos: call.pos().0 as u32,
                    end: call.end().0 as u32,
                    new_text: format!("time.Since({arg})"),
                });
            pending.push((
                match_pos(node),
                "should use time.Since instead of time.Now().Sub".into(),
                edit,
            ));
        }
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        // `report.FilterGenerated()` upstream: same gate, but the fix has to
        // ride along, so the diagnostic is built here rather than by `reportf`.
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message: message.clone(),
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with call to time.Since".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1012",
        doc: "replace time.Now().Sub with time.Since",
        url: "https://staticcheck.dev/docs/checks/#S1012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1012 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
