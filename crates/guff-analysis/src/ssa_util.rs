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

/// Reports whether `common` is a static call to `name` (e.g. `"time.Tick"`).
pub fn is_call_to(ctx_prog: &Program, common: &CallCommon, name: &str) -> bool {
    let Some(target) = callcheck::resolve_call_target(common, ctx_prog) else {
        return false;
    };
    let resolved = code::type_func_name(
        &ctx_prog.type_arena,
        &ctx_prog.object_arena,
        &ctx_prog.package_arena,
        target,
    );
    resolved == name
}

/// Reports whether `common` is a static call to any of `names`.
pub fn is_call_to_any(ctx_prog: &Program, common: &CallCommon, names: &[&str]) -> bool {
    names.iter().any(|n| is_call_to(ctx_prog, common, n))
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

/// Reports whether `fn` has at least one path that returns normally.
///
/// Port of `irutil.Terminates` (simplified; omits time.Tick recv edge case).
pub fn terminates(func: &Function, prog: &Program) -> bool {
    if func.blocks.is_empty() {
        return true;
    }
    for (_, block) in func.blocks.iter() {
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

/// Invokes `f` for every static call site in `func`.
pub fn each_call<F>(func: &Function, prog: &Program, mut f: F)
where
    F: FnMut(BlockId, &Function, InstrId, &CallCommon, Option<FuncId>),
{
    for (bid, block) in func.blocks.iter() {
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
    func.blocks
        .iter()
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

/// Reports whether `block` is inside a natural loop (back-edge from a predecessor).
pub fn is_in_loop(func: &Function, block: BlockId) -> bool {
    let b = func.blocks.get(block);
    b.preds
        .iter()
        .any(|&pred| func.blocks.get(pred).dominates(b))
}
