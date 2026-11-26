//! SA2000 — `(*sync.WaitGroup).Add` called inside the goroutine.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa2000`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr, Stmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_of_type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::render::render_expr;

fn is_waitgroup_add(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if is_call_to(pass, call, "(*sync.WaitGroup).Add") {
        return true;
    }
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen_expr(call.fun.as_ref()) else {
        return false;
    };
    if sel.name != "Add" {
        return false;
    }
    is_of_type_with_name(pass, x, "sync.WaitGroup")
        || is_of_type_with_name(pass, x, "*sync.WaitGroup")
}

/// The **first** statement of the body, unwrapping a leading block.
///
/// Upstream is a single pattern and the list is matched head-first:
///
/// ```text
/// (GoStmt (CallExpr (FuncLit _ call@(CallExpr (Symbol "(*sync.WaitGroup).Add") _):_) _))
/// ```
///
/// `call@(…):_` is head:tail, so only the body's first statement can match —
/// and a leading `BlockStmt` is matched as its own statement list, so
/// `go func() { { wg.Add(1) } }()` still does. Measured, all four ways round:
/// `{ defer wg.Done(); wg.Add(1) }` is silent (second inside the block),
/// `{ { wg.Add(1) } }` reports, and `_ = 1; { wg.Add(1) }` is silent.
///
/// guff scanned the whole body instead, so it reported a correct
/// `wg.Add(1)` that merely happened to sit inside some enclosing goroutine —
/// grafana/loki's `wire_http2_test.go:318`.
fn first_waitgroup_add<'a>(pass: &Pass<'_>, body: &'a [Stmt]) -> Option<&'a CallExpr> {
    match body.first()? {
        Stmt::ExprStmt(es) => match unparen_expr(&es.x) {
            Expr::CallExpr(call) if is_waitgroup_add(pass, call) => Some(call),
            _ => None,
        },
        Stmt::BlockStmt(block) => first_waitgroup_add(pass, &block.list),
        _ => None,
    }
}

fn check_go_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let Expr::FuncLit(lit) = unparen_expr(call.fun.as_ref()) else {
        return;
    };
    let body = &lit.body;
    let Some(add_call) = first_waitgroup_add(pass, &body.list) else {
        return;
    };
    // Upstream renders the whole `Add` call and reports the call node, not the
    // callee alone at the `(`: `wgs[0].Add(2 + 1)`, verified against
    // golangci-lint 2.12.2.
    let rendered = render_expr(&Expr::CallExpr(add_call.clone()));
    pending.push((
        add_call.pos().0 as u32,
        format!("should call {rendered} before starting the goroutine to avoid a race"),
    ));
}

fn unparen_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen_expr(&p.x),
        other => other,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // `code.Matches` walks the whole file, so a `go` statement counts wherever
    // it is written. The hand-rolled statement walk this replaces enumerated
    // statement kinds and knew about neither `switch`/`select` cases nor
    // function literals, so `go func(){ wg.Add(1) }()` inside any of them was
    // never looked at — two of grafana/loki's, and the one inside another
    // goroutine that made the rule report the wrong line.
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA2000 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(GoStmt), pass.files(), |node| {
        let NodeRef::GoStmt(g) = node else {
            return;
        };
        check_go_call(pass, &g.call, &mut pending);
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa2000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA2000",
        doc: "WaitGroup.Add called inside the goroutine",
        url: "https://staticcheck.dev/docs/checks/#SA2000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA2000 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa2000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa2000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
