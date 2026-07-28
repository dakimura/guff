//! `timeformat` — check for common mistakes in time.Format / time.Parse layouts.

use std::sync::OnceLock;

use guff::ast::{BasicLit, CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::govet_util::{expr_string_const, is_function_named, is_method_named};

const BAD_FORMAT: &str = "2006-02-01";
const GOOD_FORMAT: &str = "2006-01-02";

fn is_time_format_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    is_method_named(pass, call, "time", "Time", "Format")
        || is_function_named(pass, call, "time", "Parse")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "timeformat requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !is_time_format_call(pass, call) {
            return;
        }
        let Some(arg) = call.args.first() else {
            return;
        };
        let Some(s) = expr_string_const(pass, arg) else {
            return;
        };
        if let Some(idx) = s.find(BAD_FORMAT) {
            let msg = format!("{BAD_FORMAT} should be {GOOD_FORMAT}");
            let pos = if matches!(arg, Expr::BasicLit(BasicLit { .. })) {
                arg.pos().0 as u32 + idx as u32 + 1
            } else {
                arg.pos().0 as u32
            };
            pending.push((pos, msg));
        }
    });
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "timeformat",
        doc: "check for mistakes in time.Format and time.Parse layout strings",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/timeformat",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
