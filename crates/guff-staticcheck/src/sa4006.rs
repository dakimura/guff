//! SA4006 — assigned value never read before overwrite.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4006` (simplified; defers goyacc/generated filtering).

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
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

fn unparen_expr(expr: &Expr) -> &Expr {
    let mut cur = expr;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

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
    for &rid in guff_analysis::referrers(func, v) {
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

/// Sorted positions of every ident that uses or defines each object.
///
/// [`IdentIndex::value_is_read_before_redef`] is asked about one object at a
/// time, but the answer needs the whole package; walking the files per question
/// is quadratic in package size. One walk up front answers all of them.
#[derive(Default)]
struct IdentIndex {
    uses: HashMap<ObjectId, Vec<u32>>,
    defs: HashMap<ObjectId, Vec<u32>>,
}

impl IdentIndex {
    fn build(pass: &Pass<'_>) -> Self {
        let mut idx = Self::default();
        let Some(info) = pass.types_info() else {
            return idx;
        };
        for file in pass.files() {
            preorder(NodeRef::File(file), |n| {
                let NodeRef::Ident(id) = n else {
                    return true;
                };
                let Some(obj) = object_of(pass, id) else {
                    return true;
                };
                let pos = id.name_pos.0 as u32;
                if info.uses.contains_key(&id.id) {
                    idx.uses.entry(obj).or_default().push(pos);
                }
                if info.defs.get(&id.id).and_then(|d| *d) == Some(obj) {
                    idx.defs.entry(obj).or_default().push(pos);
                }
                true
            });
        }
        // Preorder is source order within a file, but files are independent.
        for v in idx.uses.values_mut() {
            v.sort_unstable();
        }
        for v in idx.defs.values_mut() {
            v.sort_unstable();
        }
        idx
    }

    fn first_after(map: &HashMap<ObjectId, Vec<u32>>, obj: ObjectId, pos: u32) -> Option<u32> {
        let v = map.get(&obj)?;
        v.get(v.partition_point(|&p| p <= pos)).copied()
    }

    /// Whether `obj` is read after `after_pos` before being redefined.
    ///
    /// Hybrid SSA sometimes drops receiver/arg loads (e.g. `renderer.Run(...)`
    /// after `renderer, err := ...`), producing SA4006 false positives. An AST
    /// use of the same object between this assign and the next def means the
    /// value was read — suppress the report. A later use only after an
    /// intervening def still counts as unused (classic overwrite pattern).
    fn value_is_read_before_redef(&self, obj: ObjectId, after_pos: u32) -> bool {
        match (
            Self::first_after(&self.uses, obj, after_pos),
            Self::first_after(&self.defs, obj, after_pos),
        ) {
            (Some(u), Some(d)) => u < d,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

fn ssa_unused_but_ast_read(
    pass: &Pass<'_>,
    idents: &IdentIndex,
    lhs: &Expr,
    assign_pos: u32,
) -> bool {
    let Expr::Ident(id) = lhs else {
        return false;
    };
    let Some(obj) = object_of(pass, id) else {
        return false;
    };
    idents.value_is_read_before_redef(obj, assign_pos)
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

    let exprs = ir.expr_values();
    // Only candidates that SSA already believes are unused consult it, and most
    // packages have none — build the walk-wide index on the first question.
    let idents: OnceCell<IdentIndex> = OnceCell::new();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(IncDecStmt, AssignStmt), pass.files(), |node| {
        match node {
            NodeRef::IncDecStmt(inc) => {
                if for_post_incs.contains(&(inc.tok_pos.0 as u32)) {
                    return;
                }
                let Some(ev) = exprs.get(&inc.x) else {
                    return;
                };
                let func = ir.prog.functions.get(ev.func);
                let v = ev.value;
                if matches!(v, Value::Const(_)) {
                    return;
                }
                if !has_use(func, v) {
                    // Body `i--` / `i++` that feeds the next loop iteration often
                    // lacks SSA uses under hybrid IR; AST still sees the read.
                    if let Expr::Ident(id) = unparen_expr(&inc.x) {
                        if let Some(obj) = object_of(pass, id) {
                            let idx = idents.get_or_init(|| IdentIndex::build(pass));
                            if idx.value_is_read_before_redef(obj, inc.tok_pos.0 as u32) {
                                return;
                            }
                        }
                    }
                    pending.push((
                        inc.tok_pos.0 as u32,
                        format!("this value of {} is never used", render_expr(&inc.x)),
                    ));
                }
            }
            NodeRef::AssignStmt(assign) => {
                // Upstream picks the first `src_funcs` entry that resolves the
                // first rhs or, failing that, the first lhs — i.e. the lower of
                // the two `src_funcs` positions.
                let fid = [assign.rhs.first(), assign.lhs.first()]
                    .into_iter()
                    .flatten()
                    .filter_map(|e| exprs.get(e))
                    .min_by_key(|ev| ev.order)
                    .map(|ev| ev.func);
                let Some(fid) = fid else {
                    return;
                };
                let func = ir.prog.functions.get(fid);
                if assign.lhs.len() > 1 && assign.rhs.len() == 1 {
                    if let Some((v, _)) = exprs.value_in(&ir.prog, fid, &assign.rhs[0]) {
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
                                        idents.get_or_init(|| IdentIndex::build(pass)),
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
                    // Field / index stores mutate addressable memory; hybrid SSA
                    // often drops the Store edge, but staticcheck does not flag
                    // `x.F = …` as an unused local assignment.
                    if !matches!(unparen_expr(lhs), Expr::Ident(_)) {
                        continue;
                    }
                    let val = exprs
                        .value_in(&ir.prog, fid, rhs)
                        .map(|(v, _)| v)
                        .or_else(|| {
                            if assign.tok != Some(Token::ASSIGN) {
                                exprs.value_in(&ir.prog, fid, lhs).map(|(v, _)| v)
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
                        if ssa_unused_but_ast_read(
                            pass,
                            idents.get_or_init(|| IdentIndex::build(pass)),
                            lhs,
                            assign.tok_pos.0 as u32,
                        ) {
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
