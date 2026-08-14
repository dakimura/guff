//! SA4006 — assigned value never read before overwrite.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4006` (simplified; defers goyacc/generated filtering).

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Expr, Ident, Stmt};
use guff::node_mask;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code::{example_func_spans, in_example_func, object_of};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{iter_non_debug, referrers, AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::instr::{Extract, InstrData};
use guff_ssa::value::Value;
use guff_types::ObjectId;

use crate::render::render_expr;

/// Upstream reports the assignment *node*, whose `Pos()` is the start of its
/// first left-hand expression — not the `=` / `:=` token. The two differ
/// whenever the reported variable is not the first: `if _, ok := i.(int)` is
/// reported on the `_`. Verified against golangci-lint 2.12.2.
fn assign_pos(assign: &guff::ast::AssignStmt) -> u32 {
    assign
        .lhs
        .first()
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

/// End of an assignment statement: the end of its last right-hand expression.
fn assign_end(assign: &guff::ast::AssignStmt) -> u32 {
    assign
        .rhs
        .last()
        .map(|e| e.end().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

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
    /// `(position the redefinition takes effect, key of the statement list the
    /// assignment sits in)`. The key is `0` for declarations and `:=`, which
    /// count as redefinitions from anywhere; see
    /// [`IdentIndex::value_is_read_before_redef`].
    ///
    /// The position is the **end of the assignment**, not the target ident, so
    /// that a read on the redefining statement's own right-hand side sorts
    /// *before* it. Go evaluates the right-hand side first, so
    ///
    /// ```ignore
    /// decoder := u.bodyDecoder(file.Body)
    /// decoder = decoder.SkipFields("type_url")   // reads, then overwrites
    /// ```
    ///
    /// does not make the first value dead — even though the target ident is to
    /// the left of the read. Comparing target idents flagged three live values
    /// across consul and grafana.
    defs: HashMap<ObjectId, Vec<(u32, u32)>>,
    /// Statement-list key for each plain-assignment target, by ident position.
    blocks: HashMap<u32, u32>,
    /// End of the enclosing assignment for each of its target idents, by ident
    /// position. See [`Self::defs`].
    assign_ends: HashMap<u32, u32>,
    /// `(start, end)` of every loop body, so a value that is read earlier in
    /// the same loop can be recognised as live across the back edge.
    loops: Vec<(u32, u32)>,
}

impl IdentIndex {
    fn build(pass: &Pass<'_>) -> Self {
        let mut idx = Self::default();
        let Some(info) = pass.types_info() else {
            return idx;
        };
        // go/types records the `x` of `x = v` in `Uses`, not `Defs` — only `:=`
        // and declarations produce a `Def`. Counting those as reads made every
        // plain overwrite look like "the value is read later", which suppressed
        // the classic SA4006 pattern (`c := a; c = b; _ = c`). Collect
        // assignment targets, keyed by the statement list they are a direct
        // member of, so a later assignment can be recognised as a redefinition
        // only when it is straight-line code — see `value_is_read_before_redef`.
        // One walk does both jobs: preorder visits a statement list before the
        // idents inside it, so `blocks` is already populated for an ident's own
        // block by the time that ident is reached.
        for file in pass.files() {
            preorder(NodeRef::File(file), |n| {
                match n {
                    NodeRef::ForStmt(f) => {
                        idx.loops
                            .push((f.body.lbrace.0 as u32, f.body.rbrace.0 as u32));
                    }
                    NodeRef::RangeStmt(r) => {
                        idx.loops
                            .push((r.body.lbrace.0 as u32, r.body.rbrace.0 as u32));
                    }
                    _ => {}
                }
                let list: Option<(u32, &[Stmt])> = match n {
                    NodeRef::BlockStmt(b) => Some((b.lbrace.0 as u32, &b.list)),
                    NodeRef::CaseClause(c) => Some((c.case.0 as u32, &c.body)),
                    NodeRef::CommClause(c) => Some((c.case.0 as u32, &c.body)),
                    _ => None,
                };
                if let Some((key, list)) = list {
                    for stmt in list {
                        if let Stmt::AssignStmt(assign) = stmt {
                            let end = assign_end(assign);
                            for lhs in &assign.lhs {
                                if let Expr::Ident(id) = unparen_expr(lhs) {
                                    idx.blocks.insert(id.name_pos.0 as u32, key);
                                    idx.assign_ends.insert(id.name_pos.0 as u32, end);
                                }
                            }
                        }
                    }
                }
                let NodeRef::Ident(id) = n else {
                    return true;
                };
                let Some(obj) = object_of(pass, id) else {
                    return true;
                };
                let pos = id.name_pos.0 as u32;
                let target_block = idx.blocks.get(&pos).copied();
                if info.uses.contains_key(&id.id) && target_block.is_none() {
                    idx.uses.entry(obj).or_default().push(pos);
                }
                // A redefinition takes effect at the end of its assignment, not
                // at the target ident (see `defs`). Declarations outside an
                // assignment (`var x T`, parameters) have no such end.
                let redef_pos = idx.assign_ends.get(&pos).copied().unwrap_or(pos);
                if info.defs.get(&id.id).and_then(|d| *d) == Some(obj) {
                    idx.defs.entry(obj).or_default().push((redef_pos, 0));
                } else if let Some(key) = target_block {
                    idx.defs.entry(obj).or_default().push((redef_pos, key));
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

    /// Whether `obj` is read anywhere inside a loop body that contains `pos`.
    fn read_in_enclosing_loop(&self, obj: ObjectId, pos: u32) -> bool {
        let Some(uses) = self.uses.get(&obj) else {
            return false;
        };
        self.loops
            .iter()
            .filter(|(start, end)| *start <= pos && pos <= *end)
            .any(|(start, end)| {
                uses[uses.partition_point(|&p| p < *start)..]
                    .first()
                    .is_some_and(|&p| p <= *end)
            })
    }

    fn first_use_after(&self, obj: ObjectId, pos: u32) -> Option<u32> {
        let v = self.uses.get(&obj)?;
        v.get(v.partition_point(|&p| p <= pos)).copied()
    }

    /// First redefinition of `obj` after `pos` that is guaranteed to run: a
    /// declaration / `:=` (key `0`), or a plain assignment in the same
    /// statement list as the assignment being judged.
    fn first_redef_after(&self, obj: ObjectId, pos: u32, block: Option<u32>) -> Option<u32> {
        let v = self.defs.get(&obj)?;
        v[v.partition_point(|&(p, _)| p <= pos)..]
            .iter()
            .find(|(_, key)| *key == 0 || Some(*key) == block)
            .map(|(p, _)| *p)
    }

    /// Whether `obj` is read after `after_pos` before being redefined.
    ///
    /// Hybrid SSA sometimes drops receiver/arg loads (e.g. `renderer.Run(...)`
    /// after `renderer, err := ...`), producing SA4006 false positives. An AST
    /// use of the same object between this assign and the next def means the
    /// value was read — suppress the report. A later use only after an
    /// intervening def still counts as unused (classic overwrite pattern).
    ///
    /// Callers pass the **end** of the assignment, so a read on its own
    /// right-hand side (`x = append(x, y...)`) does not count: that reads the
    /// old value, which is exactly what makes the new one unused.
    ///
    /// A later assignment only counts as a redefinition when it sits in the
    /// same statement list. A branch cannot be assumed to run, so
    ///
    /// ```ignore
    /// loadingRules := clientcmd.NewDefaultClientConfigLoadingRules()
    /// if len(settings.KubeConfig) > 0 {
    ///     loadingRules = &clientcmd.ClientConfigLoadingRules{…}
    /// }
    /// // loadingRules read here — the first value is live on the other path
    /// ```
    ///
    /// is not an overwrite. Treating it as one flagged four live values across
    /// caddy and helm.
    fn value_is_read_before_redef(&self, obj: ObjectId, after_pos: u32, block: Option<u32>) -> bool {
        match (
            self.first_use_after(obj, after_pos),
            self.first_redef_after(obj, after_pos, block),
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
    let block = idents.blocks.get(&(id.name_pos.0 as u32)).copied();
    // A loop back edge carries the value to reads that appear *earlier* in the
    // source, which position ordering alone cannot see:
    //
    // ```ignore
    // for … {
    //     ca.Append(…)                       // reads the value assigned below
    //     …
    //     newChunk, _, ca, err = ca.AppendFloatHistogram(…)
    // }
    // ```
    //
    // Any read of the object anywhere inside an enclosing loop means the value
    // is live. Without this, prometheus' `tsdb/chunks/chunks.go:190` was a
    // false positive.
    if idents.read_in_enclosing_loop(obj, assign_pos) {
        return true;
    }
    idents.value_is_read_before_redef(obj, assign_pos, block)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4006 requires inspect analyzer".to_string())?
        .clone();

    let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) else {
        return Ok(None);
    };

    let exprs = ir.expr_values();
    // Only candidates that SSA already believes are unused consult it, and most
    // packages have none — build the walk-wide index on the first question.
    let idents: OnceCell<IdentIndex> = OnceCell::new();
    // `irutil.IsExample` is the first thing upstream's loop over SrcFuncs asks,
    // before it even looks at `fn.Source()`.
    let examples = example_func_spans(pass);
    let mut pending = Vec::new();
    // Upstream walks `*ast.AssignStmt` only: `n++` is an `*ast.IncDecStmt` and is
    // never examined, so `func f(n int) { n++ }` is not a finding. Verified
    // against golangci-lint 2.12.2.
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        match node {
            NodeRef::AssignStmt(assign) => {
                // `irutil.IsExample` is asked per `SrcFuncs` entry, before the
                // body is looked at, so every assignment inside a runnable
                // example is skipped whole. The spans were already being
                // computed here but never consulted, which is how
                // `tsdb/example_test.go:58` became a guff-only finding on
                // prometheus (COMPAT-HARDENING §4, 2026-08-13).
                if in_example_func(&examples, assign_pos(assign)) {
                    return;
                }
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
                        for rid in iter_non_debug(referrers(func, v), func) {
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
                                        assign_end(assign),
                                    ) {
                                        continue;
                                    }
                                    pending.push((
                                        assign_pos(assign),
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
                    // Upstream asks `fn.ValueForExpr(rhs)` and nothing else, so
                    // a compound assignment is judged by its right-hand side:
                    // `n += 1` yields the constant `1` and is skipped below.
                    let Some((v, _)) = exprs.value_in(&ir.prog, fid, rhs) else {
                        continue;
                    };
                    // A conversion that only re-labels an existing value —
                    // `MySlice(y)` (ChangeType) or boxing into an interface
                    // (MakeInterface) — is not a finding upstream, while a real
                    // conversion (`string(b)`, a Convert) is. Verified against
                    // golangci-lint 2.12.2 with all four shapes side by side.
                    if let Value::Instr(iid) = v {
                        if matches!(
                            func.instrs.get(iid),
                            InstrData::ChangeType(_) | InstrData::MakeInterface(_)
                        ) {
                            continue;
                        }
                    }
                    if matches!(v, Value::Const(_)) {
                        continue;
                    }
                    if !has_use(func, v) {
                        if ssa_unused_but_ast_read(
                            pass,
                            idents.get_or_init(|| IdentIndex::build(pass)),
                            lhs,
                            assign_end(assign),
                        ) {
                            continue;
                        }
                        pending.push((
                            assign_pos(assign),
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
