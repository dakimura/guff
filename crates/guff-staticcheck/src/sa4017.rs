//! SA4017 — discarding return value of pure function call
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4017`.
//! Pure stdlib names mirror `analysis/facts/purity.pureStdlib` (package funcs).
//!
//! Upstream does not flag `_ = pure()` / `x := pure()` — only calls whose
//! results are unused as expression statements (no assignment). guff's blank
//! `LValue::store` is a no-op (like go/ssa), so SSA referrers alone would
//! still report blank-assigns; we skip CallExprs that appear as assign RHS.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::buildir;
use guff_analysis::{has_non_debug_referrer, is_call_to_any, referrers, short_call_name};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::value::Value;

/// Hard-coded pure stdlib functions from honnef `purity.pureStdlib` (non-method entries).
/// Method forms like `(time.Time).Add` are DEFERRED until SSA callee matching covers them.
const PURE_FUNCS: &[&str] = &[
    "errors.New",
    "fmt.Errorf",
    "fmt.Sprintf",
    "fmt.Sprint",
    "sort.Reverse",
    "strings.Map",
    "strings.Repeat",
    "strings.Replace",
    "strings.Title",
    "strings.ToLower",
    "strings.ToLowerSpecial",
    "strings.ToTitle",
    "strings.ToTitleSpecial",
    "strings.ToUpper",
    "strings.ToUpperSpecial",
    "strings.Trim",
    "strings.TrimFunc",
    "strings.TrimLeft",
    "strings.TrimLeftFunc",
    "strings.TrimPrefix",
    "strings.TrimRight",
    "strings.TrimRightFunc",
    "strings.TrimSpace",
    "strings.TrimSuffix",
    // Extra entries historically present in guff (also pure in practice).
    "strconv.Itoa",
    "strconv.FormatInt",
];

fn call_pos(expr: &Expr) -> Option<u32> {
    match expr {
        Expr::CallExpr(c) => Some(c.lparen.0 as u32),
        Expr::ParenExpr(p) => call_pos(&p.x),
        _ => None,
    }
}

/// Positions of CallExprs that are (part of) an assignment RHS — not ExprStmt discards.
fn assigned_call_positions(pass: &Pass<'_>) -> HashSet<u32> {
    let mut out = HashSet::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::AssignStmt(a) = n {
                for r in &a.rhs {
                    if let Some(pos) = call_pos(r) {
                        out.insert(pos);
                    }
                }
            }
            true
        });
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4017 requires buildir analyzer".to_string())?;
    let assigned = assigned_call_positions(pass);
    let mut pending = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                let val = Value::Instr(iid);
                if has_non_debug_referrer(referrers(func, val), func) {
                    continue;
                }
                let pos = func.pos(iid).0 as u32;
                if assigned.contains(&pos) {
                    continue;
                }
                if !is_call_to_any(&ir.prog, call, PURE_FUNCS) {
                    continue;
                }
                let name = short_call_name(&ir.prog, call).unwrap_or_default();
                pending.push((
                    pos,
                    format!("{name} doesn't have side effects and its return value is ignored"),
                ));
            }
        }
    }
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4017",
        doc: "discarding return value of pure function call",
        url: "https://staticcheck.dev/docs/checks/#SA4017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
