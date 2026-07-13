//! S1039 — unnecessary use of `fmt.Sprint`.
//!
//! Port of `honnef.co/go/tools/simple/s1039`.

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr};
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1039 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to_any(pass, call, &["fmt.Sprint", "fmt.Sprintf"]) || call.args.len() != 1 {
            return;
        };
        let Expr::BasicLit(BasicLit { .. }) = &call.args[0] else {
            return;
        };
        let Some(val) = expr_to_string(pass, &call.args[0]) else {
            return;
        };
        if val.contains('%') {
            return;
        }
        pending.push((match_pos(node), "unnecessary use of fmt.Sprint".into()));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1039_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1039",
        doc: "unnecessary use of fmt.Sprint",
        url: "https://staticcheck.dev/docs/checks/#S1039",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1039_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1039_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
