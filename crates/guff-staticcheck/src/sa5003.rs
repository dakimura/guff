//! SA5003 — defers in infinite loops will never execute.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5003`.
//!
//! The body scan is `ast.Inspect` over the *whole* loop body, pruned at
//! `FuncLit`. That breadth is the check: a `return` anywhere in the body means
//! the loop can be left, and a `defer` anywhere in it is a defer that will not
//! run. A walker that only descends into the statement kinds it names gets
//! both directions wrong at once — see the fixtures for the eleven shapes a
//! `switch`/`select`/labelled-statement blind spot moved.

use std::sync::OnceLock;

use guff::ast::ForStmt;
use guff::node_mask;
use guff::token::Token;
use guff::walk::{preorder_prune, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_loop(loop_: &ForStmt, pending: &mut Vec<u32>) {
    if loop_.cond.is_some() {
        return;
    }
    let mut might_exit = false;
    let mut defers = Vec::new();
    // `preorder_prune` is `ast.Inspect`: returning false stops the descent into
    // that node and the walk carries on with its siblings. Upstream relies on
    // exactly that — `return false` on a `return`/`break` does not end the
    // scan, it only stops looking inside the statement that already answered.
    preorder_prune(NodeRef::BlockStmt(&loop_.body), |n| match n {
        NodeRef::ReturnStmt(_) => {
            might_exit = true;
            false
        }
        // Upstream's own TODO: a `break` inside a `switch` or `select` leaves
        // that statement, not the loop, and this counts it anyway. Keeping the
        // false negative is what keeps the two tools equal.
        NodeRef::BranchStmt(b) if b.tok == Token::BREAK => {
            might_exit = true;
            false
        }
        NodeRef::DeferStmt(d) => {
            defers.push(d.defer_.0 as u32);
            true
        }
        // A defer or return inside a function literal belongs to the literal.
        NodeRef::FuncLit(_) => false,
        _ => true,
    });
    if might_exit {
        return;
    }
    pending.extend(defers);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5003 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |n| {
        let NodeRef::ForStmt(loop_) = n else {
            return;
        };
        check_loop(loop_, &mut pending);
    });
    for pos in pending {
        pass.report_unless_generated(pos, "defers in this infinite loop will never run");
    }
    Ok(None)
}

fn sa5003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5003",
        doc: "defers in infinite loops will never execute",
        url: "https://staticcheck.dev/docs/checks/#SA5003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
