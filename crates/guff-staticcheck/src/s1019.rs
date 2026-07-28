//! S1019 — simplify make call by omitting redundant arguments.
//!
//! Port of `honnef.co/go/tools/simple/s1019`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_integer_literal, same_non_dynamic};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::{Chan, TypeData, TypeId};

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn is_chan_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let types = &artifacts.types;
    matches!(types.get(typ.underlying(types)), TypeData::Chan(Chan { .. }))
}

fn check_make(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    if call_name(pass, &call.fun)? != "make" {
        return None;
    }
    if call.args.len() == 2 && is_integer_literal(pass, &call.args[1], 0) {
        let typ = type_of_expr(pass, &call.args[0])?;
        if is_chan_type(pass, typ) {
            return Some("should use make(T) instead of make(T, 0)".into());
        }
    }
    if call.args.len() == 3
        && same_non_dynamic(pass, &call.args[1], &call.args[2])
    {
        return Some("should use make(T, size) instead of make(T, size, size)".into());
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1019 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if let Some(msg) = check_make(pass, call) {
            pending.push((call.lparen.0 as u32, msg));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1019",
        doc: "simplify make call by omitting redundant arguments",
        url: "https://staticcheck.dev/docs/checks/#S1019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
