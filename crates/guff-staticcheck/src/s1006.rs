//! S1006 — use `for { ... }` for infinite loops.
//!
//! Port of `honnef.co/go/tools/simple/s1006`.

use std::sync::OnceLock;

use guff::ast::ForStmt;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{bool_const, is_bool_const};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_for(pass: &Pass<'_>, loop_: &ForStmt) -> Option<u32> {
    if loop_.init.is_some() || loop_.post.is_some() {
        return None;
    }
    let cond = loop_.cond.as_ref()?;
    if !is_bool_const(pass, cond) || !bool_const(pass, cond) {
        return None;
    }
    Some(loop_.for_.0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<u32> = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |n| {
        let NodeRef::ForStmt(loop_) = n else {
            return;
        };
        if let Some(pos) = check_for(pass, loop_) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, "should use for {} instead of for true {}");
    }
    Ok(None)
}

fn s1006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1006",
        doc: "use for { ... } for infinite loops",
        url: "https://staticcheck.dev/docs/checks/#S1006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1006 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
