//! Gosec **G115** — integer overflow conversion (SSA + range analysis).
//!
//! Port of securego/gosec v2.26.1 (the version golangci-lint 2.12.2 pins)
//! `analyzers/conversion_overflow.go` plus the whole of
//! `analyzers/range_analyzer.go` that a conversion can reach, and the
//! `GetIntTypeInfo` / `IsConstantInTypeRange` / `ExplicitValsInRange` /
//! `getRealValueFromOperation` / `isEquivalent` / `isSameOrRelated` group from
//! `analyzers/util.go`.
//!
//! The rule itself is two lines — "does the destination type hold every value
//! the source type can hold?" — and answering *only* that reports every
//! `int32(i)` in a corpus. Everything else here is the range analysis that
//! decides a conversion is already guarded: constants, `len`/`cap`, `%`, `&`,
//! `min`/`max`, `strconv.Parse{Int,Uint}` bit sizes, and the `if` edges that
//! dominate the conversion. Upstream needs it badly enough to have put six
//! `#nosec G115` comments in `range_analyzer.go` itself.
//!
//! **Deliberate omissions from `range_analyzer.go`**: the `ByteRange` group
//! (`ResolveByteRange` / `BufferedLen` / `mergeRanges` / `subtractRange` /
//! `Precedes`) belongs to the taint and walk-symlink analyzers, not to a
//! conversion, and `RangeAnalyzer`'s `sync.Pool` + `shared`-flag recycling is a
//! Go allocation strategy with nothing to port: results here are owned values
//! and the memo table clones on hit.
//!
//! Bug-compatible with upstream on two points worth naming, because they are
//! reachable and "fixing" them would move findings:
//! - a result computed at the depth limit is memoized like any other, so the
//!   answer for a value can depend on the path that reached it first;
//! - `ComputeRange`'s `Extract` arm matches the *callee name* `ParseInt` /
//!   `ParseUint`, not `strconv.ParseInt`, so a local function of that name is
//!   read as a bit-size-bounded parse.

use std::collections::{HashMap, HashSet};

use guff::token::Token;
use guff_analysis::callcheck::static_callee;
use guff_analysis::referrers;
use guff_constant::Kind;
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, ConstId, FuncId, InstrId};
use guff_ssa::instr::InstrData;
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::TypeId;

/// gosec `MaxDepth`.
const MAX_DEPTH: u32 = 20;

const MAX_INT64_U: u64 = i64::MAX as u64;

// ---------------------------------------------------------------------------
// util.go: integer type properties
// ---------------------------------------------------------------------------

/// gosec `IntTypeInfo`. `int`/`uint`/`uintptr` are hard-coded 64-bit, exactly
/// as upstream: gosec never consults `types.Sizes`, so a 32-bit target is
/// reported the same way by both tools.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct IntTypeInfo {
    signed: bool,
    size: i32,
    min: i64,
    max: u64,
}

fn to_uint64(i: i64) -> u64 {
    i as u64
}

fn to_int64(u: u64) -> i64 {
    u as i64
}

/// Go's `x << s` / `x >> s` yield 0 (or -1, for a negative signed value shifted
/// right) once `s` reaches the word size, where Rust's shift operators panic.
fn shl_u64(v: u64, s: u64) -> u64 {
    if s >= 64 {
        0
    } else {
        v << s
    }
}

fn shr_u64(v: u64, s: u64) -> u64 {
    if s >= 64 {
        0
    } else {
        v >> s
    }
}

fn shl_i64(v: i64, s: u64) -> i64 {
    if s >= 64 {
        0
    } else {
        to_int64(to_uint64(v) << s)
    }
}

fn shr_i64(v: i64, s: u64) -> i64 {
    if s >= 64 {
        if v < 0 {
            -1
        } else {
            0
        }
    } else {
        v >> s
    }
}

/// `bits.Mul64`.
fn mul64(a: u64, b: u64) -> (u64, u64) {
    let p = (a as u128) * (b as u128);
    ((p >> 64) as u64, p as u64)
}

/// gosec `GetIntTypeInfo`.
fn int_type_info(prog: &Program, t: TypeId) -> Option<IntTypeInfo> {
    let mut u = t.underlying(&prog.type_arena);
    if let TypeData::Pointer(p) = prog.type_arena.get(u) {
        u = p.elem().underlying(&prog.type_arena);
    }
    let TypeData::Basic(b) = prog.type_arena.get(u) else {
        return None;
    };
    let info = match b.kind() {
        BasicKind::Int | BasicKind::Int64 => IntTypeInfo {
            signed: true,
            size: 64,
            min: i64::MIN,
            max: i64::MAX as u64,
        },
        BasicKind::Int8 => IntTypeInfo {
            signed: true,
            size: 8,
            min: i8::MIN as i64,
            max: i8::MAX as u64,
        },
        BasicKind::Int16 => IntTypeInfo {
            signed: true,
            size: 16,
            min: i16::MIN as i64,
            max: i16::MAX as u64,
        },
        BasicKind::Int32 => IntTypeInfo {
            signed: true,
            size: 32,
            min: i32::MIN as i64,
            max: i32::MAX as u64,
        },
        BasicKind::Uint | BasicKind::Uint64 | BasicKind::Uintptr => IntTypeInfo {
            signed: false,
            size: 64,
            min: 0,
            max: u64::MAX,
        },
        BasicKind::Uint8 => IntTypeInfo {
            signed: false,
            size: 8,
            min: 0,
            max: u8::MAX as u64,
        },
        BasicKind::Uint16 => IntTypeInfo {
            signed: false,
            size: 16,
            min: 0,
            max: u16::MAX as u64,
        },
        BasicKind::Uint32 => IntTypeInfo {
            signed: false,
            size: 32,
            min: 0,
            max: u32::MAX as u64,
        },
        _ => return None,
    };
    Some(info)
}

/// `Value.Type()` without [`value_type_of`]'s panics: an operand whose
/// instruction records no result type (guff models `MultiConvert` /
/// `SliceToArrayPointer` as placeholders) must make the conversion unanalyzable,
/// not abort the linter.
fn value_type(prog: &Program, func: &Function, v: Value) -> Option<TypeId> {
    match v {
        Value::Instr(i) => func.instrs.get(i).result_type(),
        Value::Function(f) => prog.functions.get(f).signature,
        _ => Some(value_type_of(prog, func, v)),
    }
}

fn basic_of(prog: &Program, t: TypeId) -> Option<&guff_types::basic::Basic> {
    match prog.type_arena.get(t.underlying(&prog.type_arena)) {
        TypeData::Basic(b) => Some(b),
        _ => None,
    }
}

/// gosec `isPlatformWordType`.
fn is_platform_word_kind(k: BasicKind) -> bool {
    matches!(k, BasicKind::Int | BasicKind::Uint | BasicKind::Uintptr)
}

/// gosec `isSameWidthPlatformConversion`.
fn is_same_width_platform_conversion(prog: &Program, src: TypeId, dst: TypeId) -> bool {
    let (Some(s), Some(d)) = (basic_of(prog, src), basic_of(prog, dst)) else {
        return false;
    };
    is_platform_word_kind(s.kind()) && is_platform_word_kind(d.kind())
}

/// gosec `hasOverflow`.
fn has_overflow(src: IntTypeInfo, dst: IntTypeInfo) -> bool {
    src.min < dst.min || src.max > dst.max
}

/// go/ssa's `NewConst` replaces a `nil` constant value with the *typed zero*
/// (`0` for every numeric type, via `soleTypeKind`); guff-ssa's `Const::new`
/// keeps the `None` — see its `// TODO: soleTypeKind logic if val is None` — so
/// every reader here has to apply the normalization itself.
///
/// This rule must, and not as a nicety: `var acc int` has no initializing store,
/// so the lifter feeds the loop's phi a zero constant on the entry edge, and
/// reading that edge as "no value known" instead of `0` drops the `[0, 0]` bound
/// upstream computes for the phi. That is the whole difference between a finding
/// and silence on
///
/// ```go
/// var acc int
/// for _, b := range bs { acc += int(b) }
/// return uint8(acc)
/// ```
///
/// which golangci-lint does not report. `acc := 0` was never affected — that
/// constant carries a value — which is why the two spellings disagreed.
fn is_zero_valued_numeric_const(prog: &Program, c: ConstId) -> bool {
    let typ = prog.constants.get(c).typ;
    // `contains` is an all-bits test and `IS_NUMERIC` is three bits; a plain
    // `int` sets only `IS_INTEGER`.
    basic_of(prog, typ).is_some_and(|b| (b.info().0 & guff_types::IS_NUMERIC.0) != 0)
}

/// The `int64` view of a constant, with go/ssa's zero normalization applied.
fn const_id_int64(prog: &Program, c: ConstId) -> Option<i64> {
    match prog.constants.get(c).val.as_ref() {
        Some(val) if val.kind() == Kind::Int => {
            let (n, exact) = guff_constant::int64_val(val);
            exact.then_some(n)
        }
        Some(_) => None,
        None => is_zero_valued_numeric_const(prog, c).then_some(0),
    }
}

/// The `uint64` view of a constant, with go/ssa's zero normalization applied.
fn const_id_uint64(prog: &Program, c: ConstId) -> Option<u64> {
    match prog.constants.get(c).val.as_ref() {
        Some(val) if val.kind() == Kind::Int => {
            let (n, exact) = guff_constant::uint64_val(val);
            exact.then_some(n)
        }
        Some(_) => None,
        None => is_zero_valued_numeric_const(prog, c).then_some(0),
    }
}

/// gosec `IsConstantInTypeRange`.
fn is_constant_in_type_range(prog: &Program, c: ConstId, dst: IntTypeInfo) -> bool {
    if dst.signed {
        let Some(v) = const_id_int64(prog, c) else {
            return false;
        };
        return v >= dst.min && to_uint64(v) <= dst.max;
    }
    let Some(v) = const_id_uint64(prog, c) else {
        return false;
    };
    v <= dst.max
}

/// gosec `ExplicitValsInRange`.
fn explicit_vals_in_range(pos: &[u64], neg: &[i64], dst: IntTypeInfo) -> bool {
    pos.iter().any(|&v| v <= dst.max) || neg.iter().any(|&v| v >= dst.min)
}

/// gosec `GetConstantInt64`. Deliberately *not* `callcheck::extract_const_int`:
/// that one flattens `Phi` and `ChangeType` first, and a range analysis that
/// reads a phi as a constant loses the very branch it is trying to measure.
fn constant_int64(prog: &Program, func: &Function, v: Value) -> Option<i64> {
    if let Value::Const(cid) = v {
        return const_id_int64(prog, cid);
    }
    if let Value::Instr(iid) = v {
        if let InstrData::UnOp(u) = func.instrs.get(iid) {
            if u.op == Token::SUB {
                return constant_int64(prog, func, u.x).map(|n| n.wrapping_neg());
            }
        }
    }
    None
}

/// gosec `GetConstantUint64`.
fn constant_uint64(prog: &Program, v: Value) -> Option<u64> {
    let Value::Const(cid) = v else {
        return None;
    };
    const_id_uint64(prog, cid)
}

/// gosec `isUint`.
fn is_uint(prog: &Program, func: &Function, v: Value) -> bool {
    let Some(t) = value_type(prog, func, v) else {
        return false;
    };
    basic_of(prog, t).is_some_and(|b| b.info().contains(guff_types::IS_UNSIGNED))
}

// ---------------------------------------------------------------------------
// util.go: value shapes
// ---------------------------------------------------------------------------

/// The operation `getRealValueFromOperation` peeled off a value. Upstream keys
/// this by the operator's source text (`"<<"`, `"neg"`, `"field"`, …).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    None,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Quo,
    Neg,
    Field,
    Alloc,
}

#[derive(Clone, Copy)]
struct OperationInfo {
    op: Op,
    extra: Option<Value>,
    flipped: bool,
}

impl OperationInfo {
    const NONE: OperationInfo = OperationInfo {
        op: Op::None,
        extra: None,
        flipped: false,
    };
}

/// gosec `getRealValueFromOperation`.
fn real_value_from_operation(
    prog: &Program,
    func: &Function,
    v: Value,
) -> (Value, OperationInfo) {
    let Value::Instr(iid) = v else {
        return (v, OperationInfo::NONE);
    };
    match func.instrs.get(iid) {
        InstrData::BinOp(b) => {
            let op = match b.op {
                Token::SHL => Op::Shl,
                Token::ADD => Op::Add,
                Token::SUB => Op::Sub,
                Token::SHR => Op::Shr,
                Token::MUL => Op::Mul,
                Token::QUO => Op::Quo,
                _ => return (v, OperationInfo::NONE),
            };
            if constant_int64(prog, func, b.y).is_some() {
                return (
                    b.x,
                    OperationInfo {
                        op,
                        extra: Some(b.y),
                        flipped: false,
                    },
                );
            }
            if constant_int64(prog, func, b.x).is_some() {
                return (
                    b.y,
                    OperationInfo {
                        op,
                        extra: Some(b.x),
                        flipped: true,
                    },
                );
            }
            (v, OperationInfo::NONE)
        }
        InstrData::Convert(c) => real_value_from_operation(prog, func, c.x),
        InstrData::UnOp(u) => match u.op {
            Token::SUB => (
                u.x,
                OperationInfo {
                    op: Op::Neg,
                    extra: None,
                    flipped: false,
                },
            ),
            // Load. Follow a load-of-load, and stop at a field address.
            Token::MUL => {
                if let Value::Instr(xid) = u.x {
                    match func.instrs.get(xid) {
                        InstrData::UnOp(inner) if inner.op == Token::MUL => {
                            return real_value_from_operation(prog, func, u.x);
                        }
                        InstrData::FieldAddr(_) => {
                            return (
                                u.x,
                                OperationInfo {
                                    op: Op::Field,
                                    extra: None,
                                    flipped: false,
                                },
                            );
                        }
                        _ => {}
                    }
                }
                (v, OperationInfo::NONE)
            }
            _ => (v, OperationInfo::NONE),
        },
        InstrData::FieldAddr(_) => (
            v,
            OperationInfo {
                op: Op::Field,
                extra: None,
                flipped: false,
            },
        ),
        InstrData::Alloc(_) => (
            v,
            OperationInfo {
                op: Op::Alloc,
                extra: None,
                flipped: false,
            },
        ),
        _ => (v, OperationInfo::NONE),
    }
}

/// Constant identity. go/ssa hands out one `*Const` per occurrence, so
/// upstream's `a == b` misses two spellings of the same literal and its `Const`
/// arm compares `Value` and `Type` instead; guff's `ConstId`s are equally
/// per-occurrence, so the arm is needed here for the same reason.
fn const_eq(prog: &Program, a: ConstId, b: ConstId) -> bool {
    let (ca, cb) = (prog.constants.get(a), prog.constants.get(b));
    if ca.typ != cb.typ {
        return false;
    }
    // Same normalization as `const_id_int64`: a nil-valued numeric constant is
    // the typed zero, and go/ssa compares it equal to a spelled-out `0`.
    let numeric_zero = |c: &guff_ssa::const_val::Const, id: ConstId| -> Option<i64> {
        match c.val.as_ref() {
            None => is_zero_valued_numeric_const(prog, id).then_some(0),
            Some(_) => None,
        }
    };
    match (ca.val.as_ref(), cb.val.as_ref()) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            if x.kind() != y.kind() || x.kind() == Kind::Unknown {
                return false;
            }
            guff_constant::compare(x.clone(), Token::EQL, y.clone())
        }
        (Some(_), None) => numeric_zero(cb, b) == const_id_int64(prog, a),
        (None, Some(_)) => numeric_zero(ca, a) == const_id_int64(prog, b),
    }
}

/// gosec `isEquivalent`.
fn is_equivalent(prog: &Program, func: &Function, a: Value, b: Value) -> bool {
    if a == b {
        return true;
    }
    if let (Value::Const(ca), Value::Const(cb)) = (a, b) {
        return const_eq(prog, ca, cb);
    }
    let (Value::Instr(ia), Value::Instr(ib)) = (a, b) else {
        return false;
    };
    match (func.instrs.get(ia), func.instrs.get(ib)) {
        (InstrData::BinOp(x), InstrData::BinOp(y)) => {
            x.op == y.op
                && is_equivalent(prog, func, x.x, y.x)
                && is_equivalent(prog, func, x.y, y.y)
        }
        (InstrData::UnOp(x), InstrData::UnOp(y)) => {
            x.op == y.op && is_equivalent(prog, func, x.x, y.x)
        }
        _ => false,
    }
}

/// gosec `isSameOrRelated`.
fn is_same_or_related(prog: &Program, func: &Function, a: Value, b: Value) -> bool {
    if a == b {
        return true;
    }
    if let (Value::Instr(ia), Value::Instr(ib)) = (a, b) {
        if let (InstrData::Extract(xa), InstrData::Extract(xb)) =
            (func.instrs.get(ia), func.instrs.get(ib))
        {
            return xa.index == xb.index && is_same_or_related(prog, func, xa.tuple, xb.tuple);
        }
    }
    let (a_val, a_info) = real_value_from_operation(prog, func, a);
    let (b_val, b_info) = real_value_from_operation(prog, func, b);
    if a_val == b_val && a_info.op == b_info.op {
        return true;
    }
    let (Value::Instr(ia), Value::Instr(ib)) = (a_val, b_val) else {
        return false;
    };
    match (func.instrs.get(ia), func.instrs.get(ib)) {
        (InstrData::FieldAddr(fa), InstrData::FieldAddr(fb)) => {
            fa.field == fb.field && is_same_or_related(prog, func, fa.x, fb.x)
        }
        (InstrData::IndexAddr(xa), InstrData::IndexAddr(xb)) => {
            is_same_or_related(prog, func, xa.x, xb.x)
                && is_same_or_related(prog, func, xa.index, xb.index)
        }
        (InstrData::UnOp(ua), InstrData::UnOp(ub))
            if ua.op == Token::MUL && ub.op == Token::MUL =>
        {
            is_same_or_related(prog, func, ua.x, ub.x)
        }
        _ => false,
    }
}

/// gosec `IsRangeCheck`.
fn is_range_check_cond(prog: &Program, func: &Function, cond: Value, x: Value) -> bool {
    let (compare_val, _) = real_value_from_operation(prog, func, x);
    let Value::Instr(cid) = cond else {
        return false;
    };
    let InstrData::BinOp(op) = func.instrs.get(cid) else {
        return false;
    };
    if !matches!(
        op.op,
        Token::LSS | Token::LEQ | Token::GTR | Token::GEQ | Token::EQL | Token::NEQ
    ) {
        return false;
    }
    let side_matches = |side: Value| {
        if is_same_or_related(prog, func, side, x)
            || is_same_or_related(prog, func, side, compare_val)
        {
            return true;
        }
        let (rval, _) = real_value_from_operation(prog, func, side);
        rval == x || rval == compare_val
    };
    side_matches(op.x) || side_matches(op.y)
}

// ---------------------------------------------------------------------------
// range_analyzer.go
// ---------------------------------------------------------------------------

/// gosec `rangeResult`. The pool bookkeeping (`shared`, `acquireResult`,
/// `releaseResult`) has no counterpart: results are owned.
#[derive(Clone)]
struct RangeResult {
    min_value: u64,
    max_value: u64,
    min_value_set: bool,
    max_value_set: bool,
    explicit_positive_vals: Vec<u64>,
    explicit_negative_vals: Vec<i64>,
    is_range_check: bool,
}

impl RangeResult {
    /// gosec `rangeResult.Reset` — the wide `[MinInt64, MaxUint64]` window that
    /// `acquireResult` hands out.
    fn new() -> Self {
        Self {
            min_value: to_uint64(i64::MIN),
            max_value: u64::MAX,
            min_value_set: false,
            max_value_set: false,
            explicit_positive_vals: Vec::new(),
            explicit_negative_vals: Vec::new(),
            is_range_check: false,
        }
    }

    /// gosec `rangeResult.CopyFrom`.
    fn copy_from(&mut self, other: &RangeResult) {
        self.min_value = other.min_value;
        self.max_value = other.max_value;
        self.min_value_set = other.min_value_set;
        self.max_value_set = other.max_value_set;
        self.explicit_positive_vals.clear();
        self.explicit_positive_vals
            .extend_from_slice(&other.explicit_positive_vals);
        self.explicit_negative_vals.clear();
        self.explicit_negative_vals
            .extend_from_slice(&other.explicit_negative_vals);
        self.is_range_check = other.is_range_check;
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RangeCacheKey {
    block: BlockId,
    val: Value,
}

/// gosec `minBounds`.
fn min_bounds(a_val: u64, a_set: bool, b_val: u64, b_set: bool, unsigned: bool) -> u64 {
    if !a_set {
        return b_val;
    }
    if !b_set {
        return a_val;
    }
    if !unsigned {
        return if to_int64(a_val) < to_int64(b_val) {
            a_val
        } else {
            b_val
        };
    }
    if a_val < b_val {
        a_val
    } else {
        b_val
    }
}

/// gosec `maxBounds`.
fn max_bounds(a_val: u64, a_set: bool, b_val: u64, b_set: bool, unsigned: bool) -> u64 {
    if !a_set {
        return b_val;
    }
    if !b_set {
        return a_val;
    }
    if !unsigned {
        return if to_int64(a_val) > to_int64(b_val) {
            a_val
        } else {
            b_val
        };
    }
    if a_val > b_val {
        a_val
    } else {
        b_val
    }
}

/// gosec `constrainRange` (intersection).
fn constrain_range(res: &mut RangeResult, new_val: u64, is_min: bool, unsigned: bool) {
    if is_min {
        let tighter = !res.min_value_set
            || (unsigned && new_val > res.min_value)
            || (!unsigned && to_int64(new_val) > to_int64(res.min_value));
        if tighter {
            res.min_value = new_val;
            res.min_value_set = true;
            res.is_range_check = true;
        }
    } else {
        let tighter = !res.max_value_set
            || (unsigned && new_val < res.max_value)
            || (!unsigned && to_int64(new_val) < to_int64(res.max_value));
        if tighter {
            res.max_value = new_val;
            res.max_value_set = true;
            res.is_range_check = true;
        }
    }
}

/// gosec `expandRange` (union).
fn expand_range(res: &mut RangeResult, new_val: u64, is_min: bool, unsigned: bool) {
    if is_min {
        if !res.min_value_set {
            res.min_value = new_val;
            res.min_value_set = true;
        } else if (unsigned && new_val < res.min_value)
            || (!unsigned && to_int64(new_val) < to_int64(res.min_value))
        {
            res.min_value = new_val;
        }
    } else if !res.max_value_set {
        res.max_value = new_val;
        res.max_value_set = true;
    } else if (unsigned && new_val > res.max_value)
        || (!unsigned && to_int64(new_val) > to_int64(res.max_value))
    {
        res.max_value = new_val;
    }
}

/// gosec `updateExplicitValues`.
fn update_explicit_values(res: &mut RangeResult, val: i64) {
    if val < 0 {
        res.explicit_negative_vals.push(val);
    } else {
        res.explicit_positive_vals.push(val as u64);
    }
    res.min_value = to_uint64(val);
    res.max_value = to_uint64(val);
    res.min_value_set = true;
    res.max_value_set = true;
    res.is_range_check = true;
}

/// gosec `updateMinMaxForLessOrEqual`.
fn update_min_max_for_less_or_equal(
    res: &mut RangeResult,
    val: i64,
    op: Token,
    operands_flipped: bool,
    success_path: bool,
) {
    if success_path != operands_flipped {
        res.max_value = to_uint64(val);
        if (op == Token::LSS && success_path) || (op == Token::LEQ && !success_path) {
            res.max_value = res.max_value.wrapping_sub(1);
        }
        res.max_value_set = true;
        res.is_range_check = true;
    } else {
        res.min_value = to_uint64(val);
        if (op == Token::LEQ && !success_path) || (op == Token::LSS && success_path) {
            res.min_value = res.min_value.wrapping_add(1);
        }
        res.min_value_set = true;
        res.is_range_check = true;
    }
}

/// gosec `updateMinMaxForGreaterOrEqual`.
fn update_min_max_for_greater_or_equal(
    res: &mut RangeResult,
    val: i64,
    op: Token,
    operands_flipped: bool,
    success_path: bool,
) {
    if success_path != operands_flipped {
        res.min_value = to_uint64(val);
        if (op == Token::GTR && success_path) || (op == Token::GEQ && !success_path) {
            res.min_value = res.min_value.wrapping_add(1);
        }
        res.min_value_set = true;
        res.is_range_check = true;
    } else {
        res.max_value = to_uint64(val);
        if (op == Token::GEQ && !success_path) || (op == Token::GTR && success_path) {
            res.max_value = res.max_value.wrapping_sub(1);
        }
        res.max_value_set = true;
        res.is_range_check = true;
    }
}

/// gosec `signedMinForUnsignedSize`.
fn signed_min_for_unsigned_size(size: i32) -> i64 {
    if size >= 64 {
        i64::MIN
    } else {
        -(1i64 << (size - 1))
    }
}

/// gosec `signedMaxForUnsignedSize`.
fn signed_max_for_unsigned_size(size: i32) -> i64 {
    if size >= 64 {
        i64::MAX
    } else {
        (1i64 << (size - 1)) - 1
    }
}

/// gosec `RangeAnalyzer`, scoped to one function (upstream calls `state.Reset()`
/// per `SrcFunc`, which clears every cache in it).
struct RangeAnalyzer<'a> {
    prog: &'a Program,
    func: &'a Function,
    /// `Instruction.Block()`, which guff-ssa does not store on the instruction.
    block_of: HashMap<InstrId, BlockId>,
    cache: HashMap<RangeCacheKey, RangeResult>,
    depth: u32,
}

impl<'a> RangeAnalyzer<'a> {
    fn new(prog: &'a Program, func: &'a Function) -> Self {
        let mut block_of = HashMap::new();
        for (bid, block) in func.live_blocks() {
            for &iid in &block.instrs {
                block_of.insert(iid, bid);
            }
        }
        Self {
            prog,
            func,
            block_of,
            cache: HashMap::new(),
            depth: 0,
        }
    }

    fn instr_block(&self, iid: InstrId) -> Option<BlockId> {
        self.block_of.get(&iid).copied()
    }

    fn is_uint(&self, v: Value) -> bool {
        is_uint(self.prog, self.func, v)
    }

    /// gosec `RangeAnalyzer.IsReachable`.
    fn is_reachable(&self, start: BlockId, target: BlockId, exclude: Option<BlockId>) -> bool {
        if start == target {
            return true;
        }
        let func = self.func;
        let mut seen: HashSet<BlockId> = HashSet::new();
        if let Some(ex) = exclude {
            seen.insert(ex);
        }
        let mut stack = vec![start];
        while let Some(curr) = stack.pop() {
            if curr == target {
                return true;
            }
            if !seen.insert(curr) {
                continue;
            }
            for &succ in &func.blocks.get(curr).succs {
                if !seen.contains(&succ) {
                    stack.push(succ);
                }
            }
        }
        false
    }

    /// gosec `RangeAnalyzer.ResolveRange`.
    fn resolve_range(&mut self, v: Value, block: BlockId) -> RangeResult {
        let key = RangeCacheKey { block, val: v };
        if let Some(hit) = self.cache.get(&key) {
            return hit.clone();
        }
        let func = self.func;

        let is_src_unsigned = self.is_uint(v);
        let mut result = RangeResult::new();
        if is_src_unsigned {
            result.min_value = 0;
        } else {
            result.max_value = MAX_INT64_U;
        }

        // An IndexAddr's own range is the range of its index.
        let index_of = match v {
            Value::Instr(iid) => match func.instrs.get(iid) {
                InstrData::IndexAddr(ia) => Some((ia.index, iid)),
                _ => None,
            },
            _ => None,
        };
        if let Some((index, iid)) = index_of {
            let idx_block = self.instr_block(iid).unwrap_or(block);
            let res = self.resolve_range(index, idx_block);
            if res.is_range_check && res.min_value_set && res.max_value_set {
                result.min_value = max_bounds(
                    result.min_value,
                    result.min_value_set,
                    res.min_value,
                    res.min_value_set,
                    is_src_unsigned,
                );
                result.max_value = min_bounds(
                    result.max_value,
                    result.max_value_set,
                    res.max_value,
                    res.max_value_set,
                    is_src_unsigned,
                );
                result.min_value_set = true;
                result.max_value_set = true;
                result.is_range_check = true;
            }
        }

        if self.depth > MAX_DEPTH {
            self.cache.insert(key, result.clone());
            return result;
        }

        self.depth += 1;

        if self.is_non_negative(v) {
            result.min_value = 0;
            result.min_value_set = true;
            result.is_range_check = true;
        }

        let def_range = self.compute_range(v, block);
        if def_range.is_range_check || def_range.min_value_set || def_range.max_value_set {
            result.is_range_check = true;
            if def_range.min_value_set {
                result.min_value = max_bounds(
                    result.min_value,
                    result.min_value_set,
                    def_range.min_value,
                    def_range.min_value_set,
                    is_src_unsigned,
                );
                result.min_value_set = true;
            }
            if def_range.max_value_set {
                result.max_value = min_bounds(
                    result.max_value,
                    result.max_value_set,
                    def_range.max_value,
                    def_range.max_value_set,
                    is_src_unsigned,
                );
                result.max_value_set = true;
            }
        }

        // Constraints from the `if`s that dominate `block`.
        let mut curr_dom = func.blocks.get(block).idom();
        while let Some(dom) = curr_dom {
            let last = func.blocks.get(dom).instrs.last().copied();
            let is_if = last.is_some_and(|l| matches!(func.instrs.get(l), InstrData::If(_)));
            if is_if {
                let if_id = last.unwrap();
                let succs = func.blocks.get(dom).succs.clone();
                let mut final_res: Option<RangeResult> = None;
                let mut match_count = 0;
                for (i, &succ) in succs.iter().enumerate() {
                    if !self.is_reachable(succ, block, Some(dom)) {
                        continue;
                    }
                    match_count += 1;
                    let res_if = self.result_range_for_if_edge(if_id, i == 0, v);
                    if match_count == 1 {
                        final_res = Some(res_if);
                    } else {
                        final_res = None;
                    }
                }
                if match_count == 1 {
                    if let Some(res_if) = final_res {
                        if res_if.min_value_set {
                            result.min_value = max_bounds(
                                result.min_value,
                                result.min_value_set,
                                res_if.min_value,
                                res_if.min_value_set,
                                is_src_unsigned,
                            );
                            result.min_value_set = true;
                        }
                        if res_if.max_value_set {
                            result.max_value = min_bounds(
                                result.max_value,
                                result.max_value_set,
                                res_if.max_value,
                                res_if.max_value_set,
                                is_src_unsigned,
                            );
                            result.max_value_set = true;
                        }
                        if res_if.is_range_check {
                            result.is_range_check = true;
                        }
                    }
                }
            }
            curr_dom = func.blocks.get(dom).idom();
        }

        self.depth -= 1;
        self.cache.insert(key, result.clone());
        result
    }

    /// gosec `RangeAnalyzer.getResultRangeForIfEdge`.
    fn result_range_for_if_edge(
        &mut self,
        if_id: InstrId,
        is_true: bool,
        v: Value,
    ) -> RangeResult {
        let (prog, func) = (self.prog, self.func);
        let mut res = RangeResult::new();
        let InstrData::If(iff) = func.instrs.get(if_id) else {
            return res;
        };
        let cond = iff.cond;
        let binop = match cond {
            Value::Instr(cid) if matches!(func.instrs.get(cid), InstrData::BinOp(_)) => Some(cid),
            _ => None,
        };
        if let Some(binop_id) = binop {
            if is_range_check_cond(prog, func, cond, v) {
                self.update_result_from_binop_for_value(&mut res, binop_id, v, is_true);
            }
        }
        res
    }

    /// gosec `RangeAnalyzer.updateResultFromBinOpForValue`.
    fn update_result_from_binop_for_value(
        &mut self,
        result: &mut RangeResult,
        binop_id: InstrId,
        v: Value,
        success_path: bool,
    ) {
        let (prog, func) = (self.prog, self.func);
        let InstrData::BinOp(binop) = func.instrs.get(binop_id) else {
            return;
        };
        let (bx, by, bop) = (binop.x, binop.y, binop.op);

        let mut operands_flipped = false;
        let (compare_val, mut op) = real_value_from_operation(prog, func, v);
        let mut inverse_op = OperationInfo::NONE;

        // "side is `compareVal` with one operation applied", i.e. upstream's
        // `if rVal, rOp := getRealValueFromOperation(side); rVal == compareVal`.
        let peeled = |side: Value| -> Option<OperationInfo> {
            let (rval, rop) = real_value_from_operation(prog, func, side);
            (rval == compare_val).then_some(rop)
        };

        // gosec's cascade: an exact match on either operand (which drops the
        // operation on `v`), then a related-value match, then a match on the
        // operand *under* an operation — whose inverse is applied to the limit.
        let match_side = if is_equivalent(prog, func, bx, v) {
            op = OperationInfo::NONE;
            by
        } else if is_equivalent(prog, func, by, v) {
            operands_flipped = true;
            op = OperationInfo::NONE;
            bx
        } else if is_same_or_related(prog, func, bx, compare_val) {
            inverse_op = peeled(bx).unwrap_or(OperationInfo::NONE);
            by
        } else if let Some(rop) = peeled(bx) {
            inverse_op = rop;
            by
        } else if is_same_or_related(prog, func, by, compare_val) {
            operands_flipped = true;
            inverse_op = peeled(by).unwrap_or(OperationInfo::NONE);
            bx
        } else if let Some(rop) = peeled(by) {
            operands_flipped = true;
            inverse_op = rop;
            bx
        } else {
            return;
        };

        let Some(mut val) = constant_int64(prog, func, match_side) else {
            return;
        };

        // Undo the operation that stands between `compareVal` and `v`.
        if inverse_op.op != Op::None {
            let extra = inverse_op.extra;
            match inverse_op.op {
                Op::Shl => {
                    if let Some(shift) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if shift >= 0 {
                            val = shr_i64(val, shift as u64);
                        }
                    }
                }
                Op::Add => {
                    if let Some(add) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        val = val.wrapping_sub(add);
                    }
                }
                Op::Sub => {
                    if let Some(sub) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if inverse_op.flipped {
                            val = sub.wrapping_sub(val);
                            operands_flipped = !operands_flipped;
                        } else {
                            val = val.wrapping_add(sub);
                        }
                    }
                }
                Op::Neg => {
                    val = val.wrapping_neg();
                    operands_flipped = !operands_flipped;
                }
                Op::Shr => {
                    if let Some(shift) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if shift >= 0 {
                            val = shl_i64(val, shift as u64);
                        }
                    }
                }
                Op::Mul => {
                    if let Some(mul) = extra.and_then(|e| constant_uint64(prog, e)) {
                        if mul > 0 {
                            val = to_int64(to_uint64(val) / mul);
                        }
                    }
                }
                Op::Quo => {
                    if let Some(quo) = extra.and_then(|e| constant_uint64(prog, e)) {
                        if quo > 0 {
                            if inverse_op.flipped {
                                if val != 0 {
                                    val = to_int64(quo / to_uint64(val));
                                }
                                operands_flipped = !operands_flipped;
                            } else {
                                val = to_int64(to_uint64(val).wrapping_mul(quo));
                            }
                        }
                    }
                }
                Op::None | Op::Field | Op::Alloc => {}
            }
        }

        // Apply the operation between `v` and the compared value.
        if op.op != Op::None {
            let extra = op.extra;
            match op.op {
                Op::Shl => {
                    if let Some(shift) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if shift >= 0 {
                            val = shl_i64(val, shift as u64);
                        }
                    }
                }
                Op::Add => {
                    if let Some(add) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        val = val.wrapping_add(add);
                    }
                }
                Op::Sub => {
                    if let Some(sub) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if op.flipped {
                            val = sub.wrapping_sub(val);
                            operands_flipped = !operands_flipped;
                        } else {
                            val = val.wrapping_sub(sub);
                        }
                    }
                }
                Op::Shr => {
                    if let Some(shift) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if shift >= 0 {
                            val = shr_i64(val, shift as u64);
                        }
                    }
                }
                Op::Mul => {
                    if self.is_uint(v) {
                        if let Some(mul) = extra.and_then(|e| constant_uint64(prog, e)) {
                            if mul != 0 {
                                let (hi, lo) = mul64(to_uint64(val), mul);
                                if hi != 0 {
                                    return;
                                }
                                val = to_int64(lo);
                            }
                        }
                    } else if let Some(mul) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if mul != 0 {
                            if mul > 0 {
                                if val >= 0 {
                                    let (hi, lo) = mul64(to_uint64(val), to_uint64(mul));
                                    if hi != 0 {
                                        return;
                                    }
                                    val = to_int64(lo);
                                } else {
                                    if val < i64::MIN.wrapping_div(mul) {
                                        return;
                                    }
                                    val = val.wrapping_mul(mul);
                                }
                            } else {
                                val = val.wrapping_mul(mul);
                                operands_flipped = !operands_flipped;
                            }
                        }
                    }
                }
                Op::Quo => {
                    if let Some(quo) = extra.and_then(|e| constant_int64(prog, func, e)) {
                        if quo > 0 {
                            if op.flipped {
                                if val != 0 {
                                    val = quo.wrapping_div(val);
                                }
                                operands_flipped = !operands_flipped;
                            } else {
                                val = val.wrapping_div(quo);
                            }
                        }
                    }
                }
                Op::Neg => {
                    val = val.wrapping_neg();
                    operands_flipped = !operands_flipped;
                }
                Op::None | Op::Field | Op::Alloc => {}
            }
        }

        match bop {
            Token::LEQ | Token::LSS => {
                update_min_max_for_less_or_equal(result, val, bop, operands_flipped, success_path);
            }
            Token::GEQ | Token::GTR => {
                update_min_max_for_greater_or_equal(
                    result,
                    val,
                    bop,
                    operands_flipped,
                    success_path,
                );
            }
            Token::EQL => {
                if success_path {
                    update_explicit_values(result, val);
                }
            }
            Token::NEQ => {
                if !success_path {
                    update_explicit_values(result, val);
                }
            }
            _ => {}
        }
    }

    /// gosec `RangeAnalyzer.IsNonNegative`.
    fn is_non_negative(&mut self, v: Value) -> bool {
        let mut seen = HashSet::new();
        self.is_non_negative_recursive(v, &mut seen)
    }

    /// gosec `RangeAnalyzer.isNonNegativeRecursive`.
    fn is_non_negative_recursive(&mut self, v: Value, seen: &mut HashSet<Value>) -> bool {
        if !seen.insert(v) {
            return true; // cycle: assume non-negative, as upstream does
        }
        let (prog, func) = (self.prog, self.func);
        if self.is_uint(v) {
            return true;
        }
        if is_element_of_string_rune_slice(prog, func, v) {
            return true;
        }
        let (v, info) = real_value_from_operation(prog, func, v);
        if info.op == Op::Neg || info.op == Op::Sub {
            return false;
        }
        let Value::Instr(iid) = v else {
            if let Value::Const(_) = v {
                if let Some(val) = constant_int64(prog, func, v) {
                    return val >= 0;
                }
            }
            return false;
        };
        match func.instrs.get(iid) {
            InstrData::Extract(ex) => {
                // Only a range-loop index is guaranteed non-negative.
                if let Value::Instr(tid) = ex.tuple {
                    if matches!(func.instrs.get(tid), InstrData::Next(_)) && ex.index == 0 {
                        return true;
                    }
                }
                false
            }
            InstrData::Call(c) => {
                if let Value::Builtin(bid) = c.call.value {
                    let args = c.call.args.clone();
                    match prog.builtins.get(bid).name.as_str() {
                        "len" | "cap" => return true,
                        "min" => {
                            for arg in &args {
                                if !self.is_non_negative_recursive(*arg, seen) {
                                    return false;
                                }
                            }
                            return !args.is_empty();
                        }
                        "max" => {
                            for arg in &args {
                                if self.is_non_negative_recursive(*arg, seen) {
                                    return true;
                                }
                            }
                            return false;
                        }
                        _ => {}
                    }
                }
                if let Some(callee) = static_callee(&c.call) {
                    let name = &prog.functions.get(callee).name;
                    if name.contains("UnixMilli")
                        || name.contains("UnixMicro")
                        || name.contains("UnixNano")
                    {
                        return true;
                    }
                }
                false
            }
            InstrData::BinOp(b) => {
                let (bx, by, bop) = (b.x, b.y, b.op);
                match bop {
                    Token::ADD | Token::MUL | Token::QUO => {
                        self.is_non_negative_recursive(bx, seen)
                            && self.is_non_negative_recursive(by, seen)
                    }
                    Token::REM | Token::AND | Token::SHR => {
                        self.is_non_negative_recursive(bx, seen)
                    }
                    _ => false,
                }
            }
            InstrData::Phi(phi) => {
                let edges = phi.edges.clone();
                for edge in edges.into_iter().flatten() {
                    if !self.is_non_negative_recursive(edge, seen) {
                        // `-1` sentinels (the "not found" idiom) do not make the
                        // phi negative upstream.
                        if let Some(-1) = constant_int64(prog, func, edge) {
                            continue;
                        }
                        return false;
                    }
                }
                true
            }
            InstrData::Convert(c) => self.is_uint(c.x),
            _ => false,
        }
    }

    /// gosec `RangeAnalyzer.ComputeRange`.
    fn compute_range(&mut self, v: Value, block: BlockId) -> RangeResult {
        let (prog, func) = (self.prog, self.func);
        let mut res = RangeResult::new();
        let is_src_unsigned = self.is_uint(v);

        let Value::Instr(iid) = v else {
            if let Value::Const(_) = v {
                if let Some(val) = constant_int64(prog, func, v) {
                    res.min_value = to_uint64(val);
                    res.max_value = to_uint64(val);
                    res.min_value_set = true;
                    res.max_value_set = true;
                    res.is_range_check = true;
                }
            }
            return res;
        };

        match func.instrs.get(iid) {
            InstrData::BinOp(b) => {
                let (bx, by, bop) = (b.x, b.y, b.op);
                match bop {
                    Token::ADD => self.range_add(&mut res, bx, by, block, is_src_unsigned),
                    Token::SUB => self.range_sub(&mut res, bx, by, block, is_src_unsigned),
                    Token::MUL => self.range_mul(&mut res, bx, by, block),
                    Token::SHL => {
                        if let Some(val) = constant_int64(prog, func, by) {
                            if val >= 0 {
                                let sub = self.resolve_range(bx, block);
                                if sub.min_value_set {
                                    let new_min = shl_u64(sub.min_value, val as u64);
                                    if shr_u64(new_min, val as u64) == sub.min_value {
                                        constrain_range(&mut res, new_min, true, is_src_unsigned);
                                    }
                                }
                                if sub.max_value_set {
                                    let new_max = shl_u64(sub.max_value, val as u64);
                                    if shr_u64(new_max, val as u64) == sub.max_value {
                                        constrain_range(&mut res, new_max, false, is_src_unsigned);
                                    }
                                }
                            }
                        }
                    }
                    Token::SHR => {
                        if let Some(val) = constant_int64(prog, func, by) {
                            if val >= 0 {
                                let sub = self.resolve_range(bx, block);
                                if sub.min_value_set {
                                    constrain_range(
                                        &mut res,
                                        shr_u64(sub.min_value, val as u64),
                                        true,
                                        is_src_unsigned,
                                    );
                                }
                                if sub.max_value_set {
                                    constrain_range(
                                        &mut res,
                                        shr_u64(sub.max_value, val as u64),
                                        false,
                                        is_src_unsigned,
                                    );
                                } else if let Some(src_int) =
                                    value_type(prog, func, bx).and_then(|t| int_type_info(prog, t))
                                {
                                    // The type's own upper bound still shifts down.
                                    res.max_value = shr_u64(src_int.max, val as u64);
                                    res.max_value_set = true;
                                    res.is_range_check = true;
                                }
                            }
                        }
                    }
                    Token::QUO => {
                        if let Some(val) = constant_int64(prog, func, by) {
                            if val != 0 {
                                let sub = self.resolve_range(bx, block);
                                if val > 0 {
                                    if sub.min_value_set && sub.is_range_check {
                                        constrain_range(
                                            &mut res,
                                            to_uint64(to_int64(sub.min_value).wrapping_div(val)),
                                            true,
                                            is_src_unsigned,
                                        );
                                    }
                                    if sub.max_value_set && sub.is_range_check {
                                        constrain_range(
                                            &mut res,
                                            to_uint64(to_int64(sub.max_value).wrapping_div(val)),
                                            false,
                                            is_src_unsigned,
                                        );
                                    }
                                } else {
                                    if sub.max_value_set && sub.is_range_check {
                                        constrain_range(
                                            &mut res,
                                            to_uint64(to_int64(sub.max_value).wrapping_div(val)),
                                            true,
                                            is_src_unsigned,
                                        );
                                    }
                                    if sub.min_value_set && sub.is_range_check {
                                        constrain_range(
                                            &mut res,
                                            to_uint64(to_int64(sub.min_value).wrapping_div(val)),
                                            false,
                                            is_src_unsigned,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    Token::REM => {
                        if let Some(val) = constant_int64(prog, func, by) {
                            if val > 0 {
                                res.min_value = to_uint64(val.wrapping_sub(1).wrapping_neg());
                                res.max_value = to_uint64(val.wrapping_sub(1));
                                res.min_value_set = true;
                                res.max_value_set = true;
                                res.is_range_check = true;
                                let sub = self.resolve_range(bx, block);
                                if (sub.min_value_set && to_int64(sub.min_value) >= 0)
                                    || self.is_non_negative(bx)
                                {
                                    res.min_value = 0;
                                }
                            }
                        }
                    }
                    Token::AND => {
                        let mask = constant_int64(prog, func, by)
                            .filter(|&n| n >= 0)
                            .or_else(|| constant_int64(prog, func, bx).filter(|&n| n >= 0));
                        if let Some(val) = mask {
                            res.min_value = 0;
                            res.max_value = val as u64;
                            res.min_value_set = true;
                            res.max_value_set = true;
                            res.is_range_check = true;
                        }
                    }
                    _ => {}
                }
            }
            InstrData::UnOp(u) => {
                let (ux, uop) = (u.x, u.op);
                match uop {
                    // Load.
                    Token::MUL => {
                        if let Value::Instr(xid) = ux {
                            match func.instrs.get(xid) {
                                InstrData::Alloc(_) => {
                                    return self.resolve_alloc_range(ux, block, iid);
                                }
                                // `*(&data[i])` is the element, whose range has
                                // nothing to do with the index's.
                                InstrData::IndexAddr(_) => return res,
                                _ => {}
                            }
                        }
                        let sub = self.resolve_range(ux, block);
                        res.copy_from(&sub);
                    }
                    // Negation.
                    Token::SUB => {
                        let sub = self.resolve_range(ux, block);
                        let src = value_type(prog, func, ux).and_then(|t| int_type_info(prog, t));
                        if src.is_some_and(|s| s.signed)
                            && sub.min_value_set
                            && sub.max_value_set
                        {
                            let old_min = to_int64(sub.min_value);
                            let old_max = to_int64(sub.max_value);
                            res.min_value = to_uint64(old_max.wrapping_neg());
                            res.max_value = to_uint64(old_min.wrapping_neg());
                            res.min_value_set = true;
                            res.max_value_set = true;
                            res.is_range_check = sub.is_range_check;
                        }
                    }
                    _ => {}
                }
            }
            InstrData::Convert(c) => {
                let (cx, ctyp) = (c.x, c.typ);
                let sub = self.resolve_range(cx, block);
                if sub.min_value_set && sub.max_value_set {
                    let Some(src_int) = value_type(prog, func, cx).and_then(|t| int_type_info(prog, t)) else {
                        return res;
                    };
                    let Some(dst_int) = int_type_info(prog, ctyp) else {
                        return res;
                    };

                    let convert_bound = |val: u64| -> u64 {
                        match dst_int.size {
                            8 => {
                                let mut n = val & 0xFF;
                                if dst_int.signed && n & 0x80 != 0 {
                                    n |= 0xFFFF_FFFF_FFFF_FF00;
                                }
                                n
                            }
                            16 => {
                                let mut n = val & 0xFFFF;
                                if dst_int.signed && n & 0x8000 != 0 {
                                    n |= 0xFFFF_FFFF_FFFF_0000;
                                }
                                n
                            }
                            32 => {
                                let mut n = val & 0xFFFF_FFFF;
                                if dst_int.signed && n & 0x8000_0000 != 0 {
                                    n |= 0xFFFF_FFFF_0000_0000;
                                }
                                n
                            }
                            _ => val,
                        }
                    };

                    let new_min = convert_bound(sub.min_value);
                    let new_max = convert_bound(sub.max_value);

                    let fits = |val: u64| -> bool {
                        if dst_int.signed {
                            if src_int.signed {
                                let v = to_int64(val);
                                v >= dst_int.min
                                    && (dst_int.size == 64 || v <= to_int64(dst_int.max))
                            } else {
                                val <= dst_int.max
                            }
                        } else if src_int.signed {
                            let v = to_int64(val);
                            v >= 0 && (v as u64) <= dst_int.max
                        } else {
                            val <= dst_int.max
                        }
                    };

                    let ordered = if dst_int.signed {
                        to_int64(new_min) <= to_int64(new_max)
                    } else {
                        new_min <= new_max
                    };
                    if ordered && fits(sub.min_value) && fits(sub.max_value) {
                        res.min_value = new_min;
                        res.max_value = new_max;
                        res.min_value_set = true;
                        res.max_value_set = true;
                        res.is_range_check = true;
                    }
                }
            }
            InstrData::Call(c) => {
                if let Value::Builtin(bid) = c.call.value {
                    let args = c.call.args.clone();
                    let name = prog.builtins.get(bid).name.clone();
                    if (name == "min" || name == "max") && !args.is_empty() {
                        for (i, arg) in args.iter().enumerate() {
                            let arg_res = self.resolve_range(*arg, block);
                            if i == 0 {
                                res.copy_from(&arg_res);
                            } else if name == "min" {
                                res.min_value = min_bounds(
                                    res.min_value,
                                    res.min_value_set,
                                    arg_res.min_value,
                                    arg_res.min_value_set,
                                    is_src_unsigned,
                                );
                                res.min_value_set = res.min_value_set && arg_res.min_value_set;
                                res.max_value = min_bounds(
                                    res.max_value,
                                    res.max_value_set,
                                    arg_res.max_value,
                                    arg_res.max_value_set,
                                    is_src_unsigned,
                                );
                                res.max_value_set = res.max_value_set && arg_res.max_value_set;
                            } else {
                                res.min_value = max_bounds(
                                    res.min_value,
                                    res.min_value_set,
                                    arg_res.min_value,
                                    arg_res.min_value_set,
                                    is_src_unsigned,
                                );
                                res.min_value_set = res.min_value_set && arg_res.min_value_set;
                                res.max_value = max_bounds(
                                    res.max_value,
                                    res.max_value_set,
                                    arg_res.max_value,
                                    arg_res.max_value_set,
                                    is_src_unsigned,
                                );
                                res.max_value_set = res.max_value_set && arg_res.max_value_set;
                            }
                        }
                        res.is_range_check = true;
                    }
                }
            }
            InstrData::Phi(phi) => {
                let edges = phi.edges.clone();
                // The lifter fills every edge (an unstored cell gets the zero
                // constant, as in go/ssa), so `None` only ever means an edge
                // that no longer exists.
                for edge in edges.into_iter().flatten() {
                    let sub = self.resolve_range(edge, block);
                    if sub.min_value_set {
                        expand_range(&mut res, sub.min_value, true, is_src_unsigned);
                    }
                    if sub.max_value_set {
                        expand_range(&mut res, sub.max_value, false, is_src_unsigned);
                    }
                }
            }
            InstrData::Extract(ex) => {
                if ex.index == 0 {
                    if let Value::Instr(tid) = ex.tuple {
                        if let InstrData::Call(call) = func.instrs.get(tid) {
                            if let Some(callee) = static_callee(&call.call) {
                                let name = prog.functions.get(callee).name.clone();
                                let args = call.call.args.clone();
                                if args.len() == 3 {
                                    if name == "ParseInt" {
                                        if let Some(bit_size) =
                                            constant_int64(prog, func, args[2])
                                        {
                                            let shift = bit_size.wrapping_sub(1);
                                            if shift >= 0 && shift < 64 {
                                                res.min_value =
                                                    to_uint64(shl_i64(-1, shift as u64));
                                                res.max_value = to_uint64(
                                                    shl_i64(1, shift as u64).wrapping_sub(1),
                                                );
                                                res.min_value_set = true;
                                                res.max_value_set = true;
                                                res.is_range_check = true;
                                            }
                                        }
                                    } else if name == "ParseUint" {
                                        if let Some(bit_size) =
                                            constant_int64(prog, func, args[2])
                                        {
                                            if bit_size == 64 {
                                                res.max_value = u64::MAX;
                                            } else if bit_size > 0 && bit_size < 64 {
                                                res.max_value = shl_u64(1, bit_size as u64) - 1;
                                            }
                                            res.min_value = 0;
                                            res.min_value_set = true;
                                            res.max_value_set = true;
                                            res.is_range_check = true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        res
    }

    /// `ComputeRange`'s `token.ADD` arm.
    fn range_add(
        &mut self,
        res: &mut RangeResult,
        bx: Value,
        by: Value,
        block: BlockId,
        is_src_unsigned: bool,
    ) {
        let (prog, func) = (self.prog, self.func);
        if let Some(val) = constant_int64(prog, func, by) {
            let sub = self.resolve_range(bx, block);
            if sub.is_range_check {
                if sub.min_value_set {
                    res.min_value = to_uint64(to_int64(sub.min_value).wrapping_add(val));
                    res.min_value_set = true;
                }
                if sub.max_value_set {
                    res.max_value = to_uint64(to_int64(sub.max_value).wrapping_add(val));
                    res.max_value_set = true;
                }
                if res.min_value_set || res.max_value_set {
                    res.is_range_check = true;
                }
            }
            return;
        }
        if let Some(val) = constant_int64(prog, func, bx) {
            let sub = self.resolve_range(by, block);
            if sub.is_range_check {
                if sub.min_value_set {
                    res.min_value = to_uint64(val.wrapping_add(to_int64(sub.min_value)));
                    res.min_value_set = true;
                }
                if sub.max_value_set {
                    res.max_value = to_uint64(val.wrapping_add(to_int64(sub.max_value)));
                    res.max_value_set = true;
                }
                if res.min_value_set || res.max_value_set {
                    res.is_range_check = true;
                }
            }
            return;
        }
        let sub_x = self.resolve_range(bx, block);
        let sub_y = self.resolve_range(by, block);
        if sub_x.is_range_check || sub_y.is_range_check {
            if sub_x.min_value_set && sub_y.min_value_set {
                constrain_range(
                    res,
                    to_uint64(to_int64(sub_x.min_value).wrapping_add(to_int64(sub_y.min_value))),
                    true,
                    is_src_unsigned,
                );
            }
            if sub_x.max_value_set && sub_y.max_value_set {
                constrain_range(
                    res,
                    to_uint64(to_int64(sub_x.max_value).wrapping_add(to_int64(sub_y.max_value))),
                    false,
                    is_src_unsigned,
                );
            }
            if res.min_value_set || res.max_value_set {
                res.is_range_check = true;
            }
        } else if sub_x.min_value_set
            && sub_x.max_value_set
            && sub_y.min_value_set
            && sub_y.max_value_set
        {
            constrain_range(
                res,
                to_uint64(to_int64(sub_x.min_value).wrapping_add(to_int64(sub_y.min_value))),
                true,
                is_src_unsigned,
            );
            constrain_range(
                res,
                to_uint64(to_int64(sub_x.max_value).wrapping_add(to_int64(sub_y.max_value))),
                false,
                is_src_unsigned,
            );
            res.is_range_check = true;
        }
    }

    /// `ComputeRange`'s `token.SUB` arm.
    fn range_sub(
        &mut self,
        res: &mut RangeResult,
        bx: Value,
        by: Value,
        block: BlockId,
        is_src_unsigned: bool,
    ) {
        let (prog, func) = (self.prog, self.func);
        if let Some(val) = constant_int64(prog, func, by) {
            let sub = self.resolve_range(bx, block);
            if sub.is_range_check {
                if sub.min_value_set {
                    constrain_range(
                        res,
                        to_uint64(to_int64(sub.min_value).wrapping_sub(val)),
                        true,
                        is_src_unsigned,
                    );
                }
                if sub.max_value_set {
                    constrain_range(
                        res,
                        to_uint64(to_int64(sub.max_value).wrapping_sub(val)),
                        false,
                        is_src_unsigned,
                    );
                }
            }
            return;
        }
        if let Some(val) = constant_int64(prog, func, bx) {
            let sub = self.resolve_range(by, block);
            if sub.is_range_check {
                if sub.max_value_set {
                    constrain_range(
                        res,
                        to_uint64(val.wrapping_sub(to_int64(sub.max_value))),
                        true,
                        is_src_unsigned,
                    );
                }
                if sub.min_value_set {
                    constrain_range(
                        res,
                        to_uint64(val.wrapping_sub(to_int64(sub.min_value))),
                        false,
                        is_src_unsigned,
                    );
                }
            }
            return;
        }
        let sub_x = self.resolve_range(bx, block);
        let sub_y = self.resolve_range(by, block);
        if sub_x.is_range_check || sub_y.is_range_check {
            if sub_x.min_value_set && sub_y.max_value_set {
                constrain_range(
                    res,
                    to_uint64(to_int64(sub_x.min_value).wrapping_sub(to_int64(sub_y.max_value))),
                    true,
                    is_src_unsigned,
                );
            }
            if sub_x.max_value_set && sub_y.min_value_set {
                constrain_range(
                    res,
                    to_uint64(to_int64(sub_x.max_value).wrapping_sub(to_int64(sub_y.min_value))),
                    false,
                    is_src_unsigned,
                );
            }
            if res.min_value_set || res.max_value_set {
                res.is_range_check = true;
            }
        } else if sub_x.min_value_set
            && sub_x.max_value_set
            && sub_y.min_value_set
            && sub_y.max_value_set
        {
            constrain_range(
                res,
                to_uint64(to_int64(sub_x.min_value).wrapping_sub(to_int64(sub_y.max_value))),
                true,
                is_src_unsigned,
            );
            constrain_range(
                res,
                to_uint64(to_int64(sub_x.max_value).wrapping_sub(to_int64(sub_y.min_value))),
                false,
                is_src_unsigned,
            );
            res.is_range_check = true;
        }
    }

    /// `ComputeRange`'s `token.MUL` arm.
    fn range_mul(&mut self, res: &mut RangeResult, bx: Value, by: Value, block: BlockId) {
        let (prog, func) = (self.prog, self.func);
        let mut val = constant_int64(prog, func, by);
        if val.is_none() {
            val = constant_int64(prog, func, bx);
        }
        let Some(val) = val.filter(|&v| v != 0) else {
            return;
        };
        let sub = if matches!(by, Value::Const(_)) {
            self.resolve_range(bx, block)
        } else {
            self.resolve_range(by, block)
        };
        if !(sub.is_range_check || sub.min_value_set || sub.max_value_set) {
            return;
        }
        let src_int = value_type(prog, func, bx).and_then(|t| int_type_info(prog, t));
        let signed = src_int.is_some_and(|s| s.signed);
        if signed {
            if sub.min_value_set && sub.max_value_set {
                let v1 = to_int64(sub.min_value).wrapping_mul(val);
                let v2 = to_int64(sub.max_value).wrapping_mul(val);
                let (v_min, v_max) = if v1 > v2 { (v2, v1) } else { (v1, v2) };
                if v1.wrapping_div(val) == to_int64(sub.min_value) {
                    constrain_range(res, to_uint64(v_min), true, false);
                    constrain_range(res, to_uint64(v_max), false, false);
                    res.is_range_check = sub.is_range_check;
                }
            }
        } else {
            let u_val = to_uint64(val);
            if sub.max_value_set {
                let (hi, _) = mul64(sub.max_value, u_val);
                if hi == 0 {
                    if sub.min_value_set && sub.is_range_check {
                        constrain_range(res, sub.min_value.wrapping_mul(u_val), true, true);
                    }
                    if sub.max_value_set && sub.is_range_check {
                        constrain_range(res, sub.max_value.wrapping_mul(u_val), false, true);
                    }
                }
            }
        }
    }

    /// gosec `RangeAnalyzer.resolveAllocRange`.
    fn resolve_alloc_range(
        &mut self,
        alloc: Value,
        block: BlockId,
        load_instr: InstrId,
    ) -> RangeResult {
        let (prog, func) = (self.prog, self.func);
        let mut res = RangeResult::new();

        // 1. Nearest store in the same block, scanning back from the load.
        if self.instr_block(load_instr) == Some(block) {
            let instrs = func.blocks.get(block).instrs.clone();
            let start = instrs.iter().rposition(|&i| i == load_instr);
            let mut nearest: Option<Value> = None;
            if let Some(start) = start {
                for &iid in instrs[..start].iter().rev() {
                    if let InstrData::Store(st) = func.instrs.get(iid) {
                        if st.addr == alloc {
                            nearest = Some(st.val);
                            break;
                        }
                    }
                }
            }
            if let Some(val) = nearest {
                let store_res = self.resolve_range(val, block);
                res.copy_from(&store_res);
                res.is_range_check = store_res.is_range_check;
                return res;
            }
        }

        // 2. Union of every store to the cell.
        let stores: Vec<Value> = referrers(func, alloc)
            .iter()
            .filter_map(|&rid| match func.instrs.get(rid) {
                InstrData::Store(st) if st.addr == alloc => Some(st.val),
                _ => None,
            })
            .collect();

        let elem_unsigned = value_type(prog, func, alloc)
            .map(|t| t.underlying(&prog.type_arena))
            .and_then(|u| match prog.type_arena.get(u) {
                TypeData::Pointer(p) => Some(p.elem()),
                _ => None,
            })
            .and_then(|elem| basic_of(prog, elem))
            .is_some_and(|b| b.info().contains(guff_types::IS_UNSIGNED));

        let mut first = true;
        for val in stores {
            let store_res = self.resolve_range(val, block);
            if first {
                res.copy_from(&store_res);
                if store_res.min_value_set || store_res.max_value_set {
                    first = false;
                }
            } else {
                if store_res.min_value_set {
                    expand_range(&mut res, store_res.min_value, true, elem_unsigned);
                } else {
                    res.min_value_set = false;
                }
                if store_res.max_value_set {
                    expand_range(&mut res, store_res.max_value, false, elem_unsigned);
                } else {
                    res.max_value_set = false;
                }
                res.is_range_check = res.is_range_check || store_res.is_range_check;
            }
        }

        if first {
            // No store reached it: the cell holds its zero value.
            res.min_value = 0;
            res.max_value = 0;
            res.max_value_set = true;
        }

        res
    }
}

/// gosec `isElementOfStringRuneSlice`: an element loaded out of a `[]rune` that
/// came from a string is a valid code point, hence non-negative.
fn is_element_of_string_rune_slice(prog: &Program, func: &Function, v: Value) -> bool {
    let Value::Instr(iid) = v else {
        return false;
    };
    let InstrData::UnOp(u) = func.instrs.get(iid) else {
        return false;
    };
    if u.op != Token::MUL {
        return false;
    }
    let Value::Instr(idx_id) = u.x else {
        return false;
    };
    let InstrData::IndexAddr(idx) = func.instrs.get(idx_id) else {
        return false;
    };
    is_string_to_rune_conversion(prog, func, idx.x)
}

/// gosec `isStringToRuneConversion`.
fn is_string_to_rune_conversion(prog: &Program, func: &Function, v: Value) -> bool {
    let Value::Instr(iid) = v else {
        return false;
    };
    let InstrData::Convert(c) = func.instrs.get(iid) else {
        return false;
    };
    let Some(src) = value_type(prog, func, c.x) else {
        return false;
    };
    if !basic_of(prog, src).is_some_and(|b| b.kind() == BasicKind::String) {
        return false;
    }
    let dst = c.typ.underlying(&prog.type_arena);
    let TypeData::Slice(s) = prog.type_arena.get(dst) else {
        return false;
    };
    basic_of(prog, s.elem()).is_some_and(|b| b.kind() == BasicKind::Int32)
}

// ---------------------------------------------------------------------------
// conversion_overflow.go
// ---------------------------------------------------------------------------

/// gosec `overflowState.isSafeConversion`.
fn is_safe_conversion(ra: &mut RangeAnalyzer<'_>, x: Value, dst: IntTypeInfo, block: BlockId) -> bool {
    if let Value::Const(cid) = x {
        if is_constant_in_type_range(ra.prog, cid, dst) {
            return true;
        }
    }
    has_range_check(ra, x, dst, block)
}

/// gosec `overflowState.hasRangeCheck`.
fn has_range_check(
    ra: &mut RangeAnalyzer<'_>,
    v: Value,
    dst: IntTypeInfo,
    block: BlockId,
) -> bool {
    let res = ra.resolve_range(v, block);

    if explicit_vals_in_range(&res.explicit_positive_vals, &res.explicit_negative_vals, dst) {
        return true;
    }

    // Every predecessor safe on its own edge is safe overall (`||` guards).
    let preds = ra.func.blocks.get(block).preds.clone();
    if preds.len() > 1 {
        let mut all_safe = true;
        for pred in &preds {
            if !is_safe_from_predecessor(ra, v, dst, *pred, block) {
                all_safe = false;
                break;
            }
        }
        if all_safe {
            return true;
        }
    }

    // A definition-based range (constants, arithmetic on constants) is certain
    // even without an `if`.
    let definitive = res.min_value_set && res.max_value_set;
    if !res.is_range_check && !definitive {
        return false;
    }

    validate_range_limits(ra, v, &res, dst)
}

/// gosec `overflowState.validateRangeLimits`.
fn validate_range_limits(
    ra: &mut RangeAnalyzer<'_>,
    v: Value,
    res: &RangeResult,
    dst: IntTypeInfo,
) -> bool {
    let src_unsigned = ra.is_uint(v);

    // Disjoint (impossible) ranges are dead code, hence safe.
    if !src_unsigned
        && res.min_value_set
        && res.max_value_set
        && to_int64(res.min_value) > to_int64(res.max_value)
    {
        return true;
    }
    if src_unsigned && res.min_value_set && res.max_value_set && res.min_value > res.max_value {
        return true;
    }

    let Some(src) = value_type(ra.prog, ra.func, v).and_then(|t| int_type_info(ra.prog, t)) else {
        return false;
    };

    if dst.signed {
        if src_unsigned {
            return res.max_value_set && res.max_value <= dst.max;
        }
        let mut min_safe = true;
        if src.min < dst.min {
            min_safe = res.min_value_set && to_int64(res.min_value) >= dst.min;
        }
        let mut max_safe = true;
        if src.max > dst.max {
            max_safe = res.max_value_set && to_int64(res.max_value) <= to_int64(dst.max);
        }
        return min_safe && max_safe;
    }

    if src_unsigned {
        return res.max_value_set && res.max_value <= dst.max;
    }
    let mut min_safe = true;
    if src.min < 0 {
        let mut min_bound = 0i64;
        if res.is_range_check
            && res.max_value_set
            && to_int64(res.max_value) > signed_max_for_unsigned_size(dst.size)
        {
            min_bound = signed_min_for_unsigned_size(dst.size);
        }
        min_safe = res.min_value_set && to_int64(res.min_value) >= min_bound;
    }
    let mut max_safe = true;
    if src.max > dst.max {
        max_safe = res.max_value_set && res.max_value <= dst.max;
    }
    min_safe && max_safe
}

/// gosec `overflowState.isSafeFromPredecessor`.
fn is_safe_from_predecessor(
    ra: &mut RangeAnalyzer<'_>,
    v: Value,
    dst: IntTypeInfo,
    pred: BlockId,
    target: BlockId,
) -> bool {
    let func = ra.func;

    // Follow the phi to the value that actually arrives on this edge.
    let mut edge_value = v;
    if let Value::Instr(iid) = v {
        if let InstrData::Phi(phi) = func.instrs.get(iid) {
            if ra.instr_block(iid) == Some(target) {
                let preds = &func.blocks.get(target).preds;
                for (i, &p) in preds.iter().enumerate() {
                    if p == pred {
                        if let Some(Some(e)) = phi.edges.get(i) {
                            edge_value = *e;
                        }
                        break;
                    }
                }
            }
        }
    }

    if let Some(&last) = func.blocks.get(pred).instrs.last() {
        if matches!(func.instrs.get(last), InstrData::If(_)) {
            let succs = func.blocks.get(pred).succs.clone();
            for (i, &succ) in succs.iter().enumerate() {
                if succ == target {
                    let result = ra.result_range_for_if_edge(last, i == 0, edge_value);
                    if is_safe_if_edge_result(ra, edge_value, dst, &result) {
                        return true;
                    }
                }
            }
        }
    }

    let pred_preds = func.blocks.get(pred).preds.clone();
    if pred_preds.len() == 1 {
        let parent = pred_preds[0];
        if let Some(&last) = func.blocks.get(parent).instrs.last() {
            if matches!(func.instrs.get(last), InstrData::If(_)) {
                let succs = func.blocks.get(parent).succs.clone();
                for (i, &succ) in succs.iter().enumerate() {
                    if succ == pred {
                        let result = ra.result_range_for_if_edge(last, i == 0, edge_value);
                        if is_safe_if_edge_result(ra, edge_value, dst, &result) {
                            return true;
                        }
                    }
                }
            }
        }
    }

    false
}

/// gosec `overflowState.isSafeIfEdgeResult`.
fn is_safe_if_edge_result(
    ra: &RangeAnalyzer<'_>,
    v: Value,
    dst: IntTypeInfo,
    result: &RangeResult,
) -> bool {
    if !result.is_range_check {
        return false;
    }
    let src_unsigned = ra.is_uint(v);
    if dst.signed {
        if src_unsigned {
            return result.max_value_set && result.max_value <= dst.max;
        }
        return result.min_value_set
            && to_int64(result.min_value) >= dst.min
            && result.max_value_set
            && to_int64(result.max_value) <= to_int64(dst.max);
    }
    if src_unsigned {
        return result.max_value_set && result.max_value <= dst.max;
    }
    result.min_value_set
        && to_int64(result.min_value) >= 0
        && result.max_value_set
        && result.max_value <= dst.max
}

/// gosec `runConversionOverflow`: every `Convert` in dominator preorder whose
/// destination cannot hold the source's whole range and that no range check
/// guards.
pub(crate) fn collect_g115(
    prog: &Program,
    src_funcs: &[FuncId],
    pending: &mut Vec<(u32, String)>,
) {
    for &fid in src_funcs {
        let func = prog.functions.get(fid);
        if func.blocks.is_empty() {
            continue;
        }
        let mut ra = RangeAnalyzer::new(prog, func);
        for bid in func.dom_preorder() {
            let block = func.blocks.get(bid);
            if block.deleted {
                continue;
            }
            for &iid in &block.instrs {
                let InstrData::Convert(c) = func.instrs.get(iid) else {
                    continue;
                };
                let (x, dst_typ) = (c.x, c.typ);
                let Some(src_typ) = value_type(prog, func, x) else {
                    continue;
                };
                let (Some(src_info), Some(dst_info)) =
                    (int_type_info(prog, src_typ), int_type_info(prog, dst_typ))
                else {
                    continue;
                };
                if is_same_width_platform_conversion(prog, src_typ, dst_typ) {
                    continue;
                }
                if !has_overflow(src_info, dst_info) {
                    continue;
                }
                if is_safe_conversion(&mut ra, x, dst_info, bid) {
                    continue;
                }
                // The message names the *underlying basic* types, and a
                // conversion whose operand is a pointer (which GetIntTypeInfo
                // accepts) has none.
                let (Some(src_basic), Some(dst_basic)) =
                    (basic_of(prog, src_typ), basic_of(prog, dst_typ))
                else {
                    continue;
                };
                let msg = format!(
                    "G115: integer overflow conversion {} -> {}",
                    src_basic.name(),
                    dst_basic.name()
                );
                let pos = func.pos(iid);
                if pos.is_valid() {
                    pending.push((pos.0 as u32, msg));
                }
            }
        }
    }
}
