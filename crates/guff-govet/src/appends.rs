//! `appends` — detect `append(x)` with nothing to append.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/appends`.

use std::sync::OnceLock;

use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::call_name;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "appends requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if call.args.len() != 1 {
            return;
        }
        // Upstream asks `typeutil.Callee(...).(*types.Builtin)` and compares
        // the name, so a local shadowing `append` is not reported and neither
        // is a user function called `append` in another package. `call_name`
        // resolves the object the same way (and unparenthesizes, which upstream
        // gets from `typeutil.Callee`'s own `astutil.Unparen`).
        if call_name(pass, &call.fun).as_deref() != Some("append") {
            return;
        }
        // ReportRangef(call, …): the range starts at the call, i.e. at `append`.
        pending.push(call.pos().0 as u32);
    });

    for pos in pending {
        pass.reportf(pos, "append with no values");
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "appends",
        doc: "check for missing values after append",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/appends",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
