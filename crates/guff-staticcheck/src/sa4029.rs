//! SA4029 — ineffective attempt at sorting slice
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4029`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass, Diagnostic, SuggestedFix, TextEdit};


use guff::ast::{CallExpr, Expr};
use guff::walk::NodeRef;
use guff_analysis::code::selector_name;

use guff_types::arena::TypeData;
use crate::render::{render_expr, render_node};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn slice_sort_type(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "sort.Float64Slice" => Some(("sort.Float64Slice", "Float64s")),
        "sort.IntSlice" => Some(("sort.IntSlice", "Ints")),
        "sort.StringSlice" => Some(("sort.StringSlice", "Strings")),
        _ => None,
    }
}

fn conversion_name(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    if let Expr::CallExpr(CallExpr { fun, .. }) = expr {
        if let Expr::SelectorExpr(sel) = fun.as_ref() {
            return selector_name(pass, sel);
        }
    }
    None
}

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(AssignStmt target@(Ident _) "=" (CallExpr typ@(Symbol (Or "sort.Float64Slice" "sort.IntSlice" "sort.StringSlice")) [target]))"#))
}

/// An `AssignStmt` spans its first left-hand expression to its last right-hand
/// one; it has no `pos()`/`end()` of its own.
fn assign_pos(a: &guff::ast::AssignStmt) -> u32 {
    a.lhs.first().map(|e| e.pos().0).unwrap_or(0) as u32
}

fn assign_end(a: &guff::ast::AssignStmt) -> u32 {
    a.rhs.last().map(|e| e.end().0).unwrap_or(0) as u32
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4029 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    matches(pass, &inspect, pat(), |node, m| {
        let NodeRef::AssignStmt(assign) = node else { return true };
        let Some(target) = m.state.get("target").and_then(|v| v.as_ident()) else { return true };
        let info = pass.types_info().unwrap();
        let artifacts = pass.pkg().type_artifacts.as_ref().unwrap();
        let Some(tav) = info.types.get(&target.id) else { return true };
        if !matches!(artifacts.types.get(tav.typ.underlying(&artifacts.types)), TypeData::Slice(_)) {
            return true;
        }
        let typ = m
            .state
            .get("typ")
            .and_then(|v| v.as_object())
            .and_then(|o| guff_analysis::code::object_call_name(pass, o))
            .or_else(|| conversion_name(pass, &assign.rhs[0]))
            .unwrap_or_default();
        let Some((typ, alt)) = slice_sort_type(&typ) else { return true };
        let rhs = render_expr(&assign.rhs[0]);
        // `edit.ReplaceWithNode(fset, node, sort.<Alt>(target))`: the whole
        // assignment becomes the call, so `x = sort.StringSlice(x)` turns into
        // `sort.Strings(x)` — the assignment itself goes with it.
        let edit = render_node(pass, &guff::ast::Expr::Ident(target.clone())).map(|t| TextEdit {
            pos: assign_pos(assign),
            end: assign_end(assign),
            new_text: format!("sort.{alt}({t})"),
        });
        pending.push((
            match_pos(node),
            format!("{typ} is a type, not a function, and {rhs} doesn't sort your values; consider using sort.{alt} instead"),
            edit,
        ));
        true
    });
    // Upstream is the pattern and nothing else — see the note in sa4022 about
    // the duplicate hand-rolled walk this used to carry (reported again at
    // `tok_pos`, hidden by `issues.uniq-by-line`).
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.reportf(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Replace with call to sort".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}


fn sa4029_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4029",
        doc: "ineffective attempt at sorting slice",
        url: "https://staticcheck.dev/docs/checks/#SA4029",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4029_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4029_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
