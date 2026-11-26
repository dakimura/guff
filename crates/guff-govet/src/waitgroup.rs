//! `waitgroup` — `WaitGroup.Add` called from inside the new goroutine.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/waitgroup`.
//!
//! staticcheck's SA2000 asks almost the same question, and the two are not
//! interchangeable. SA2000 searches the whole goroutine body (recursing into
//! nested blocks) and reports the rendered call; this one matches a fixed stack
//! shape — `GoStmt / CallExpr / FuncLit / BlockStmt / ExprStmt / CallExpr` —
//! **and** requires the `ExprStmt` to be the block's *first* statement, then
//! reports at the `(` with a fixed message.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, FuncLit, SelectorExpr, Stmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_of_type_with_name, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_waitgroup_add(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if is_call_to(pass, call, "(*sync.WaitGroup).Add") {
        return true;
    }
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(call.fun.as_ref()) else {
        return false;
    };
    if sel.name != "Add" {
        return false;
    }
    // `typesinternal.IsMethodNamed` names the receiver's *base* type, so the
    // pointer and value spellings are the same method to upstream.
    is_of_type_with_name(pass, x, "sync.WaitGroup")
        || is_of_type_with_name(pass, x, "*sync.WaitGroup")
}

/// The `Add` call if `lit`'s body opens with one, as an expression statement.
fn leading_add<'a>(pass: &Pass<'_>, lit: &'a FuncLit) -> Option<&'a CallExpr> {
    // "ExprStmt must be Block's first stmt" — upstream compares the stack's
    // ExprStmt against `List[0]` by identity, so a `wg.Add(1)` on the second
    // line of the goroutine is not reported by this analyzer at all.
    let Stmt::ExprStmt(es) = lit.body.list.first()? else {
        return None;
    };
    let Expr::CallExpr(call) = unparen(&es.x) else {
        return None;
    };
    is_waitgroup_add(pass, call).then_some(call)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Upstream bails when the package does not import "sync" directly.
    if !pass.pkg().imports.contains_key("sync") {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "waitgroup requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(GoStmt), pass.files(), |n| {
        let NodeRef::GoStmt(go) = n else {
            return;
        };
        // The stack suffix puts the `FuncLit` directly under the `go`
        // statement's `CallExpr` — which is `Fun` for the usual
        // `go func(){…}()`, but any argument satisfies it just as well
        // (`go f(func(){…})`), so both are checked.
        let mut lits: Vec<&FuncLit> = Vec::new();
        if let Expr::FuncLit(lit) = unparen(go.call.fun.as_ref()) {
            lits.push(lit);
        }
        for arg in &go.call.args {
            if let Expr::FuncLit(lit) = unparen(arg) {
                lits.push(lit);
            }
        }
        for lit in lits {
            if let Some(call) = leading_add(pass, lit) {
                // `pass.Reportf(call.Lparen, …)` — the open paren, not the call.
                pending.push(call.lparen.0 as u32);
            }
        }
    });

    for pos in pending {
        pass.reportf(pos, "WaitGroup.Add called from inside new goroutine");
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "waitgroup",
        doc: "check for misuses of sync.WaitGroup",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/waitgroup",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
