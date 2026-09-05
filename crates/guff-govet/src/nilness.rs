//! `nilness` — report degenerate nil comparisons and definite nil dereferences.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/nilness` at x/tools v0.44.0,
//! the revision golangci-lint 2.12.2 pins.
//!
//! The analysis walks each source function's **dominator tree** carrying a
//! stack of facts of the form "this value is nil" / "is not nil". Walking the
//! dom tree rather than the CFG is what makes the stack discipline work: a
//! fact learned on a branch holds for exactly the subtree that branch
//! dominates, so it can be popped on the way out instead of being stored
//! per block.
//!
//! This is the first SSA-based analyzer in `guff-govet`; the other 35 are
//! AST/type based. It takes its SSA from the shared `buildir` pass, the way
//! `nilnesserr` (which ports the same fact walk) does. That costs an SSA
//! build only when the analyzer actually runs: `nilness` is not one of
//! `cmd/vet`'s default analyzers, so golangci reaches it only under
//! `govet.enable-all` or an explicit `govet.enable`.
//!
//! # Measured gap
//!
//! Upstream's `conversionpanic` category (`nil slice being cast to an array of
//! len > 0 will always panic`) is not reachable here: guff-ssa models
//! `SliceToArrayPointer` as an operand-less placeholder — it carries neither
//! the slice nor a result type, so `result_type()` answers `None` and the
//! builder never emits one — and the category needs the operand *and* the
//! target array's length. The same placeholder costs `nilnessOf`'s unwrapping
//! arm for that instruction.
//!
//! Measured, with `govet.enable: [nilness]`, on
//!
//! ```go
//! func conv(s []byte) byte {
//!     if s == nil {
//!         p := (*[1]byte)(s)
//!         return p[0]
//!     }
//!     return 0
//! }
//! ```
//!
//! golangci-lint 2.12.2 reports one finding at the conversion and guff
//! reports none. Closing it means building the instruction in guff-ssa, not
//! changing anything here. Nothing else in the pass is elided.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::ids::{BlockId, ConstId, FuncId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::{BasicKind, ObjectArena, PackageArena, TypeArena, TypeId};

/// `-1` non-nil, `0` unknown, `1` nil — upstream's `nilness` int, whose sign
/// carries the negation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Nilness {
    IsNonNil,
    Unknown,
    IsNil,
}

impl Nilness {
    fn negate(self) -> Nilness {
        match self {
            Nilness::IsNonNil => Nilness::IsNil,
            Nilness::Unknown => Nilness::Unknown,
            Nilness::IsNil => Nilness::IsNonNil,
        }
    }

    /// (Go: `nilnessStrings`.)
    fn as_str(self) -> &'static str {
        match self {
            Nilness::IsNonNil => "non-nil",
            Nilness::Unknown => "unknown",
            Nilness::IsNil => "nil",
        }
    }
}

/// "this block is dominated by `value == nil`" / "`value != nil`".
#[derive(Clone, Copy)]
struct Fact {
    value: Value,
    nilness: Nilness,
}

impl Fact {
    fn negate(self) -> Fact {
        Fact {
            value: self.value,
            nilness: self.nilness.negate(),
        }
    }
}

/// What a value's core type is, reduced to the cases the pass switches on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum CoreClass {
    Pointer,
    Slice,
    /// Map, Signature, Chan, Interface, or `unsafe.Pointer` — the remaining
    /// `isNillable` cases, none of which the `IndexAddr` switch distinguishes.
    OtherNillable,
    NotNillable,
}

impl CoreClass {
    fn is_nillable(self) -> bool {
        self != CoreClass::NotNillable
    }
}

/// Type questions answered up front, while the type arena can still be
/// borrowed mutably (computing a type parameter's type set is lazy and
/// mutating, and the walk itself holds the program immutably).
#[derive(Default)]
struct TypeFacts {
    /// `IndexAddr` → the core class of the indexed operand.
    index_core: HashMap<InstrId, CoreClass>,
    /// `Slice` instructions whose operand's underlying type is a pointer,
    /// i.e. the only slice expressions that nil-check.
    slice_of_array_ptr: HashSet<InstrId>,
    /// `MakeInterface` instructions whose nilness is *not* statically non-nil:
    /// the operand is a type parameter that can be instantiated as an
    /// interface (x/tools #66835).
    make_interface_unknown: HashSet<InstrId>,
    /// `TypeAssert` instructions asserting to a nillable type.
    assert_nillable: HashSet<InstrId>,
    /// Call/Go/Defer in invoke mode whose receiver is a type parameter: a nil
    /// receiver may be legitimate there.
    call_type_param_recv: HashSet<InstrId>,
    /// Constants that are nil in go/ssa's sense — no value *and* a nillable
    /// type. guff's `Const::is_nil` omits the second half (the zero value of a
    /// struct or array also has no value), which would make
    /// `panic(struct{}{})` look like a nil panic.
    nil_consts: HashSet<ConstId>,
}

pub(crate) fn collect_nilness(
    prog: &Program,
    ta: &mut TypeArena,
    src_funcs: &[FuncId],
    out: &mut Vec<(u32, String)>,
) {
    for &fid in src_funcs {
        let f = prog.functions.get(fid);
        if f.blocks.is_empty() {
            continue;
        }
        // Visit the entry block. (Go: `fn.Blocks[0]`; guff's arena keeps
        // blocks in semantic order, and block 0 is the entry.)
        let Some(entry) = f
            .live_blocks()
            .find(|(_, b)| b.index == 0)
            .map(|(id, _)| id)
        else {
            continue;
        };
        // `seen` is indexed by `BasicBlock.index`.
        let max_index = f.live_blocks().map(|(_, b)| b.index).max().unwrap_or(-1);
        if max_index < 0 {
            continue;
        }
        let mut ctx = Ctx {
            prog,
            fid,
            facts: precompute_type_facts(prog, ta, fid),
            extracts: extracts_by_tuple(prog, fid),
            block_of: block_of_instr(prog, fid),
            out: Vec::new(),
            seen: vec![false; max_index as usize + 1],
        };
        ctx.visit(entry, Vec::with_capacity(20));
        out.append(&mut ctx.out);
    }
}

struct Ctx<'a> {
    prog: &'a Program,
    fid: FuncId,
    facts: TypeFacts,
    /// `Extract`s keyed by the tuple they destructure, in instruction order —
    /// go/ssa's `Referrers()` filtered to what the comma-ok pattern needs.
    extracts: HashMap<Value, Vec<InstrId>>,
    block_of: HashMap<InstrId, BlockId>,
    out: Vec<(u32, String)>,
    seen: Vec<bool>,
}

impl Ctx<'_> {
    fn func(&self) -> &guff_ssa::function::Function {
        self.prog.functions.get(self.fid)
    }

    fn block(&self, b: BlockId) -> &guff_ssa::block::BasicBlock {
        self.func().blocks.get(b)
    }

    /// (Go: the `reportf` closure — a nil-checking instruction that does not
    /// correspond to syntax has no position and is not reported.)
    fn report(&mut self, iid: InstrId, msg: String) {
        let pos = self.func().pos(iid).0;
        if pos > 0 {
            self.out.push((pos as u32, msg));
        }
    }

    /// Report `descr` when `v` is provably nil. (Go: `notNil`.)
    fn not_nil(&mut self, stack: &[Fact], iid: InstrId, v: Value, descr: &str) {
        if self.nilness_of(stack, v) == Nilness::IsNil {
            self.report(iid, descr.to_string());
        }
    }

    fn visit(&mut self, b: BlockId, stack: Vec<Fact>) {
        let idx = self.block(b).index as usize;
        if idx < self.seen.len() {
            if self.seen[idx] {
                return;
            }
            self.seen[idx] = true;
        }

        self.report_dereferences(b, &stack);
        self.report_panics(b, &stack);

        if self.visit_nil_comparison(b, &stack) {
            return;
        }
        // Upstream does not return after the comma-ok pattern; it falls
        // through to the plain dominee walk below, which `seen` makes a no-op
        // for the successor already visited.
        self.visit_comma_ok_type_assert(b, &stack);

        for d in self.block(b).dominees().to_vec() {
            self.visit(d, stack.clone());
        }
    }

    fn report_dereferences(&mut self, b: BlockId, stack: &[Fact]) {
        for iid in self.block(b).instrs.clone() {
            let instr = self.func().instrs.get(iid);
            match instr {
                InstrData::Call(_) | InstrData::Go(_) | InstrData::Defer(_) => {
                    let cc = self.call_common(iid).expect("matched a call instruction");
                    let value = cc.value;
                    // A nil receiver may be okay for type params.
                    if !self.facts.call_type_param_recv.contains(&iid) {
                        let descr = format!("nil dereference in {}", self.call_description(iid));
                        self.not_nil(stack, iid, value, &descr);
                    }
                }
                InstrData::FieldAddr(f) => {
                    let x = f.x;
                    self.not_nil(stack, iid, x, "nil dereference in field selection");
                }
                InstrData::IndexAddr(ia) => {
                    let x = ia.x;
                    match self.facts.index_core.get(&iid).copied() {
                        Some(CoreClass::Pointer) => {
                            // *array
                            self.not_nil(stack, iid, x, "nil dereference in array index operation");
                        }
                        Some(CoreClass::Slice) => {
                            // Not necessarily a runtime error, because it is
                            // usually dominated by a bounds check.
                            let descr = if self.is_range_index(iid) {
                                "range of nil slice"
                            } else {
                                "index of nil slice"
                            };
                            self.not_nil(stack, iid, x, descr);
                        }
                        _ => {}
                    }
                }
                InstrData::MapUpdate(m) => {
                    let map = m.map;
                    self.not_nil(stack, iid, map, "nil dereference in map update");
                }
                InstrData::Range(r) => {
                    // (Not a runtime error, but a likely mistake.)
                    let x = r.x;
                    self.not_nil(stack, iid, x, "range over nil map");
                }
                InstrData::Slice(s) => {
                    // A nil check occurs in `ptr[:]` only when ptr is a
                    // pointer to an array.
                    let x = s.x;
                    if self.facts.slice_of_array_ptr.contains(&iid) {
                        self.not_nil(stack, iid, x, "nil dereference in slice operation");
                    }
                }
                InstrData::Store(s) => {
                    let addr = s.addr;
                    self.not_nil(stack, iid, addr, "nil dereference in store");
                }
                InstrData::TypeAssert(t) => {
                    let (x, comma_ok) = (t.x, t.comma_ok);
                    if !comma_ok {
                        self.not_nil(stack, iid, x, "nil dereference in type assertion");
                    }
                }
                InstrData::UnOp(u) => {
                    let (op, x) = (u.op, u.x);
                    match op {
                        Token::MUL => self.not_nil(stack, iid, x, "nil dereference in load"),
                        // (Not a runtime error, but a likely mistake.)
                        Token::ARROW => self.not_nil(stack, iid, x, "receive from nil channel"),
                        _ => {}
                    }
                }
                InstrData::Send(s) => {
                    // (Not a runtime error, but a likely mistake.)
                    let chan = s.chan;
                    self.not_nil(stack, iid, chan, "send to nil channel");
                }
                _ => {}
            }
        }
    }

    fn report_panics(&mut self, b: BlockId, stack: &[Fact]) {
        for iid in self.block(b).instrs.clone() {
            if let InstrData::Panic(p) = self.func().instrs.get(iid) {
                let x = p.x;
                if self.nilness_of(stack, x) == Nilness::IsNil {
                    self.report(iid, "panic with nil value".to_string());
                }
            }
            // The `SliceToArrayPointer` arm is unreachable here; see the
            // module doc.
        }
    }

    /// For nil-comparison blocks, report a degenerate condition and push a
    /// nilness fact when visiting the true and false successors. Returns true
    /// when it took over the dominee walk (upstream `return`s in both arms).
    fn visit_nil_comparison(&mut self, b: BlockId, stack: &[Fact]) -> bool {
        let Some((binop_iid, op, x, y, tsucc, fsucc)) = self.eq(b) else {
            return false;
        };
        let xnil = self.nilness_of(stack, x);
        let ynil = self.nilness_of(stack, y);

        if ynil != Nilness::Unknown
            && xnil != Nilness::Unknown
            && (xnil == Nilness::IsNil || ynil == Nilness::IsNil)
        {
            // Degenerate condition: both operands known, at least one nil.
            let adj = if (xnil == ynil) == (op == Token::EQL) {
                "tautological"
            } else {
                "impossible"
            };
            let op_str = if op == Token::EQL { "==" } else { "!=" };
            self.report(
                binop_iid,
                format!(
                    "{adj} condition: {} {op_str} {}",
                    xnil.as_str(),
                    ynil.as_str()
                ),
            );

            // Whichever successor's sole incoming edge is impossible is
            // unreachable: prune it and everything it dominates.
            let skip = if xnil == ynil { fsucc } else { tsucc };
            for d in self.block(b).dominees().to_vec() {
                if d == skip && self.block(d).preds.len() == 1 {
                    continue;
                }
                self.visit(d, stack.to_vec());
            }
            return true;
        }

        // `if x == nil` / `if nil == y`, the other operand unknown.
        if xnil == Nilness::IsNil || ynil == Nilness::IsNil {
            let learned = if xnil == Nilness::IsNil { y } else { x };
            let new_facts = self.expand_facts(Fact {
                value: learned,
                nilness: Nilness::IsNil,
            });

            for d in self.block(b).dominees().to_vec() {
                // Successors learn a fact only at non-critical edges.
                let mut s = stack.to_vec();
                if self.block(d).preds.len() == 1 {
                    if d == tsucc {
                        s.extend(new_facts.iter().copied());
                    } else if d == fsucc {
                        s.extend(new_facts.iter().map(|f| f.negate()));
                    }
                }
                self.visit(d, s);
            }
            return true;
        }
        false
    }

    /// ```text
    /// if ptr, ok := x.(*T); ok { ... } else { fsucc }
    /// ```
    /// `fsucc` learns that `ptr == nil`, since that is its zero value.
    fn visit_comma_ok_type_assert(&mut self, b: BlockId, stack: &[Fact]) {
        let Some(&last) = self.block(b).instrs.last() else {
            return;
        };
        let InstrData::If(iff) = self.func().instrs.get(last) else {
            return;
        };
        let succs = self.block(b).succs.clone();
        if succs.len() != 2 {
            return;
        }
        // Handle "if ok" and "if !ok" variants.
        let (mut cond, mut fsucc) = (iff.cond, succs[1]);
        if let Value::Instr(cid) = cond {
            if let InstrData::UnOp(u) = self.func().instrs.get(cid) {
                if u.op == Token::NOT {
                    cond = u.x;
                    fsucc = succs[0];
                }
            }
        }

        // Match:
        //   t0 = typeassert,ok (pointerlike)
        //   t1 = extract t0 #0  // ptr
        //   t2 = extract t0 #1  // ok
        //   if t2 goto tsucc, fsucc
        let Value::Instr(cid) = cond else { return };
        let InstrData::Extract(e1) = self.func().instrs.get(cid) else {
            return;
        };
        if e1.index != 1 {
            return;
        }
        let tuple = e1.tuple;
        let Value::Instr(tid) = tuple else { return };
        if !matches!(self.func().instrs.get(tid), InstrData::TypeAssert(_)) {
            return;
        }
        if !self.facts.assert_nillable.contains(&tid) {
            return;
        }

        let Some(candidates) = self.extracts.get(&tuple).cloned() else {
            return;
        };
        for eid in candidates {
            let InstrData::Extract(e0) = self.func().instrs.get(eid) else {
                continue;
            };
            if e0.index != 0 {
                continue;
            }
            for d in self.block(b).dominees().to_vec() {
                if self.block(d).preds.len() == 1 && d == fsucc {
                    let mut s = stack.to_vec();
                    s.push(Fact {
                        value: Value::Instr(eid),
                        nilness: Nilness::IsNil,
                    });
                    self.visit(d, s);
                }
            }
        }
    }

    /// The block's terminating `if` on an equality comparison, with its
    /// (equal, not-equal) successors. (Go: `eq`.)
    #[allow(clippy::type_complexity)]
    fn eq(&self, b: BlockId) -> Option<(InstrId, Token, Value, Value, BlockId, BlockId)> {
        let &last = self.block(b).instrs.last()?;
        let InstrData::If(iff) = self.func().instrs.get(last) else {
            return None;
        };
        let Value::Instr(cid) = iff.cond else {
            return None;
        };
        let InstrData::BinOp(binop) = self.func().instrs.get(cid) else {
            return None;
        };
        let succs = &self.block(b).succs;
        if succs.len() != 2 {
            return None;
        }
        match binop.op {
            Token::EQL => Some((cid, Token::EQL, binop.x, binop.y, succs[0], succs[1])),
            Token::NEQ => Some((cid, Token::NEQ, binop.x, binop.y, succs[1], succs[0])),
            _ => None,
        }
    }

    /// (Go: `nilnessOf`.)
    fn nilness_of(&self, stack: &[Fact], v: Value) -> Nilness {
        // Unwrap the wrappers whose nilness is that of the value inside. This
        // is in addition to `expand_facts`: it covers values whose nilness is
        // intrinsic rather than inferred.
        if let Value::Instr(iid) = v {
            match self.func().instrs.get(iid) {
                InstrData::ChangeInterface(c) => {
                    let under = self.nilness_of(stack, c.x);
                    if under != Nilness::Unknown {
                        return under;
                    }
                }
                InstrData::MakeInterface(_) => {
                    // Non-nil unless the operand is a type parameter that can
                    // be instantiated as an interface.
                    if !self.facts.make_interface_unknown.contains(&iid) {
                        return Nilness::IsNonNil;
                    }
                }
                InstrData::Slice(s) => {
                    let under = self.nilness_of(stack, s.x);
                    if under != Nilness::Unknown {
                        return under;
                    }
                }
                _ => {}
            }
        }

        // Is the value intrinsically nil or non-nil?
        match v {
            Value::Instr(iid) => match self.func().instrs.get(iid) {
                InstrData::Alloc(_)
                | InstrData::FieldAddr(_)
                | InstrData::IndexAddr(_)
                | InstrData::MakeChan(_)
                | InstrData::MakeClosure(_)
                | InstrData::MakeMap(_)
                | InstrData::MakeSlice(_) => return Nilness::IsNonNil,
                _ => {}
            },
            Value::FreeVar(_) | Value::Function(_) | Value::Global(_) => return Nilness::IsNonNil,
            Value::Const(cid) => {
                return if self.facts.nil_consts.contains(&cid) {
                    Nilness::IsNil // nil or zero value of a pointer-like type
                } else {
                    Nilness::Unknown // non-pointer
                };
            }
            _ => {}
        }

        // Search dominating control-flow facts.
        for f in stack {
            if f.value == v {
                return f.nilness;
            }
        }
        Nilness::Unknown
    }

    /// `ChangeInterface` has transitive nilness: knowing the underlying value
    /// is nil means the wrapper is too, and vice versa. (Go: `expandFacts`;
    /// `ChangeInterface` is still the only expansion upstream supports.)
    fn expand_facts(&self, f: Fact) -> Vec<Fact> {
        let mut out = vec![f];
        let mut cur = f;
        loop {
            let Value::Instr(iid) = cur.value else { break };
            let InstrData::ChangeInterface(c) = self.func().instrs.get(iid) else {
                break;
            };
            cur = Fact {
                value: c.x,
                nilness: cur.nilness,
            };
            out.push(cur);
        }
        out
    }

    fn call_common(&self, iid: InstrId) -> Option<&CallCommon> {
        match self.func().instrs.get(iid) {
            InstrData::Call(c) => Some(&c.call),
            InstrData::Go(g) => Some(&g.call),
            InstrData::Defer(d) => Some(&d.call),
            _ => None,
        }
    }

    /// (Go: `CallCommon.Description`.)
    fn call_description(&self, iid: InstrId) -> &'static str {
        let Some(cc) = self.call_common(iid) else {
            return "dynamic function call";
        };
        match cc.value {
            Value::Builtin(_) => return "built-in function call",
            Value::Instr(vid) => {
                if matches!(self.func().instrs.get(vid), InstrData::MakeClosure(_)) {
                    return "static function closure call";
                }
            }
            Value::Function(fid) => {
                let has_recv = self
                    .prog
                    .functions
                    .get(fid)
                    .signature
                    .and_then(|sig| match self.prog.type_arena.get(sig) {
                        TypeData::Signature(s) => Some(s.recv().is_some()),
                        _ => None,
                    })
                    .unwrap_or(false);
                return if has_recv {
                    "static method call"
                } else {
                    "static function call"
                };
            }
            _ => {}
        }
        if cc.method.is_some() {
            "dynamic method call" // ("invoke" mode)
        } else {
            "dynamic function call"
        }
    }

    /// Whether `iid` is a slice index within a `for range slice` loop, by
    /// reverse-engineering go/ssa's lowering:
    ///
    /// ```text
    ///      n = len(x)
    ///      jump loop
    /// loop:                                "rangeindex.loop"
    ///      phi = φ(-1, incr) #rangeindex
    ///      incr = phi + 1
    ///      cond = incr < n
    ///      if cond goto body else done
    /// body:                                "rangeindex.body"
    ///      instr = &x[incr]
    /// ```
    ///
    /// (Go: `isRangeIndex`.)
    fn is_range_index(&self, iid: InstrId) -> bool {
        let InstrData::IndexAddr(ia) = self.func().instrs.get(iid) else {
            return false;
        };
        let (index, x) = (ia.index, ia.x);
        let Value::Instr(incr_id) = index else {
            return false;
        };
        let InstrData::BinOp(incr) = self.func().instrs.get(incr_id) else {
            return false;
        };
        if incr.op != Token::ADD {
            return false;
        }
        let Some(&bid) = self.block_of.get(&incr_id) else {
            return false;
        };
        if self.block(bid).comment != "rangeindex.loop" {
            return false;
        }
        let Some(&last) = self.block(bid).instrs.last() else {
            return false;
        };
        let InstrData::If(iff) = self.func().instrs.get(last) else {
            return false;
        };
        let Value::Instr(cond_id) = iff.cond else {
            return false;
        };
        let InstrData::BinOp(cond) = self.func().instrs.get(cond_id) else {
            return false;
        };
        if cond.x != Value::Instr(incr_id) || cond.op != Token::LSS {
            return false;
        }
        let Value::Instr(call_id) = cond.y else {
            return false;
        };
        let InstrData::Call(call) = self.func().instrs.get(call_id) else {
            return false;
        };
        let Value::Builtin(b) = call.call.value else {
            return false;
        };
        if self.prog.builtins.get(b).name != "len" {
            return false;
        }
        call.call.args.first() == Some(&x)
    }
}

/// go/ssa's `Referrers()`, narrowed to the `Extract`s of each tuple, in
/// instruction order.
fn extracts_by_tuple(prog: &Program, fid: FuncId) -> HashMap<Value, Vec<InstrId>> {
    let f = prog.functions.get(fid);
    let mut out: HashMap<Value, Vec<InstrId>> = HashMap::new();
    for (_, b) in f.live_blocks() {
        for &iid in &b.instrs {
            if let InstrData::Extract(e) = f.instrs.get(iid) {
                out.entry(e.tuple).or_default().push(iid);
            }
        }
    }
    out
}

/// go/ssa's `Instruction.Block()`.
fn block_of_instr(prog: &Program, fid: FuncId) -> HashMap<InstrId, BlockId> {
    let f = prog.functions.get(fid);
    let mut out = HashMap::new();
    for (bid, b) in f.live_blocks() {
        for &iid in &b.instrs {
            out.insert(iid, bid);
        }
    }
    out
}

fn precompute_type_facts(prog: &Program, ta: &mut TypeArena, fid: FuncId) -> TypeFacts {
    // Collect the (instruction, type) questions while the program is borrowed
    // immutably, then answer them against the type arena.
    enum Q {
        Index(InstrId, TypeId),
        SliceOp(InstrId, TypeId),
        MakeIface(InstrId, TypeId),
        Assert(InstrId, TypeId),
        CallRecv(InstrId, TypeId),
        Const(ConstId, TypeId),
    }
    let mut qs: Vec<Q> = Vec::new();
    {
        let f = prog.functions.get(fid);
        let push_const = |v: Value, qs: &mut Vec<Q>| {
            if let Value::Const(cid) = v {
                let c = prog.constants.get(cid);
                if c.is_nil() {
                    qs.push(Q::Const(cid, c.typ));
                }
            }
        };
        for (_, b) in f.live_blocks() {
            for &iid in &b.instrs {
                let instr = f.instrs.get(iid);
                instr.for_each_operand(|v| push_const(*v, &mut qs));
                match instr {
                    InstrData::IndexAddr(ia) => {
                        qs.push(Q::Index(iid, value_type_of(prog, f, ia.x)))
                    }
                    InstrData::Slice(s) => qs.push(Q::SliceOp(iid, value_type_of(prog, f, s.x))),
                    InstrData::MakeInterface(m) => {
                        qs.push(Q::MakeIface(iid, value_type_of(prog, f, m.x)))
                    }
                    InstrData::TypeAssert(t) => qs.push(Q::Assert(iid, t.assert_type)),
                    InstrData::Call(_) | InstrData::Go(_) | InstrData::Defer(_) => {
                        let cc = match instr {
                            InstrData::Call(c) => &c.call,
                            InstrData::Go(g) => &g.call,
                            InstrData::Defer(d) => &d.call,
                            _ => unreachable!(),
                        };
                        // `IsInvoke()`: interface method invocation.
                        if cc.method.is_some() {
                            qs.push(Q::CallRecv(iid, value_type_of(prog, f, cc.value)));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut facts = TypeFacts::default();
    let oa = &prog.object_arena;
    let pa = &prog.package_arena;
    for q in qs {
        match q {
            Q::Index(iid, t) => {
                facts.index_core.insert(iid, core_class(ta, oa, pa, t));
            }
            Q::SliceOp(iid, t) => {
                // `is[*types.Pointer](instr.X.Type().Underlying())` — plain
                // `Underlying`, not the core type.
                if matches!(ta.get(t.underlying(ta)), TypeData::Pointer(_)) {
                    facts.slice_of_array_ptr.insert(iid);
                }
            }
            Q::MakeIface(iid, t) => {
                let u = guff_types::alias::unalias(ta, t);
                if matches!(ta.get(u), TypeData::TypeParam(_))
                    && !type_param_has_terms(ta, oa, pa, u)
                {
                    facts.make_interface_unknown.insert(iid);
                }
            }
            Q::Assert(iid, t) => {
                if core_class(ta, oa, pa, t).is_nillable() {
                    facts.assert_nillable.insert(iid);
                }
            }
            Q::CallRecv(iid, t) => {
                if guff_types::predicates::is_type_param(ta, t) {
                    facts.call_type_param_recv.insert(iid);
                }
            }
            Q::Const(cid, t) => {
                if nillable(ta, oa, pa, t) {
                    facts.nil_consts.insert(cid);
                }
            }
        }
    }
    facts
}

/// `typeparams.NormalTerms(tparam.Constraint())` returning a non-empty list —
/// the test x/tools uses to decide a type parameter cannot be instantiated as
/// an interface (#66835).
fn type_param_has_terms(
    ta: &mut TypeArena,
    oa: &ObjectArena,
    pa: &PackageArena,
    tparam: TypeId,
) -> bool {
    let pairs = guff_ssa::typeset::typeset_pairs(ta, oa, pa, tparam);
    pairs.iter().any(|(t, _)| t.is_some())
}

/// `typeparams.CoreType`, classified. Returns [`CoreClass::NotNillable`] where
/// upstream's `CoreType` returns nil, which is what both call sites want:
/// `isNillable` falls through to `false`, and the `IndexAddr` switch matches
/// neither arm.
fn core_class(ta: &mut TypeArena, oa: &ObjectArena, pa: &PackageArena, t: TypeId) -> CoreClass {
    let u = t.underlying(ta);
    if !matches!(ta.get(u), TypeData::Interface(_)) {
        return classify(ta, u);
    }
    // Interface underlying: the core type is the single common underlying of
    // the normal terms, or a channel meet.
    let pairs = guff_ssa::typeset::typeset_pairs(ta, oa, pa, u);
    let terms: Vec<TypeId> = pairs.iter().filter_map(|(_, u)| *u).collect();
    if terms.len() != pairs.len() || terms.is_empty() {
        return CoreClass::NotNillable;
    }
    let first = terms[0].underlying(ta);
    if terms.len() == 1 {
        return classify(ta, first);
    }
    // Beyond a single term the spec defines a core type only for channels;
    // every such core type is a channel, so the class is the same for all the
    // direction/element cases upstream distinguishes.
    if terms
        .iter()
        .all(|&x| matches!(ta.get(x.underlying(ta)), TypeData::Chan(_)))
    {
        // Upstream additionally requires identical element types and
        // compatible directions; both refinements still yield a channel, and
        // failing them yields nil — which is `NotNillable` either way only if
        // the elements differ. Check the elements so a mixed-element union is
        // not misclassified as nillable.
        let elems: Vec<TypeId> = terms
            .iter()
            .filter_map(|&x| match ta.get(x.underlying(ta)) {
                TypeData::Chan(c) => Some(c.elem()),
                _ => None,
            })
            .collect();
        let mut all_same = true;
        for w in elems.windows(2) {
            if !guff_types::predicates::identical(ta, oa, pa, w[0], w[1]) {
                all_same = false;
                break;
            }
        }
        if all_same {
            return CoreClass::OtherNillable;
        }
    }
    CoreClass::NotNillable
}

fn classify(ta: &TypeArena, u: TypeId) -> CoreClass {
    match ta.get(u) {
        TypeData::Pointer(_) => CoreClass::Pointer,
        TypeData::Slice(_) => CoreClass::Slice,
        TypeData::Map(_) | TypeData::Signature(_) | TypeData::Chan(_) | TypeData::Interface(_) => {
            CoreClass::OtherNillable
        }
        TypeData::Basic(b) if b.kind() == BasicKind::UnsafePointer => CoreClass::OtherNillable,
        _ => CoreClass::NotNillable,
    }
}

/// go/ssa's `nillable`: whether `*new(T) == nil` is legal. Note this is
/// `Underlying`-based, not core-type based, and an interface *is* nillable
/// here (unlike in `isNillable`).
///
/// The `untyped nil` arm has no counterpart upstream, and is not a loosening:
/// go/ssa's `emitCompare` retypes a constant operand to the other operand's
/// type before building the `BinOp`, so by the time `nilnessOf` sees the `nil`
/// in `p == nil` its type is `*T` and `nillable` says yes. guff-ssa's
/// `binary_expr` does not port that conversion, so the literal keeps its
/// `untyped nil` type — and a constant with no value at that type is the nil
/// literal and nothing else. Without this the whole comparison-fact half of
/// the analysis is dead: measured on the fixture, 2 of upstream's 19 findings.
fn nillable(ta: &mut TypeArena, oa: &ObjectArena, pa: &PackageArena, t: TypeId) -> bool {
    if matches!(
        ta.get(guff_types::alias::unalias_readonly(ta, t)),
        TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil
    ) {
        return true;
    }
    if guff_types::predicates::is_type_param(ta, t) {
        // Empty type set (u == nil) => any underlying type => not nillable.
        let pairs = guff_ssa::typeset::typeset_pairs(ta, oa, pa, t);
        return pairs.iter().all(|(_, u)| match u {
            Some(u) => {
                let u = *u;
                nillable_under(ta, u)
            }
            None => false,
        });
    }
    nillable_under(ta, t.underlying(ta))
}

fn nillable_under(ta: &TypeArena, u: TypeId) -> bool {
    matches!(
        ta.get(u),
        TypeData::Pointer(_)
            | TypeData::Slice(_)
            | TypeData::Chan(_)
            | TypeData::Map(_)
            | TypeData::Signature(_)
            | TypeData::Interface(_)
    )
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nilness",
        doc: "check for redundant or impossible nil comparisons",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/nilness",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut out: Vec<(u32, String)> = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "nilness requires the buildir analyzer".to_string())?;
        // The type questions below need a mutable arena (a type parameter's
        // type set is computed lazily); the SSA program itself is shared and
        // immutable, so work on a clone the way `nilnesserr` does.
        let mut ta = ir.prog.type_arena.clone();
        collect_nilness(&ir.prog, &mut ta, ir.src_funcs_with_methods(), &mut out);
    }
    for (pos, msg) in out {
        pass.reportf(pos, msg);
    }
    Ok(None)
}
