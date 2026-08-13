//! SA1013 — `io.Seeker.Seek` called with whence constant as first argument.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1013`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_io_seek_whence, is_method_val, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

const MSG: &str =
    "the first argument of io.Seeker is the offset, but an io.Seek* constant is being used instead";

fn check_seek_call(pass: &Pass<'_>, call: &CallExpr) -> Option<u32> {
    if call.args.len() < 2 {
        return None;
    }
    // Upstream matches through `pattern`, which strips parentheses at every
    // level: `f.Seek((io.SeekStart), 0)` and `(f.Seek)(io.SeekStart, 0)` are
    // both the pattern's `(CallExpr fun@(SelectorExpr _ (Ident "Seek")) …)`.
    let Expr::SelectorExpr(sel) = unparen(call.fun.as_ref()) else {
        return None;
    };
    if !is_method_val(pass, sel, "Seek") {
        return None;
    }
    if !is_io_seek_whence(pass, unparen(&call.args[0])) {
        return None;
    }
    Some(call.fun.pos().0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1013 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<u32> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some(pos) = check_seek_call(pass, call) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, MSG);
    }
    Ok(None)
}

fn sa1013_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1013",
        doc: "io.Seeker.Seek called with whence constant as first argument",
        url: "https://staticcheck.dev/docs/checks/#SA1013",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1013 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1013_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1013_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
