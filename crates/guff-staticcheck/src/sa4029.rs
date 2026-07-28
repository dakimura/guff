//! SA4029 — ineffective attempt at sorting slice
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4029`.

use std::sync::OnceLock;

use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::ast::{CallExpr, Expr, Ident};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::selector_name;

use guff_types::arena::TypeData;
use crate::render::render_expr;

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

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4029 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
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
        pending.push((match_pos(node), format!("{typ} is a type, not a function, and {rhs} doesn't sort your values; consider using sort.{alt} instead")));
        true
    });
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        let NodeRef::AssignStmt(assign) = node else { return };
        let Some(rhs) = assign.rhs.first() else { return };
        let Some(name) = conversion_name(pass, rhs) else { return };
        let Some((typ, alt)) = slice_sort_type(&name) else { return };
        let Expr::Ident(target) = &assign.lhs[0] else { return };
        let info = pass.types_info().unwrap();
        let artifacts = pass.pkg().type_artifacts.as_ref().unwrap();
        let Some(tav) = info.types.get(&target.id) else { return };
        if !matches!(artifacts.types.get(tav.typ.underlying(&artifacts.types)), TypeData::Slice(_)) {
            return;
        }
        let rhs = render_expr(rhs);
        pending.push((
            assign.tok_pos.0 as u32,
            format!("{typ} is a type, not a function, and {rhs} doesn't sort your values; consider using sort.{alt} instead"),
        ));
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
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
