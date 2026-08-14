//! SSA/IR helpers for staticcheck rules.
//!
//! Port of `honnef.co/go/tools/go/ir/irutil`.

use guff::token::Token;
use guff_ssa::arena::ArenaId;
use guff_ssa::block::BasicBlock;
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, FuncId, InstrId, ParamId};
use guff_ssa::instr::{
    Call, CallCommon, If, InstrData, Store,
};
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::arena::ObjectId;

use crate::callcheck;
use crate::code;

/// Returns the control-transfer instruction at the end of `block`, if any.
pub fn block_control<'a>(func: &'a Function, block: &BasicBlock) -> Option<&'a InstrData> {
    for &iid in block.instrs.iter().rev() {
        let instr = func.instrs.get(iid);
        if matches!(
            instr,
            InstrData::Return(_)
                | InstrData::Jump(_)
                | InstrData::If(_)
                | InstrData::Panic(_)
        ) {
            return Some(instr);
        }
    }
    None
}

/// Resolves the static callee name of `common` (e.g. `"time.Tick"`), if any.
fn resolved_call_name(ctx_prog: &Program, common: &CallCommon) -> Option<String> {
    let target = callcheck::resolve_call_target(common, ctx_prog)?;
    Some(code::type_func_name(
        &ctx_prog.type_arena,
        &ctx_prog.object_arena,
        &ctx_prog.package_arena,
        target,
    ))
}

/// Reports whether `common` is a static call to `name` (e.g. `"time.Tick"`).
pub fn is_call_to(ctx_prog: &Program, common: &CallCommon, name: &str) -> bool {
    resolved_call_name(ctx_prog, common).is_some_and(|resolved| resolved == name)
}

/// Reports whether `common` is a static call to any of `names`.
///
/// Resolves the callee name once (unlike calling [`is_call_to`] in a loop).
pub fn is_call_to_any(ctx_prog: &Program, common: &CallCommon, names: &[&str]) -> bool {
    resolved_call_name(ctx_prog, common).is_some_and(|resolved| names.iter().any(|n| *n == resolved))
}

/// Returns referrers of `value` in `func`, or an empty slice.
pub fn referrers<'a>(func: &'a Function, value: Value) -> &'a [InstrId] {
    func.referrers
        .as_ref()
        .and_then(|r| r.get(&value))
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

/// Skips `DebugRef` instructions.
pub fn filter_debug(instrs: &[InstrId], func: &Function) -> Vec<InstrId> {
    instrs
        .iter()
        .copied()
        .filter(|&iid| !matches!(func.instrs.get(iid), InstrData::DebugRef(_)))
        .collect()
}

/// [`filter_debug`] without the `Vec`.
///
/// Most callers only walk the result once, and the allocation showed up as its
/// own line in the profile (PERF_TASKS_V3 V1-9). Use [`filter_debug`] when the
/// result needs indexing or a length — SA2003 looks at adjacent pairs.
pub fn iter_non_debug<'a>(
    instrs: &'a [InstrId],
    func: &'a Function,
) -> impl Iterator<Item = InstrId> + 'a {
    instrs
        .iter()
        .copied()
        .filter(move |&iid| !matches!(func.instrs.get(iid), InstrData::DebugRef(_)))
}

/// True if any non-`DebugRef` referrer exists (avoids allocating for the common
/// "has any real use?" check in SA4017 / SA4010 / …).
pub fn has_non_debug_referrer(instrs: &[InstrId], func: &Function) -> bool {
    instrs
        .iter()
        .copied()
        .any(|iid| !matches!(func.instrs.get(iid), InstrData::DebugRef(_)))
}

/// Reports whether `fn` has at least one path that returns normally.
///
/// Port of `irutil.Terminates` (simplified; omits time.Tick recv edge case).
pub fn terminates(func: &Function, prog: &Program) -> bool {
    if func.blocks.is_empty() {
        return true;
    }
    for (_, block) in func.live_blocks() {
        let Some(InstrData::Return(_)) = block_control(func, block) else {
            continue;
        };
        if block.preds.is_empty() {
            return true;
        }
        for &pred_id in &block.preds {
            let pred = func.blocks.get(pred_id);
            match block_control(func, pred) {
                Some(InstrData::Panic(_)) => {}
                Some(InstrData::If(iff)) => {
                    if !is_time_tick_recv_cond(func, prog, iff) {
                        return true;
                    }
                }
                _ => return true,
            }
        }
    }
    false
}

fn is_time_tick_recv_cond(func: &Function, prog: &Program, iff: &If) -> bool {
    let Value::Instr(ex_id) = iff.cond else {
        return false;
    };
    let InstrData::Extract(ex) = func.instrs.get(ex_id) else {
        return false;
    };
    if ex.index != 1 {
        return false;
    }
    let Value::Instr(recv_id) = ex.tuple else {
        return false;
    };
    // Channel receive is modeled as UnOp ARROW in guff-ssa.
    let InstrData::UnOp(unop) = func.instrs.get(recv_id) else {
        return false;
    };
    if unop.op != Token::ARROW {
        return false;
    }
    let Value::Instr(call_id) = unop.x else {
        return false;
    };
    let InstrData::Call(Call { call, .. }) = func.instrs.get(call_id) else {
        return false;
    };
    is_call_to(prog, call, "time.Tick")
}

/// Walks blocks dominated by `start` until `stop` is reached, calling `f` for each.
pub fn walk_dominated<F>(func: &Function, start: BlockId, stop: BlockId, mut f: F)
where
    F: FnMut(BlockId, &BasicBlock) -> bool,
{
    let start_block = func.blocks.get(start);
    let mut stack = vec![start];
    let mut seen = std::collections::HashSet::new();
    while let Some(bid) = stack.pop() {
        if !seen.insert(bid) || bid == stop {
            continue;
        }
        let block = func.blocks.get(bid);
        if !start_block.dominates(block) {
            continue;
        }
        if !f(bid, block) {
            return;
        }
        for &succ in &block.succs {
            stack.push(succ);
        }
    }
}

/// If `store.addr` is an `IndexAddr` into `param`, returns true.
pub fn store_modifies_param(func: &Function, store: &Store, param: Value) -> bool {
    let Value::Instr(addr_id) = store.addr else {
        return false;
    };
    let InstrData::IndexAddr(ia) = func.instrs.get(addr_id) else {
        return false;
    };
    ia.x == param
}

/// If `call` is `append(param, ...)`, returns true.
pub fn append_modifies_param(func: &Function, call: &Call, param: Value) -> bool {
    let Value::Builtin(_) = call.call.value else {
        return false;
    };
    call.call
        .args
        .first()
        .is_some_and(|first| *first == param)
}

/// Returns the `n`th parameter as a [`Value`], if present.
pub fn param_value(func: &Function, index: usize) -> Option<Value> {
    let mut params: Vec<ParamId> = func.params.iter().map(|(id, _)| id).collect();
    params.sort_by_key(|id| id.index());
    params.get(index).map(|&pid| Value::Param(pid))
}

/// Returns the short name of a static callee (`"Lock"`, `"append"`, …).
pub fn short_call_name(prog: &Program, common: &CallCommon) -> Option<String> {
    if common.method.is_some() {
        return common.method.map(|o| o.name(&prog.object_arena).to_string());
    }
    match common.value {
        Value::Builtin(b) => Some(prog.builtins.get(b).name.clone()),
        Value::Function(fid) => {
            let obj = prog.functions.get(fid).object?;
            Some(obj.name(&prog.object_arena).to_string())
        }
        _ => None,
    }
}

/// Resolves a closure target within `caller`.
pub fn closure_fn_in(caller: &Function, value: Value) -> Option<FuncId> {
    match value {
        Value::Function(fid) => Some(fid),
        Value::Instr(iid) => {
            let InstrData::MakeClosure(mc) = caller.instrs.get(iid) else {
                return None;
            };
            Some(mc.fn_)
        }
        _ => None,
    }
}

/// Reports whether `value` is a nil SSA constant.
pub fn is_nil_const(prog: &Program, caller: &Function, value: Value) -> bool {
    let v = callcheck::flatten_ssa_value(caller, value);
    let Value::Const(id) = v else {
        return false;
    };
    prog.constants.get(id).val.is_none()
}

/// Returns the object for a static call target.
pub fn call_object(prog: &Program, common: &CallCommon) -> Option<ObjectId> {
    callcheck::resolve_call_target(common, prog)
}

/// Maps the position guff-ssa stamps on an instruction to the start of the AST
/// node honnef's IR records as that instruction's `Source()`.
///
/// guff-ssa follows go/ssa, where an instruction's position is an inner token:
/// `Call.Pos()` is the `Lparen`, a map update's is the `[`, a `BinOp`'s is the
/// operator, and a `TypeAssert`'s is the `(` of `.(T)`. honnef's IR — which
/// every staticcheck port models — instead keeps the AST node on the
/// instruction and defines `Pos()` as `Source().Pos()`, so findings land on the
/// start of the callee expression or of the indexed operand. A check that
/// reports `func.pos(iid)` therefore sits one token to the right of upstream
/// unless it goes through this map.
///
/// `defer` / `go` need no entry: there honnef's source node is the *statement*,
/// and guff already stamps the keyword.
pub fn call_node_starts(pass: &crate::pass::Pass<'_>) -> std::collections::HashMap<u32, u32> {
    use guff::walk::{preorder, NodeRef};

    let mut starts = std::collections::HashMap::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::CallExpr(c) => {
                    starts.insert(c.lparen.0 as u32, c.pos().0 as u32);
                }
                NodeRef::IndexExpr(ix) => {
                    starts.insert(ix.lbrack.0 as u32, ix.x.pos().0 as u32);
                }
                // Go: `BinaryExpr.Pos()` and `TypeAssertExpr.Pos()` are both
                // `X.Pos()`.
                NodeRef::BinaryExpr(b) => {
                    starts.insert(b.op_pos.0 as u32, b.x.pos().0 as u32);
                }
                NodeRef::TypeAssertExpr(ta) => {
                    starts.insert(ta.lparen.0 as u32, ta.x.pos().0 as u32);
                }
                _ => {}
            }
            true
        });
    }
    starts
}

/// Invokes `f` for every static call site in `func`.
pub fn each_call<F>(func: &Function, prog: &Program, mut f: F)
where
    F: FnMut(BlockId, &Function, InstrId, &CallCommon, Option<FuncId>),
{
    for (bid, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let (common, callee) = match func.instrs.get(iid) {
                InstrData::Call(Call { call, .. }) => (call, callcheck::static_callee(call)),
                InstrData::Defer(d) => (&d.call, callcheck::static_callee(&d.call)),
                InstrData::Go(g) => (&g.call, callcheck::static_callee(&g.call)),
                _ => continue,
            };
            if callcheck::resolve_call_target(common, prog).is_some() {
                f(bid, func, iid, common, callee);
            }
        }
    }
}

/// Blocks that end in `return`.
pub fn return_blocks(func: &Function) -> Vec<BlockId> {
    func.live_blocks()
        .filter_map(|(bid, block)| {
            matches!(block_control(func, block), Some(InstrData::Return(_))).then_some(bid)
        })
        .collect()
}

/// Reports whether `from` dominates every return block in `func`.
pub fn dominates_all_returns(func: &Function, from: BlockId) -> bool {
    let from_block = func.blocks.get(from);
    return_blocks(func)
        .iter()
        .all(|&ret| from_block.dominates(func.blocks.get(ret)))
}

/// Reports whether `block` is inside a natural loop.
///
/// Port of `honnef.co/go/tools/staticcheck/sa6000.isInLoop` +
/// `irutil.FindLoops`: a block is in a loop iff it belongs to the natural
/// loop of some header `h` that has a back-edge from a block `n` it
/// dominates (`h` dominates `n`, edge `n → h`).
pub fn is_in_loop(func: &Function, block: BlockId) -> bool {
    for (h_id, h) in func.live_blocks() {
        // Clone preds so we can re-borrow `func.blocks` while iterating.
        let preds = h.preds.clone();
        for n_id in preds {
            let n = func.blocks.get(n_id);
            if !h.dominates(n) {
                continue;
            }
            // `n → h` is a back-edge; `h` is the loop header.
            if h_id == block || n_id == block {
                return true;
            }
            if n_id == h_id {
                // Self-loop: members are just `{h}`.
                continue;
            }
            if natural_loop_contains(func, n_id, h_id, block) {
                return true;
            }
        }
    }
    false
}

/// True if `target` is among `allPredsBut(start, header)` — the natural-loop
/// body collected by walking predecessors of `start` without crossing `header`.
fn natural_loop_contains(
    func: &Function,
    start: BlockId,
    header: BlockId,
    target: BlockId,
) -> bool {
    let mut stack = vec![start];
    let mut seen = std::collections::HashSet::from([start, header]);
    while let Some(b) = stack.pop() {
        for &pred in &func.blocks.get(b).preds {
            if pred == header || !seen.insert(pred) {
                continue;
            }
            if pred == target {
                return true;
            }
            stack.push(pred);
        }
    }
    false
}
