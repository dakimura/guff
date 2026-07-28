//! SA1012 — nil `context.Context` passed to a function.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1012`.

use std::sync::OnceLock;

use guff::ast::CallExpr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_target_object, first_param_type, is_nil, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::ObjectData;

const MSG: &str = "do not pass a nil Context, even if a function permits it; pass context.TODO if you are unsure about which Context to use";

fn check_call(pass: &Pass<'_>, call: &CallExpr) -> Option<u32> {
    let first = call.args.first()?;
    if !is_nil(pass, first) {
        return None;
    }
    let obj = call_target_object(pass, &call.fun)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(artifacts.objects.get(obj), ObjectData::Func(_)) {
        return None;
    }
    let param_typ = first_param_type(pass, obj)?;
    if !type_with_name(pass, param_typ, "context.Context") {
        return None;
    }
    Some(first.pos().0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1012 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<u32> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some(pos) = check_call(pass, call) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, MSG);
    }
    Ok(None)
}

fn sa1012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1012",
        doc: "do not pass a nil context.Context to a function",
        url: "https://staticcheck.dev/docs/checks/#SA1012",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1012 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
