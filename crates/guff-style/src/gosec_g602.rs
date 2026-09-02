//! Gosec **G602** — slice index / bounds out of range (SSA).
//!
//! Port of securego/gosec v2.26.1 `analyzers/slice_bounds.go` (+
//! `GetSliceBounds` / `ComputeSliceNewCap` from `util.go`), including the
//! `ifs` map collected via `extractSliceIfLenCondition` and the post-pass
//! that deletes issues in if-successor blocks.
//!
//! guff lowers `make([]T, n)` as [`MakeSlice`] (go/ssa often uses Alloc of
//! `[n]T` + Slice); the MakeSlice entry covers isolate fixtures.
//!
//! # Where guff's SSA does not spell the slice the way upstream reads it
//!
//! Upstream never asks "how big is this slice"; it asks "which `Alloc` of an
//! array does this slice come from", and every capacity it knows is read off
//! that array's type. Two of guff's lowerings put no such array in the program,
//! and each cost the analyzer a whole family of findings — measured against
//! golangci-lint 2.12.2 on twenty-four shapes, sixteen of which upstream
//! reports:
//!
//! - **`make([]T, constN)` passed to a function.** go/ssa lowers a constant
//!   `make` to `Alloc *[N]T` + `Slice`; guff emits a single [`MakeSlice`]. The
//!   entry point below already knows that, but [`track_slice_bounds`]'s
//!   `Call → parameter` step was a literal transcription of upstream's
//!   `arg.(*ssa.Slice)` test, so the walk stopped at the call.
//! - **a variadic call.** go/ssa packs `f(a, b)` into a fresh `Alloc *[2]any` +
//!   `Slice`, which is where upstream's capacity for `f`'s `args` parameter
//!   comes from. guff deliberately does not build that slice — it passes the
//!   tail through individually and records the spread in
//!   [`CallCommon::ellipsis`](guff_ssa::instr::CallCommon::ellipsis) — so there
//!   was no array, no entry point, and G602 never looked inside a variadic
//!   callee at all. [`variadic_call_cap`] synthesises exactly the capacity that
//!   array would have had.
//!
//! Two shapes say where that synthesis stops, and both were measured:
//! `f()` with an empty tail is silent upstream (go/ssa passes the `nil` slice
//! constant, not an `Alloc`), and a call through a function *value* is silent
//! too (upstream needs `Call.Value.(*ssa.Function)`).
//!
//! The SSA program and the `SrcFuncs` list come from [`crate::gosec_ssa`], which
//! builds them once for every SSA-based gosec analyzer.

use std::collections::HashMap;

use guff::token::Token;
use guff_analysis::callcheck::{extract_const_int, static_callee, SsaValue};
use guff_analysis::referrers;
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, FuncId, InstrId, ParamId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::signature::signature_variadic;
use guff_types::TypeId;

const MAX_DEPTH: u32 = 20;
const MSG_INDEX: &str = "G602: slice index out of range";
const MSG_BOUNDS: &str = "G602: slice bounds out of range";

/// Tracked SSA node: instruction or parameter within a function.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Node {
    Instr(FuncId, InstrId),
    Param(FuncId, guff_ssa::ids::ParamId),
}

impl Node {
    fn func(self) -> FuncId {
        match self {
            Node::Instr(f, _) | Node::Param(f, _) => f,
        }
    }

    fn value(self) -> Value {
        match self {
            Node::Instr(_, i) => Value::Instr(i),
            Node::Param(_, p) => Value::Param(p),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    node: Node,
    slice_cap: i32,
}

struct TrackCacheValue {
    violations: Vec<(FuncId, InstrId, bool)>,
    /// `(func, if_instr) → binop_instr` (gosec `map[*ssa.If]*ssa.BinOp`).
    ifs: HashMap<(FuncId, InstrId), InstrId>,
}

/// Collects G602 out of the SSA build [`crate::gosec_ssa`] shares between the
/// gosec analyzers, appending `(pos, message)` into `pending`.
pub(crate) fn collect_g602(
    prog: &Program,
    src_funcs: &[FuncId],
    pending: &mut Vec<(u32, u32, String)>,
) {
    let mut reports: HashMap<(FuncId, InstrId), &'static str> = HashMap::new();
    collect_reports(prog, src_funcs, &mut reports);

    for ((fid, iid), msg) in reports {
        let func = prog.functions.get(fid);
        let pos = func.pos(iid);
        if pos.is_valid() {
            pending.push((pos.0 as u32, pos.0 as u32, msg.to_string()));
        }
    }
}

fn collect_reports(
    prog: &Program,
    src_funcs: &[FuncId],
    reports: &mut HashMap<(FuncId, InstrId), &'static str>,
) {
    // Package-wide like gosec's single `ifs` map for the run.
    let mut ifs: HashMap<(FuncId, InstrId), InstrId> = HashMap::new();

    for &fid in src_funcs {
        let func = prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                match func.instrs.get(iid) {
                    InstrData::Alloc(_) => {
                        let ty = value_type_of(prog, func, Value::Instr(iid));
                        let Some(slice_cap) = extract_array_len(prog, ty) else {
                            continue;
                        };
                        for &ref_id in referrers(func, Value::Instr(iid)) {
                            let InstrData::Slice(_) = func.instrs.get(ref_id) else {
                                continue;
                            };
                            let (l, h, max_idx) = get_slice_bounds(prog, func, ref_id);
                            let mut violations: Vec<(FuncId, InstrId, bool)> = Vec::new();
                            if max_idx > 0 {
                                if !is_three_index_slice_inside_bounds(l, h, max_idx, slice_cap) {
                                    violations.push((fid, ref_id, true));
                                }
                            } else if !is_slice_inside_bounds(0, slice_cap, l, h) {
                                violations.push((fid, ref_id, true));
                            }
                            let new_cap = compute_slice_new_cap(l, h, max_idx, slice_cap);
                            track_slice_bounds(
                                prog,
                                0,
                                new_cap,
                                Node::Instr(fid, ref_id),
                                &mut violations,
                                &mut ifs,
                                &mut HashMap::new(),
                            );
                            for (vf, vi, is_bounds) in violations {
                                if is_bounds {
                                    reports.insert((vf, vi), MSG_BOUNDS);
                                    continue;
                                }
                                // Skip IndexAddr that indexes the original Alloc directly.
                                if let InstrData::IndexAddr(ia) = prog.functions.get(vf).instrs.get(vi)
                                {
                                    if ia.x == Value::Instr(iid) && vf == fid {
                                        continue;
                                    }
                                }
                                reports.insert((vf, vi), MSG_INDEX);
                            }
                        }
                    }
                    InstrData::MakeSlice(ms) => {
                        let Some(len_v) = ms.len else {
                            continue;
                        };
                        let Some(slice_cap) =
                            extract_const_int(prog, func, SsaValue::new(len_v)).map(|n| n as i32)
                        else {
                            continue;
                        };
                        let mut violations: Vec<(FuncId, InstrId, bool)> = Vec::new();
                        track_slice_bounds(
                            prog,
                            0,
                            slice_cap,
                            Node::Instr(fid, iid),
                            &mut violations,
                            &mut ifs,
                            &mut HashMap::new(),
                        );
                        for (vf, vi, is_bounds) in violations {
                            reports.insert((vf, vi), if is_bounds { MSG_BOUNDS } else { MSG_INDEX });
                        }
                    }
                    // The variadic tail go/ssa would have packed into an
                    // array. Upstream's entry point is that array's `Alloc`;
                    // guff never builds it, so the capacity is read off the
                    // call site instead and the callee's parameter is entered
                    // directly. See [`variadic_call_cap`].
                    InstrData::Call(c) => {
                        let Some((callee, pid, slice_cap)) = variadic_call_cap(prog, &c.call)
                        else {
                            continue;
                        };
                        let mut violations: Vec<(FuncId, InstrId, bool)> = Vec::new();
                        track_slice_bounds(
                            prog,
                            0,
                            slice_cap,
                            Node::Param(callee, pid),
                            &mut violations,
                            &mut ifs,
                            &mut HashMap::new(),
                        );
                        for (vf, vi, is_bounds) in violations {
                            reports.insert((vf, vi), if is_bounds { MSG_BOUNDS } else { MSG_INDEX });
                        }
                    }
                    InstrData::IndexAddr(ia) => {
                        // Nil slice constant index (go/ssa Const nil).
                        if let Value::Const(cid) = ia.x {
                            let c = prog.constants.get(cid);
                            if c.val.is_none() {
                                let ty = value_type_of(prog, func, ia.x);
                                let u = ty.underlying(&prog.type_arena);
                                if matches!(prog.type_arena.get(u), TypeData::Slice(_)) {
                                    reports.insert((fid, iid), MSG_INDEX);
                                }
                            }
                        }
                        // Direct index into Alloc of fixed array.
                        if let Value::Instr(xid) = ia.x {
                            if matches!(func.instrs.get(xid), InstrData::Alloc(_)) {
                                let ty = value_type_of(prog, func, Value::Instr(xid));
                                if let Some(array_len) = extract_array_len(prog, ty) {
                                    if let Some(idx) =
                                        extract_int_value_index_addr(prog, fid, iid, array_len)
                                    {
                                        if !is_slice_index_inside_bounds(array_len, idx) {
                                            reports.insert((fid, iid), MSG_INDEX);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    suppress_via_ifs(prog, &ifs, reports);
}

fn track_slice_bounds(
    prog: &Program,
    depth: u32,
    slice_cap: i32,
    node: Node,
    violations: &mut Vec<(FuncId, InstrId, bool)>,
    ifs: &mut HashMap<(FuncId, InstrId), InstrId>,
    cache: &mut HashMap<CacheKey, Option<TrackCacheValue>>,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let key = CacheKey { node, slice_cap };
    if let Some(cached) = cache.get(&key) {
        if let Some(v) = cached {
            violations.extend_from_slice(&v.violations);
            for (k, b) in &v.ifs {
                ifs.insert(*k, *b);
            }
        }
        return;
    }
    cache.insert(key, None); // visiting

    let mut local = TrackCacheValue {
        violations: Vec::new(),
        ifs: HashMap::new(),
    };
    let fid = node.func();
    let func = prog.functions.get(fid);
    let val = node.value();

    for &ref_id in referrers(func, val) {
        match func.instrs.get(ref_id) {
            InstrData::Slice(_) => {
                check_all_slices_bounds(
                    prog,
                    depth + 1,
                    slice_cap,
                    fid,
                    ref_id,
                    &mut local.violations,
                    &mut local.ifs,
                    cache,
                );
                // Upstream recurses for `Alloc | Parameter | Slice` and omits
                // MakeSlice — but that omission is unreachable there, not a
                // decision: go/ssa lowers `make([]T, constN)` to
                // `Alloc *[N]T` + `Slice`, so upstream enters at the Alloc and
                // a re-slice's X is always the *previous Slice*. guff lowers
                // the same source to a single MakeSlice, so a re-slice's X is
                // the MakeSlice, and copying the switch literally stopped the
                // walk one step in: `s := make([]byte, 10); s = s[:2]; s[4]`
                // reported nothing. MakeSlice stands where upstream's
                // Alloc/Slice pair stands, so it recurses with them.
                match slice_x_kind(prog, func, ref_id) {
                    SliceXKind::Alloc
                    | SliceXKind::Param
                    | SliceXKind::Slice
                    | SliceXKind::MakeSlice => {
                        let (l, h, max_idx) = get_slice_bounds(prog, func, ref_id);
                        let new_cap = compute_slice_new_cap(l, h, max_idx, slice_cap);
                        track_slice_bounds(
                            prog,
                            depth + 1,
                            new_cap,
                            Node::Instr(fid, ref_id),
                            &mut local.violations,
                            &mut local.ifs,
                            cache,
                        );
                    }
                    SliceXKind::Other => {}
                }
            }
            InstrData::IndexAddr(ia) => {
                if let Some(idx) = extract_const_int(prog, func, SsaValue::new(ia.index)).map(|n| n as i32)
                {
                    if !is_slice_index_inside_bounds(slice_cap, idx) {
                        local.violations.push((fid, ref_id, false));
                    }
                }
                if let Some(idx) = extract_int_value_index_addr(prog, fid, ref_id, slice_cap) {
                    if !is_slice_index_inside_bounds(slice_cap, idx) {
                        local.violations.push((fid, ref_id, false));
                    }
                }
            }
            InstrData::Call(c) => {
                // Always try len→If collection (including when tracking a Param).
                if let Some((if_id, binop_id)) = extract_slice_if_len_condition(prog, func, ref_id) {
                    local.ifs.insert((fid, if_id), binop_id);
                    continue;
                }
                // Call→Param only when the tracked node is the slice *value*
                // itself. Upstream writes that as `arg.(*ssa.Slice)`, which in
                // go/ssa covers `make([]T, constN)` as well, because that is
                // lowered to `Alloc` + `Slice`. guff emits a single MakeSlice
                // for it, so MakeSlice stands in the same place and has to be
                // accepted here too — without it `f(make([]any, 2))` never
                // reached `f`'s body.
                //
                // A *parameter* is still excluded, and that is not an
                // oversight: upstream stops there as well, so a variadic tail
                // forwarded on with `pairs...` is silent in both tools.
                let Node::Instr(_, tracked_iid) = node else {
                    continue;
                };
                if !matches!(
                    func.instrs.get(tracked_iid),
                    InstrData::Slice(_) | InstrData::MakeSlice(_)
                ) {
                    continue;
                }
                let mut par_pos: Option<usize> = None;
                for (pos, &arg) in c.call.args.iter().enumerate() {
                    if arg == val {
                        par_pos = Some(pos);
                        break;
                    }
                }
                let Some(pos) = par_pos else {
                    continue;
                };
                let Some(callee) = resolve_static_func(&c.call) else {
                    continue;
                };
                let callee_fn = prog.functions.get(callee);
                let params: Vec<_> = callee_fn.params.iter().map(|(pid, _)| pid).collect();
                if let Some(&pid) = params.get(pos) {
                    track_slice_bounds(
                        prog,
                        depth + 1,
                        slice_cap,
                        Node::Param(callee, pid),
                        &mut local.violations,
                        &mut local.ifs,
                        cache,
                    );
                }
            }
            _ => {}
        }
    }

    violations.extend_from_slice(&local.violations);
    for (k, b) in &local.ifs {
        ifs.insert(*k, *b);
    }
    cache.insert(key, Some(local));
}

fn check_all_slices_bounds(
    prog: &Program,
    depth: u32,
    slice_cap: i32,
    fid: FuncId,
    slice_id: InstrId,
    violations: &mut Vec<(FuncId, InstrId, bool)>,
    ifs: &mut HashMap<(FuncId, InstrId), InstrId>,
    cache: &mut HashMap<CacheKey, Option<TrackCacheValue>>,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let func = prog.functions.get(fid);
    let (low, high, max_idx) = get_slice_bounds(prog, func, slice_id);
    if max_idx > 0 {
        if !is_three_index_slice_inside_bounds(low, high, max_idx, slice_cap) {
            violations.push((fid, slice_id, true));
        }
    } else if !is_slice_inside_bounds(0, slice_cap, low, high) {
        violations.push((fid, slice_id, true));
    }
    match slice_x_kind(prog, func, slice_id) {
        SliceXKind::Alloc | SliceXKind::Param | SliceXKind::Slice => {
            let new_cap = compute_slice_new_cap(low, high, max_idx, slice_cap);
            track_slice_bounds(
                prog,
                depth + 1,
                new_cap,
                Node::Instr(fid, slice_id),
                violations,
                ifs,
                cache,
            );
        }
        SliceXKind::MakeSlice | SliceXKind::Other => {}
    }
}

/// gosec `extractSliceIfLenCondition`: Call is builtin `len`, walk referrers
/// for a BinOp whose referrer is an If.
fn extract_slice_if_len_condition(
    prog: &Program,
    func: &Function,
    call_id: InstrId,
) -> Option<(InstrId, InstrId)> {
    let InstrData::Call(c) = func.instrs.get(call_id) else {
        return None;
    };
    let Value::Builtin(bid) = c.call.value else {
        return None;
    };
    if prog.builtins.get(bid).name != "len" {
        return None;
    }

    // Upstream returns the *first* `if` it reaches, and only that one is
    // recorded — so which comparison against `len(s)` is found decides whether
    // the whole family of issues in the branch is deleted. go/ssa's referrer
    // list is in the order the referrers were built, which follows the source;
    // guff's is not, and on
    //
    //     for i := 0; i < p; i += 2 {
    //         if i+1 < p { _ = pairs[i+1] }
    //     }
    //
    // it offered `i+1 < p` first. That comparison has `lenOffset == -1`, which
    // makes the deletion rule `lenOffset+idxOffset-1 < 0` fire and the finding
    // disappear; upstream sees `i < p` (`lenOffset == 0`), keeps the finding,
    // and reports it. So the referrers are put back into build order here.
    let mut refs: Vec<InstrId> = in_build_order(func, referrers(func, Value::Instr(call_id)));
    let mut depth = 0u32;
    while !refs.is_empty() && depth < MAX_DEPTH {
        let mut newrefs = Vec::new();
        for rid in refs {
            if matches!(func.instrs.get(rid), InstrData::BinOp(_)) {
                for &r2 in in_build_order(func, referrers(func, Value::Instr(rid))).iter() {
                    if matches!(func.instrs.get(r2), InstrData::If(_)) {
                        return Some((r2, rid));
                    }
                    newrefs.push(r2);
                }
            }
        }
        refs = in_build_order(func, &newrefs);
        depth += 1;
    }
    None
}

/// `ids`, reordered the way go/ssa lists a value's referrers: in the order the
/// builder created them, which — because the builder walks the AST — is source
/// order.
///
/// guff's block arena is *not* that order: it holds a `for` statement's body
/// before its condition, so `referrers(p)` on
///
/// ```text
/// for i := 0; i < p; i += 2 { if i+1 < p { ... } }
/// ```
///
/// offers `i+1 < p` before `i < p`. Sorting on position puts them back.
/// Instructions with no position sort last rather than first, so a synthetic
/// instruction never displaces a real comparison.
fn in_build_order(func: &Function, ids: &[InstrId]) -> Vec<InstrId> {
    if ids.len() < 2 {
        return ids.to_vec();
    }
    let mut out = ids.to_vec();
    out.sort_by_key(|&iid| {
        let p = func.pos(iid).0;
        if p == 0 {
            u32::MAX
        } else {
            p as u32
        }
    });
    out
}

/// Post-pass: delete issues in if successor blocks (gosec `runSliceBounds` ~255–347).
fn suppress_via_ifs(
    prog: &Program,
    ifs: &HashMap<(FuncId, InstrId), InstrId>,
    reports: &mut HashMap<(FuncId, InstrId), &'static str>,
) {
    for (&(fid, if_id), &binop_id) in ifs {
        let func = prog.functions.get(fid);
        let Some(if_block) = instr_block(func, if_id) else {
            continue;
        };
        let succs = func.blocks.get(if_block).succs.clone();

        let mut bound;
        let mut value = 0i32;
        let mut loop_var: Option<Value> = None;
        let mut len_offset = 0i32;
        let mut is_len_bound = false;

        if let Some((b, v)) = extract_bin_op_bound(prog, func, binop_id) {
            bound = b;
            value = v;
        } else if let Some((lv, off)) = extract_len_bound(prog, func, binop_id) {
            loop_var = Some(lv);
            len_offset = off;
            is_len_bound = true;
            bound = BoundKind::UpperBounded;
        } else {
            continue;
        }

        for (i, &succ) in succs.iter().enumerate() {
            let mut branch_bound = bound;
            if i == 1 {
                branch_bound = inv_bound(branch_bound);
            }
            process_suppress_block(
                prog,
                fid,
                succ,
                0,
                branch_bound,
                value,
                loop_var,
                len_offset,
                is_len_bound,
                reports,
            );
        }
    }
}

fn process_suppress_block(
    prog: &Program,
    fid: FuncId,
    block: BlockId,
    depth: u32,
    bound: BoundKind,
    value: i32,
    loop_var: Option<Value>,
    len_offset: i32,
    is_len_bound: bool,
    reports: &mut HashMap<(FuncId, InstrId), &'static str>,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let func = prog.functions.get(fid);
    let instrs = func.blocks.get(block).instrs.clone();
    for &iid in &instrs {
        if reports.contains_key(&(fid, iid)) {
            match bound {
                BoundKind::LowerUnbounded => {}
                BoundKind::UpperUnbounded | BoundKind::Unbounded => {
                    reports.remove(&(fid, iid));
                }
                BoundKind::UpperBounded => match func.instrs.get(iid) {
                    InstrData::Slice(_) => {
                        if !is_len_bound {
                            let (_, _, m) = get_slice_bounds(prog, func, iid);
                            if is_slice_inside_bounds(0, value, m, value) {
                                reports.remove(&(fid, iid));
                            }
                        }
                    }
                    InstrData::IndexAddr(ia) => {
                        if is_len_bound {
                            if let Some(lv) = loop_var {
                                if let Some(idx_offset) = extract_index_offset(prog, func, ia.index, lv)
                                {
                                    if len_offset + idx_offset - 1 < 0 {
                                        reports.remove(&(fid, iid));
                                    }
                                }
                            }
                        } else if let Some(index_value) =
                            extract_const_int(prog, func, SsaValue::new(ia.index)).map(|n| n as i32)
                        {
                            if is_slice_index_inside_bounds(value, index_value) {
                                reports.remove(&(fid, iid));
                            }
                        }
                    }
                    _ => {}
                },
                BoundKind::Bounded => match func.instrs.get(iid) {
                    InstrData::Slice(_) => {
                        let (_, _, m) = get_slice_bounds(prog, func, iid);
                        if is_slice_inside_bounds(value, value, m, value) {
                            reports.remove(&(fid, iid));
                        }
                    }
                    InstrData::IndexAddr(ia) => {
                        if let Some(index_value) =
                            extract_const_int(prog, func, SsaValue::new(ia.index)).map(|n| n as i32)
                        {
                            if index_value == value {
                                reports.remove(&(fid, iid));
                            }
                        }
                    }
                    _ => {}
                },
            }
        } else if matches!(func.instrs.get(iid), InstrData::If(_)) {
            if let Some(nested_block) = instr_block(func, iid) {
                let nested_succs = func.blocks.get(nested_block).succs.clone();
                for &nb in &nested_succs {
                    process_suppress_block(
                        prog,
                        fid,
                        nb,
                        depth + 1,
                        bound,
                        value,
                        loop_var,
                        len_offset,
                        is_len_bound,
                        reports,
                    );
                }
            }
        }
    }
}

fn instr_block(func: &Function, iid: InstrId) -> Option<BlockId> {
    for (bid, block) in func.live_blocks() {
        if block.instrs.contains(&iid) {
            return Some(bid);
        }
    }
    None
}

fn inv_bound(bound: BoundKind) -> BoundKind {
    match bound {
        BoundKind::LowerUnbounded => BoundKind::UpperUnbounded,
        BoundKind::UpperUnbounded => BoundKind::LowerUnbounded,
        BoundKind::UpperBounded => BoundKind::Unbounded,
        BoundKind::Unbounded => BoundKind::UpperBounded,
        BoundKind::Bounded => BoundKind::Bounded,
    }
}

/// gosec `extractIndexOffset`: index is `loopVar` or `loopVar ± C`.
fn extract_index_offset(
    prog: &Program,
    func: &Function,
    index_val: Value,
    loop_var: Value,
) -> Option<i32> {
    if index_val == loop_var {
        return Some(0);
    }
    let Value::Instr(iid) = index_val else {
        return None;
    };
    let InstrData::BinOp(bin) = func.instrs.get(iid) else {
        return None;
    };
    match bin.op {
        Token::ADD => {
            if bin.x == loop_var {
                return extract_const_int(prog, func, SsaValue::new(bin.y)).map(|n| n as i32);
            }
            if bin.y == loop_var {
                return extract_const_int(prog, func, SsaValue::new(bin.x)).map(|n| n as i32);
            }
        }
        Token::SUB => {
            if bin.x == loop_var {
                return extract_const_int(prog, func, SsaValue::new(bin.y)).map(|n| -(n as i32));
            }
        }
        _ => {}
    }
    None
}

/// The capacity go/ssa's variadic packing would have given the callee's
/// `args ...T` parameter at this call site — the number of arguments past the
/// last declared parameter — together with the callee and that parameter.
///
/// go/ssa turns `f(a, b)` into `Alloc *[2]any` + `Slice` + `f(t)`, and every
/// bound upstream's G602 knows about `f`'s `args` comes from that array's
/// type. guff passes the tail through individually, so this reads the same
/// number off the call.
///
/// `None` is the answer for the shapes that have no such array upstream
/// either, each one measured against golangci-lint rather than reasoned about:
///
/// - a spread call, `f(xs...)`: the length is whatever `xs` holds, and go/ssa
///   forwards the slice instead of building one;
/// - a call through a function *value* or an interface method: upstream needs
///   `Call.Value.(*ssa.Function)` to find the parameter at all;
/// - a non-variadic callee;
/// - an **empty tail**, `f()`: go/ssa passes the `nil` slice constant, not an
///   `Alloc`, so upstream is silent even though the capacity would be 0 and
///   every index into it out of range.
fn variadic_call_cap(prog: &Program, common: &CallCommon) -> Option<(FuncId, ParamId, i32)> {
    if common.ellipsis || common.method.is_some() {
        return None;
    }
    let callee = resolve_static_func(common)?;
    let f = prog.functions.get(callee);
    if !signature_variadic(&prog.type_arena, f.signature?) {
        return None;
    }
    // A method's receiver occupies both `params[0]` and `args[0]`, so counting
    // from the end is right for functions and methods alike.
    let last = f.params.iter().map(|(pid, _)| pid).last()?;
    let n_fixed = f.params.len() - 1;
    if common.args.len() <= n_fixed {
        return None;
    }
    Some((callee, last, (common.args.len() - n_fixed) as i32))
}

fn resolve_static_func(common: &CallCommon) -> Option<FuncId> {
    if let Some(fid) = static_callee(common) {
        return Some(fid);
    }
    match common.value {
        Value::Function(fid) => Some(fid),
        _ => None,
    }
}

enum SliceXKind {
    Alloc,
    Param,
    Slice,
    MakeSlice,
    Other,
}

fn slice_x_kind(prog: &Program, func: &Function, slice_id: InstrId) -> SliceXKind {
    let InstrData::Slice(s) = func.instrs.get(slice_id) else {
        return SliceXKind::Other;
    };
    match s.x {
        Value::Param(_) => SliceXKind::Param,
        Value::Instr(xid) => match func.instrs.get(xid) {
            InstrData::Alloc(_) => SliceXKind::Alloc,
            InstrData::Slice(_) => SliceXKind::Slice,
            InstrData::MakeSlice(_) => SliceXKind::MakeSlice,
            _ => SliceXKind::Other,
        },
        _ => {
            let _ = prog;
            SliceXKind::Other
        }
    }
}

fn extract_array_len(prog: &Program, typ: TypeId) -> Option<i32> {
    let mut t = typ.underlying(&prog.type_arena);
    if let TypeData::Pointer(p) = prog.type_arena.get(t) {
        t = p.elem().underlying(&prog.type_arena);
    }
    match prog.type_arena.get(t) {
        TypeData::Array(a) if a.len() >= 0 => Some(a.len() as i32),
        _ => None,
    }
}

fn get_slice_bounds(prog: &Program, func: &Function, slice_id: InstrId) -> (i32, i32, i32) {
    let InstrData::Slice(s) = func.instrs.get(slice_id) else {
        return (0, 0, 0);
    };
    let low = s
        .low
        .and_then(|v| extract_const_int(prog, func, SsaValue::new(v)))
        .unwrap_or(0) as i32;
    let high = s
        .high
        .and_then(|v| extract_const_int(prog, func, SsaValue::new(v)))
        .unwrap_or(0) as i32;
    let max_idx = s
        .max
        .and_then(|v| extract_const_int(prog, func, SsaValue::new(v)))
        .unwrap_or(0) as i32;
    (low, high, max_idx)
}

fn compute_slice_new_cap(l: i32, h: i32, max_idx: i32, old_cap: i32) -> i32 {
    if max_idx > 0 {
        return max_idx - l;
    }
    if l == 0 && h == 0 {
        return old_cap;
    }
    if l > 0 && h == 0 {
        return old_cap - l;
    }
    if l == 0 && h > 0 {
        return h;
    }
    h - l
}

fn is_slice_inside_bounds(l: i32, h: i32, cl: i32, ch: i32) -> bool {
    (l <= cl && h >= ch) && (l <= ch && h >= cl)
}

fn is_three_index_slice_inside_bounds(l: i32, h: i32, max_idx: i32, old_cap: i32) -> bool {
    l >= 0 && h >= l && max_idx >= h && max_idx <= old_cap
}

fn is_slice_index_inside_bounds(h: i32, index: i32) -> bool {
    0 <= index && index < h
}

fn decompose_index(prog: &Program, func: &Function, v: Value) -> (Value, i32) {
    let Value::Instr(iid) = v else {
        return (v, 0);
    };
    match func.instrs.get(iid) {
        InstrData::BinOp(b) if b.op == Token::ADD => {
            if let Some(n) = extract_const_int(prog, func, SsaValue::new(b.y)) {
                let (base, off) = decompose_index(prog, func, b.x);
                return (base, off + n as i32);
            }
            if let Some(n) = extract_const_int(prog, func, SsaValue::new(b.x)) {
                let (base, off) = decompose_index(prog, func, b.y);
                return (base, off + n as i32);
            }
        }
        InstrData::BinOp(b) if b.op == Token::SUB => {
            if let Some(n) = extract_const_int(prog, func, SsaValue::new(b.y)) {
                let (base, off) = decompose_index(prog, func, b.x);
                return (base, off - n as i32);
            }
        }
        _ => {}
    }
    (v, 0)
}

fn extract_bin_op_bound(prog: &Program, func: &Function, iid: InstrId) -> Option<(BoundKind, i32)> {
    let InstrData::BinOp(b) = func.instrs.get(iid) else {
        return None;
    };
    if let Some(n) = extract_const_int(prog, func, SsaValue::new(b.x)).map(|v| v as i32) {
        return match b.op {
            Token::LSS | Token::LEQ => Some((BoundKind::UpperUnbounded, n)),
            Token::GTR | Token::GEQ => Some((BoundKind::LowerUnbounded, n)),
            Token::EQL => Some((BoundKind::Bounded, n)),
            Token::NEQ => Some((BoundKind::Unbounded, n)),
            _ => None,
        };
    }
    if let Some(n) = extract_const_int(prog, func, SsaValue::new(b.y)).map(|v| v as i32) {
        return match b.op {
            Token::LSS | Token::LEQ => Some((BoundKind::LowerUnbounded, n)),
            Token::GTR | Token::GEQ => Some((BoundKind::UpperUnbounded, n)),
            Token::EQL => Some((BoundKind::Bounded, n)),
            Token::NEQ => Some((BoundKind::Unbounded, n)),
            _ => None,
        };
    }
    None
}

#[derive(Clone, Copy)]
enum BoundKind {
    LowerUnbounded,
    UpperUnbounded,
    Unbounded,
    UpperBounded,
    Bounded,
}

/// `i < len…` style bound (gosec `extractLenBound`); only LSS.
fn extract_len_bound(prog: &Program, func: &Function, iid: InstrId) -> Option<(Value, i32)> {
    let InstrData::BinOp(b) = func.instrs.get(iid) else {
        return None;
    };
    if b.op != Token::LSS {
        return None;
    }
    if extract_const_int(prog, func, SsaValue::new(b.y)).is_some() {
        return None; // RHS constant → not a len bound
    }

    let loop_var = b.x;

    // RHS: len +/- const
    if let Value::Instr(yid) = b.y {
        if let InstrData::BinOp(rhs) = func.instrs.get(yid) {
            if matches!(rhs.op, Token::ADD | Token::SUB) {
                let mut const_val: Option<i32> = None;
                if let Some(n) = extract_const_int(prog, func, SsaValue::new(rhs.y)) {
                    const_val = Some(n as i32);
                } else if let Some(n) = extract_const_int(prog, func, SsaValue::new(rhs.x)) {
                    const_val = Some(n as i32);
                }
                if let Some(cv) = const_val {
                    match rhs.op {
                        Token::ADD => return Some((loop_var, cv)),
                        Token::SUB => {
                            if extract_const_int(prog, func, SsaValue::new(rhs.x)).is_some() {
                                // k - len → skip
                            } else {
                                return Some((loop_var, -cv));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // LHS: (loopVar +/- const) < len
    if let Value::Instr(xid) = b.x {
        if let InstrData::BinOp(lhs) = func.instrs.get(xid) {
            if matches!(lhs.op, Token::ADD | Token::SUB) {
                let (const_val, var_val) = if let Some(n) = extract_const_int(prog, func, SsaValue::new(lhs.y))
                {
                    (Some(n as i32), Some(lhs.x))
                } else if let Some(n) = extract_const_int(prog, func, SsaValue::new(lhs.x)) {
                    (Some(n as i32), Some(lhs.y))
                } else {
                    (None, None)
                };
                if let (Some(cv), Some(vv)) = (const_val, var_val) {
                    let off = match lhs.op {
                        Token::ADD => -cv,
                        Token::SUB => cv,
                        _ => 0,
                    };
                    return Some((vv, off));
                }
            }
        }
    }

    Some((loop_var, 0))
}

fn extract_int_value_index_addr(
    prog: &Program,
    fid: FuncId,
    index_addr_id: InstrId,
    slice_cap: i32,
) -> Option<i32> {
    let func = prog.functions.get(fid);
    let InstrData::IndexAddr(ia) = func.instrs.get(index_addr_id) else {
        return None;
    };
    let (base, offset) = decompose_index(prog, func, ia.index);

    if let Some(val) = extract_const_int(prog, func, SsaValue::new(base)).map(|n| n as i32) {
        let final_idx = val + offset;
        if !is_slice_index_inside_bounds(slice_cap, final_idx) {
            return Some(final_idx);
        }
        return None;
    }

    let Value::Instr(phi_id) = base else {
        return None;
    };
    let InstrData::Phi(phi) = func.instrs.get(phi_id) else {
        return None;
    };

    let mut start = 0i32;
    let mut has_start = false;
    let mut next: Option<Value> = None;
    for edge in &phi.edges {
        let Some(e) = edge else {
            continue;
        };
        let (e_base, e_off) = decompose_index(prog, func, *e);
        if let Some(val) = extract_const_int(prog, func, SsaValue::new(e_base)).map(|n| n as i32) {
            start = val + e_off;
            has_start = true;
            if !is_slice_index_inside_bounds(slice_cap, start + offset) {
                return Some(start + offset);
            }
        } else {
            next = Some(*e);
        }
    }

    if !(has_start && next.is_some()) {
        return None;
    }
    let next = next.unwrap();
    let (n_base, n_offset) = decompose_index(prog, func, next);

    let mut search = vec![Value::Instr(phi_id), n_base];
    if n_base != next {
        search.push(next);
    }

    for v in search {
        for &rid in referrers(func, v) {
            let InstrData::BinOp(bin) = func.instrs.get(rid) else {
                continue;
            };
            if let Some((bound, limit)) = extract_bin_op_bound(prog, func, rid) {
                let incr = if bin.op == Token::LSS { -1 } else { 0 };
                let max_v = limit + incr;
                let mut bound_adjust = 0;
                if (v == next && base != next)
                    || (v == n_base && n_base != Value::Instr(phi_id) && base != n_base)
                {
                    bound_adjust = -n_offset;
                }
                if matches!(bound, BoundKind::LowerUnbounded | BoundKind::UpperBounded) {
                    let final_max = max_v + bound_adjust;
                    if !is_slice_index_inside_bounds(slice_cap, final_max + offset) {
                        return Some(final_max + offset);
                    }
                }
            } else if let Some((_loop_var, off)) = extract_len_bound(prog, func, rid) {
                // Limit is the tracked sliceCap (gosec FP path for i < len(other)).
                let limit = slice_cap;
                let max_v = limit + off - 1; // LSS
                let mut bound_adjust = 0;
                if (v == next && base != next)
                    || (v == n_base && n_base != Value::Instr(phi_id) && base != n_base)
                {
                    bound_adjust = -n_offset;
                }
                let final_max = max_v + bound_adjust;
                if !is_slice_index_inside_bounds(slice_cap, final_max + offset) {
                    return Some(final_max + offset);
                }
            }
        }
    }

    None
}
