//! `shift` — check for shifts that exceed the width of the integer.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/shift` (dead-code pruning omitted).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::expr_to_int;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::expreq::{token_is_shift, token_is_shift_assign};
use crate::govet_util::format_expr;

fn type_bit_width(pass: &Pass<'_>, expr: &Expr) -> Option<i64> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let t = info.types.get(&expr.id())?.typ;
    let sizes = pass.types_sizes();
    let u = t.underlying(&artifacts.types);
    let size = sizes.sizeof(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        u,
    );
    if size <= 0 {
        return None;
    }
    Some(size * 8)
}

fn is_const_operand(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    info.types
        .get(&expr.id())
        .and_then(|tv| tv.val.as_ref())
        .is_some()
}

fn check_long_shift(pass: &Pass<'_>, node_pos: u32, x: &Expr, y: &Expr, pending: &mut Vec<(u32, String)>) {
    if is_const_operand(pass, x) {
        return;
    }
    let Some(amt) = expr_to_int(pass, y) else {
        return;
    };
    let Some(bits) = type_bit_width(pass, x) else {
        return;
    };
    if amt >= bits {
        // Upstream: `analysisutil.Format(pass.Fset, x)`. The old fallback here
        // was the literal string "x", so every operand that was not a bare
        // identifier — `s.f`, `a[0]`, `(i)` — reported the letter x.
        let name = format_expr(pass, x);
        pending.push((
            node_pos,
            format!("{name} ({bits} bits) too small for shift of {amt}"),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "assign requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt, BinaryExpr), pass.files(), |n| {
        match n {
            NodeRef::BinaryExpr(BinaryExpr { op, x, y, .. }) if token_is_shift(*op) => {
                check_long_shift(pass, x.pos().0 as u32, x, y, &mut pending);
            }
            NodeRef::AssignStmt(AssignStmt { tok, lhs, rhs, .. })
                if lhs.len() == 1 && rhs.len() == 1 && token_is_shift_assign(*tok) =>
            {
                check_long_shift(pass, lhs[0].pos().0 as u32, &lhs[0], &rhs[0], &mut pending);
            }
            _ => {}
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
        name: "shift",
        doc: "check for shifts that equal or exceed the width of the integer",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/shift",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
