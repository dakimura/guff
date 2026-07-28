//! SA4006 — assigned value never read before overwrite.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4006` (simplified; defers goyacc/generated filtering).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Expr, Ident, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{filter_debug, referrers, AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::instr::{Extract, InstrData};
use guff_ssa::value::Value;
use guff_types::ObjectId;

use crate::render::render_expr;

fn has_use(func: &guff_ssa::function::Function, v: Value) -> bool {
    let mut seen = HashSet::new();
    has_use_rec(func, v, &mut seen)
}

fn has_use_rec(
    func: &guff_ssa::function::Function,
    v: Value,
    seen: &mut HashSet<Value>,
) -> bool {
    if !seen.insert(v) {
        return false; // cyclic Phi chain (seen under incomplete hybrid SSA)
    }
    let refs = filter_debug(guff_analysis::referrers(func, v), func);
    for &rid in &refs {
        match func.instrs.get(rid) {
            InstrData::Phi(_) => {
                if has_use_rec(func, Value::Instr(rid), seen) {
                    return true;
                }
            }
            InstrData::DebugRef(_) => {}
            // Match upstream: any non-DebugRef/Phi referrer (including Store) counts.
            // Local unused assigns still fire because lifting removes the spill Store
            // when the value stays in registers; heap field stores correctly count as uses.
            _ => return true,
        }
    }
    false
}

/// Whether `obj` is read after `after_pos` before being redefined.
///
/// Hybrid SSA sometimes drops receiver/arg loads (e.g. `renderer.Run(...)`
/// after `renderer, err := ...`), producing SA4006 false positives. An AST
/// use of the same object between this assign and the next def means the
/// value was read — suppress the report. A later use only after an intervening
/// def still counts as unused (classic overwrite pattern).
fn ast_value_is_read_before_redef(pass: &Pass<'_>, obj: ObjectId, after_pos: u32) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let mut next_use: Option<u32> = None;
    let mut next_def: Option<u32> = None;
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::Ident(id) = n else {
                return true;
            };
            let pos = id.name_pos.0 as u32;
            if pos <= after_pos {
                return true;
            }
            if object_of(pass, id) != Some(obj) {
                return true;
            }
            if info.uses.contains_key(&id.id) {
                next_use = Some(next_use.map_or(pos, |u| u.min(pos)));
            }
            if info.defs.get(&id.id).and_then(|d| *d) == Some(obj) {
                next_def = Some(next_def.map_or(pos, |d| d.min(pos)));
            }
            true
        });
    }
    match (next_use, next_def) {
        (Some(u), Some(d)) => u < d,
        (Some(_), None) => true,
        _ => false,
    }
}

fn ssa_unused_but_ast_read(
    pass: &Pass<'_>,
    lhs: &Expr,
    assign_pos: u32,
) -> bool {
    let Expr::Ident(id) = lhs else {
        return false;
    };
    let Some(obj) = object_of(pass, id) else {
        return false;
    };
    ast_value_is_read_before_redef(pass, obj, assign_pos)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4006 requires inspect analyzer".to_string())?
        .clone();

    let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) else {
        return Ok(None);
    };

    // Collect ForStmt post IncDec positions so we don't flag loop increments
    // (`for ; i >= 0; i--`) — the updated value is read by the condition / after break.
    let mut for_post_incs = HashSet::new();
    inspect.preorder_typed(node_mask!(ForStmt), pass.files(), |node| {
        let NodeRef::ForStmt(fs) = node else {
            return;
        };
        if let Some(Stmt::IncDecStmt(inc)) = fs.post.as_deref() {
            for_post_incs.insert(inc.tok_pos.0 as u32);
        }
    });

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(IncDecStmt, AssignStmt), pass.files(), |node| {
        match node {
            NodeRef::IncDecStmt(inc) => {
                if for_post_incs.contains(&(inc.tok_pos.0 as u32)) {
                    return;
                }
                let func = ir.src_funcs.iter().find_map(|&fid| {
                    let f = ir.prog.functions.get(fid);
                    f.value_for_expr(&inc.x).map(|_| f)
                });
                let Some(func) = func else {
                    return;
                };
                let Some((v, _)) = func.value_for_expr(&inc.x) else {
                    return;
                };
                if matches!(v, Value::Const(_)) {
                    return;
                }
                if !has_use(func, v) {
                    pending.push((
                        inc.tok_pos.0 as u32,
                        format!("this value of {} is never used", render_expr(&inc.x)),
                    ));
                }
            }
            NodeRef::AssignStmt(assign) => {
                let func = ir.src_funcs.iter().find_map(|&fid| {
                    let f = ir.prog.functions.get(fid);
                    assign
                        .rhs
                        .first()
                        .and_then(|e| f.value_for_expr(e))
                        .or_else(|| assign.lhs.first().and_then(|e| f.value_for_expr(e)))
                        .map(|_| f)
                });
                let Some(func) = func else {
                    return;
                };
                if assign.lhs.len() > 1 && assign.rhs.len() == 1 {
                    if let Some((v, _)) = func.value_for_expr(&assign.rhs[0]) {
                        for rid in filter_debug(referrers(func, v), func) {
                            if let InstrData::Extract(Extract { index, .. }) = func.instrs.get(rid)
                            {
                                let lhs = &assign.lhs[*index];
                                if matches!(lhs, Expr::Ident(Ident { name, .. }) if name == "_") {
                                    continue;
                                }
                                if !has_use(func, Value::Instr(rid)) {
                                    if ssa_unused_but_ast_read(
                                        pass,
                                        lhs,
                                        assign.tok_pos.0 as u32,
                                    ) {
                                        continue;
                                    }
                                    pending.push((
                                        assign.tok_pos.0 as u32,
                                        format!(
                                            "this value of {} is never used",
                                            render_expr(lhs)
                                        ),
                                    ));
                                }
                            }
                        }
                    }
                    return;
                }
                if assign.lhs.len() != assign.rhs.len() {
                    return;
                }
                for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
                    if matches!(lhs, Expr::Ident(Ident { name, .. }) if name == "_") {
                        continue;
                    }
                    let val = func
                        .value_for_expr(rhs)
                        .map(|(v, _)| v)
                        .or_else(|| {
                            if assign.tok != Some(Token::ASSIGN) {
                                func.value_for_expr(lhs).map(|(v, _)| v)
                            } else {
                                None
                            }
                        });
                    let Some(v) = val else {
                        continue;
                    };
                    if matches!(v, Value::Const(_)) {
                        continue;
                    }
                    if !has_use(func, v) {
                        if ssa_unused_but_ast_read(pass, lhs, assign.tok_pos.0 as u32) {
                            continue;
                        }
                        pending.push((
                            assign.tok_pos.0 as u32,
                            format!("this value of {} is never used", render_expr(lhs)),
                        ));
                    }
                }
            }
            _ => {}
        }
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa4006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4006",
        doc: "a value assigned to a variable is never read before being overwritten",
        url: "https://staticcheck.dev/docs/checks/#SA4006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer(), buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
