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
use guff_analysis::passes::facts::ctrlflow;
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
    /// For an assignment that a `return` follows in its own statement list: the
    /// position of that `return`.
    ///
    /// Read together with [`Self::loops`]: a read that appears *above* the
    /// assignment can only be reached by coming round the loop again, and a
    /// `return` in the assignment's own statement list means that never
    /// happens. This does **not** say the value is dead — a read *below* the
    /// assignment is still a read, which is what `value_is_read_before_redef`
    /// answers.
    ///
    /// opentofu `internal/legacy/tofu/state.go:442` is the shape —
    ///
    /// ```ignore
    /// for _, is := range lists {
    ///     for i, instance := range is {
    ///         if instance == v {
    ///             is, is[len(is)-1] = append(is[:i], is[i+1:]...), nil
    ///             return
    /// ```
    ///
    /// — where the only read of `is` is the `range` header *above* the
    /// assignment, which the position index reads as a read inside an
    /// enclosing loop.
    returns_after: HashMap<u32, u32>,
    /// `if` keyword position -> the key of the statement list the `if`
    /// statement is a direct member of. A branch redefinition can only be
    /// unconditional for the code after the `if` when the `if` itself is in the
    /// same statement list as the assignment being judged.
    if_lists: HashMap<u32, u32>,
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
                    for stmt in list.iter() {
                        if let Stmt::IfStmt(ifs) = stmt {
                            idx.if_lists.insert(ifs.if_.0 as u32, key);
                        }
                    }
                    for (i, stmt) in list.iter().enumerate() {
                        if let Stmt::AssignStmt(assign) = stmt {
                            let end = assign_end(assign);
                            if let Some(ret) = list[i + 1..].iter().find_map(|s| match s {
                                Stmt::ReturnStmt(r) => Some(r.return_.0 as u32),
                                _ => None,
                            }) {
                                idx.returns_after.insert(end, ret);
                            }
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

    /// Whether the back edge of an enclosing loop can carry the value assigned
    /// at `pos` to a read of `obj`.
    ///
    /// The read has to come *before* the iteration redefines `obj`. A read that
    /// a `:=` at the top of the body already overwrote is reading this
    /// iteration's value, not the one the back edge brought in:
    ///
    /// ```ignore
    /// for … {
    ///     pp, moreDiags := start(i)          // redefines first …
    ///     diags = append(diags, moreDiags…)  // … so this read is not the back edge's
    ///     flat, moreDiags := decode(pp)      // and this value is dead
    /// }
    /// ```
    ///
    /// Taking the first read in the body without asking what precedes it made
    /// every such value look live. That is hashicorp/packer's
    /// `hcl2template/types.packer_config.go:636`, which upstream reports and
    /// guff did not.
    ///
    /// "Redefines" means a redefinition that is guaranteed to run: a
    /// declaration or `:=` (key `0`), or a plain assignment sitting directly in
    /// the loop body's own statement list. One inside a nested `if` may not run,
    /// so it does not break the back edge — the same distinction
    /// [`Self::first_redef_after`] makes.
    fn read_in_enclosing_loop(&self, obj: ObjectId, pos: u32) -> bool {
        let Some(uses) = self.uses.get(&obj) else {
            return false;
        };
        self.loops
            .iter()
            .filter(|(start, end)| *start <= pos && pos <= *end)
            .any(|(start, end)| {
                let Some(&first_use) = uses[uses.partition_point(|&p| p < *start)..].first()
                else {
                    return false;
                };
                if first_use > *end {
                    return false;
                }
                let first_redef = self.defs.get(&obj).and_then(|defs| {
                    defs[defs.partition_point(|&(p, _)| p < *start)..]
                        .iter()
                        .find(|(p, key)| *p <= *end && (*key == 0 || key == start))
                        .map(|(p, _)| *p)
                });
                first_redef.is_none_or(|d| first_use < d)
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

    /// Every redefinition of `obj` strictly between `lo` and `hi`, with the key
    /// of the statement list it is a direct member of — branch ones included,
    /// which [`Self::first_redef_after`] deliberately leaves out.
    fn redefs_between(&self, obj: ObjectId, lo: u32, hi: u32) -> Vec<(u32, u32)> {
        let Some(v) = self.defs.get(&obj) else {
            return Vec::new();
        };
        v[v.partition_point(|&(p, _)| p <= lo)..]
            .iter()
            .take_while(|(p, _)| *p < hi)
            .copied()
            .collect()
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
    /// `until` bounds the question: a read past the `return` that follows the
    /// assignment in its own statement list is in code this value never
    /// reaches. Without the bound, `if a > 0 { x = a + 1; return 0 }; return x`
    /// looks live because of the second `return`.
    fn value_is_read_before_redef(
        &self,
        obj: ObjectId,
        after_pos: u32,
        block: Option<u32>,
        until: Option<u32>,
    ) -> bool {
        let use_after = self
            .first_use_after(obj, after_pos)
            .filter(|u| until.is_none_or(|stop| *u < stop));
        match (use_after, self.first_redef_after(obj, after_pos, block)) {
            (Some(u), Some(d)) => u < d,
            (Some(_), None) => true,
            _ => false,
        }
    }
}

/// Whether a statement list ends in something that leaves the function, so the
/// code after the `if` it belongs to is not reachable from it.
///
/// `return` and a call `ctrlflow` proved cannot return (`panic`, `t.Fatal`,
/// `log.Fatalf`, `os.Exit`, …) both qualify; `break`, `continue` and `goto`
/// stay in the function and do not.
fn stmt_list_leaves_function(
    ctrl: &ctrlflow::CtrlFlowResult,
    info: &guff_types::api::Info,
    list: &[Stmt],
) -> bool {
    match list.last() {
        Some(Stmt::ReturnStmt(_)) => true,
        Some(Stmt::ExprStmt(e)) => match unparen_expr(&e.x) {
            Expr::CallExpr(call) => ctrl.call_never_returns(info, call),
            _ => false,
        },
        Some(Stmt::BlockStmt(b)) => stmt_list_leaves_function(ctrl, info, &b.list),
        Some(Stmt::IfStmt(ifs)) => {
            // Every arm, and there has to be a final `else`.
            let mut cur = ifs;
            loop {
                if !stmt_list_leaves_function(ctrl, info, &cur.body.list) {
                    return false;
                }
                match cur.else_.as_deref() {
                    Some(Stmt::IfStmt(next)) => cur = next,
                    Some(Stmt::BlockStmt(b)) => {
                        return stmt_list_leaves_function(ctrl, info, &b.list)
                    }
                    _ => return false,
                }
            }
        }
        _ => false,
    }
}

/// Whether the redefinition sitting directly in the arm keyed `arm_key` runs on
/// every path that leaves its `if` statement.
///
/// `first_redef_after` only counts a redefinition in the assignment's own
/// statement list, because a branch may not run:
///
/// ```ignore
/// loadingRules := clientcmd.NewDefaultClientConfigLoadingRules()
/// if len(settings.KubeConfig) > 0 {
///     loadingRules = &clientcmd.ClientConfigLoadingRules{…}
/// }
/// // loadingRules read here — the first value is live on the other path
/// ```
///
/// But when every *other* arm of the chain leaves the function, the redefining
/// arm is the only way out and the first value is dead after all. beats writes
/// that twice:
///
/// ```ignore
/// config := map[string]interface{}{}
/// if !okAccessKeyID || accessKeyID == "" {
///     t.Fatal("$AWS_ACCESS_KEY_ID not set or set to empty")
/// } else if !okSecretAccessKey || secretAccessKey == "" {
///     t.Fatal("$AWS_SECRET_ACCESS_KEY not set or set to empty")
/// } else {
///     config = map[string]interface{}{…}
/// }
/// config["default_region"] = defaultRegion   // the read that looked like a use
/// ```
///
/// The chain has to be the *outermost* `if` covering the redefinition — an
/// `else if` is the `else` of the one above it, and judging the inner one alone
/// would forget the arms before it — and it has to sit in the same statement
/// list as the assignment, or reaching the read without running the `if` at all
/// would be possible.
fn branch_redef_is_unconditional(
    pass: &Pass<'_>,
    ctrl: &ctrlflow::CtrlFlowResult,
    idents: &IdentIndex,
    assign_block: Option<u32>,
    redef_pos: u32,
    arm_key: u32,
) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let mut answer = false;
    for file in pass.files() {
        let mut judged = false;
        guff::walk::preorder_prune(NodeRef::File(file), |n| {
            if judged {
                return false;
            }
            let NodeRef::IfStmt(ifs) = n else {
                return true;
            };
            let start = ifs.if_.0 as u32;
            // The end of the whole chain: the last arm's closing brace.
            let mut end = ifs.body.rbrace.0 as u32;
            {
                let mut cur = ifs;
                loop {
                    match cur.else_.as_deref() {
                        Some(Stmt::IfStmt(next)) => {
                            end = next.body.rbrace.0 as u32;
                            cur = next;
                        }
                        Some(Stmt::BlockStmt(b)) => {
                            end = b.rbrace.0 as u32;
                            break;
                        }
                        _ => break,
                    }
                }
            }
            if redef_pos < start || redef_pos > end {
                return true;
            }
            // The outermost `if` covering it is the one that decides; whatever
            // it says, stop looking.
            judged = true;
            if idents.if_lists.get(&start).copied() != assign_block {
                return false;
            }
            let mut arms: Vec<&[Stmt]> = Vec::new();
            let mut keys: Vec<u32> = Vec::new();
            let mut cur = ifs;
            loop {
                arms.push(&cur.body.list);
                keys.push(cur.body.lbrace.0 as u32);
                match cur.else_.as_deref() {
                    Some(Stmt::IfStmt(next)) => cur = next,
                    Some(Stmt::BlockStmt(b)) => {
                        arms.push(&b.list);
                        keys.push(b.lbrace.0 as u32);
                        break;
                    }
                    // No final `else`: the implicit fall-through arm runs no
                    // redefinition and never leaves the function.
                    _ => return false,
                }
            }
            let Some(mine) = keys.iter().position(|k| *k == arm_key) else {
                return false;
            };
            answer = arms
                .iter()
                .enumerate()
                .all(|(i, list)| i == mine || stmt_list_leaves_function(ctrl, info, list));
            false
        });
        if judged {
            break;
        }
    }
    answer
}

fn ssa_unused_but_ast_read(
    pass: &Pass<'_>,
    ctrl: Option<&ctrlflow::CtrlFlowResult>,
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
    // …but only while the loop can come back round. A `return` after the
    // assignment in its own statement list ends the iteration *and* the
    // function, so a read that sits above the assignment is not reachable from
    // it — see `IdentIndex::returns_after`. The reads *below* it are still
    // reads, which is why this guards only the loop arm: `x := f(); _ = x; …;
    // return nil` has a live value however many returns follow.
    let returns_at = idents.returns_after.get(&assign_pos).copied();
    if returns_at.is_none() && idents.read_in_enclosing_loop(obj, assign_pos) {
        return true;
    }
    if !idents.value_is_read_before_redef(obj, assign_pos, block, returns_at) {
        return false;
    }
    // It said "read". A branch redefinition between the assignment and that
    // read still overwrites the value when every other arm of its `if` leaves
    // the function — see `branch_redef_is_unconditional`.
    let Some(ctrl) = ctrl else {
        return true;
    };
    let Some(read_at) = idents
        .first_use_after(obj, assign_pos)
        .filter(|u| returns_at.is_none_or(|stop| *u < stop))
    else {
        return true;
    };
    !idents
        .redefs_between(obj, assign_pos, read_at)
        .into_iter()
        .any(|(pos, key)| {
            key != 0
                && branch_redef_is_unconditional(pass, ctrl, idents, block, pos, key)
        })
}

/// Whether the instruction that produced `v` is still in the function.
///
/// A call to a function `ctrlflow` proved cannot return is followed by a
/// `Panic`, and everything after it becomes an unreachable block that
/// `blockopt::delete_unreachable_blocks` removes — upstream's IR does the same
/// (`go/ir/emit.go`'s `fn.Prog.noReturn(callee.object)` arm). The check walks
/// the *AST*, though, so it still visits assignments whose instructions are
/// gone, and then reads "no referrers" off a value nothing can refer to any
/// more.
///
/// VictoriaMetrics `lib/fs/reader_at.go:312` is that shape: the CAS loop sits
/// behind `if !mincore(…)`, and on every non-linux build `mincore` is
/// `panic("BUG: unexpected call")`. Replace the panic with a `return` in a
/// repro and the finding disappears from guff as well.
///
/// A register covers most of it, but not all: a func literal with no free
/// variables is a `Value::Function` **constant**, so it has no instruction to
/// ask about and the guard passed it straight through. What the IR deleted
/// there is the *store*, so for a non-register value the question has to be
/// asked about the assignment's own position range. grafana/loki's
/// `pkg/querytee/proxy_endpoint_test.go:534` assigns a handler to a captured
/// variable after `t.Skip`, and `t.Skip` does not return. Measured: `t.Fatal`
/// behaves the same and a plain `return` does not, which is what says the
/// trigger is a no-return call and not "unreachable code" in general.
fn value_is_live(
    func: &guff_ssa::function::Function,
    v: Value,
    assign_range: (u32, u32),
) -> bool {
    if let Value::Instr(iid) = v {
        return func.live_blocks().any(|(_, b)| b.instrs.contains(&iid));
    }
    let (lo, hi) = assign_range;
    func.live_blocks().any(|(_, b)| {
        b.instrs.iter().any(|&iid| {
            let p = func.pos(iid).0 as u32;
            p >= lo && p <= hi
        })
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4006 requires inspect analyzer".to_string())?
        .clone();

    let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) else {
        return Ok(None);
    };

    // Methods included. `expr_values()` follows `buildir_src_methods`, which is
    // off outside contextcheck runs to keep SA5011 from over-reporting — and an
    // expression in a method body then resolves to nothing, so SA4006 never
    // fired inside a method at all. See `BuildIrResult::expr_values_with_methods`.
    let exprs = ir.expr_values_with_methods();
    let ctrl = pass.result_of::<ctrlflow::CtrlFlowResult>(ctrlflow::analyzer());
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
                                if !value_is_live(func, Value::Instr(rid), (0, 0)) {
                                    continue;
                                }
                                if !has_use(func, Value::Instr(rid)) {
                                    if ssa_unused_but_ast_read(
                                        pass,
                                        ctrl.as_deref(),
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
                    if !value_is_live(func, v, (assign_pos(assign), assign_end(assign))) {
                        continue;
                    }
                    if !has_use(func, v) {
                        if ssa_unused_but_ast_read(
                            pass,
                            ctrl.as_deref(),
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
        requires: vec![
            inspect::analyzer(),
            buildir::analyzer(),
            // `call_never_returns` — which `if` arm cannot reach the code below it.
            ctrlflow::analyzer(),
        ],
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
