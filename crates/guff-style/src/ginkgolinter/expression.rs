//! `internal/expression` (with its `value`, `actual`, `matcher` sub-packages)
//! and `internal/intervals`.
//!
//! Upstream deep-copies the assertion (`astcopy.CallExpr`) and keeps pointers
//! into the copy — the actual call, its argument, the matcher — which the rules
//! then rewrite in place before the copy is printed as the suggestion. Owned
//! Rust trees cannot hold those pointers, so the copy is addressed by path
//! instead: the actual call is `actual_depth` selector hops below the root, and
//! the matcher is always the root's first argument (upstream strips the `Not`
//! wrappers off the copy as soon as it builds the matcher). Sub-expressions a
//! rule only reads, or copies elsewhere, are kept as owned clones; node ids
//! survive cloning, so a clone still answers type queries.

use guff::ast::{BasicLit, BinaryExpr, CallExpr, Expr, Ident, SelectorExpr};
use guff::position::Pos;
use guff::token::Token;
use guff_types::api::TypeAndValue;
use guff_types::arena::{TypeData, TypeId};
use guff_types::basic::{IS_BOOLEAN, IS_FLOAT, IS_INTEGER, IS_STRING};

use super::gomega::{BasicInfo, CallType, Handler};
use super::types::{int64, tv_is_nil, tv_is_value, Tc};

// ---------------------------------------------------------------------------
// value

#[derive(Clone)]
pub(super) enum VKind {
    Unspecified,
    Err,
    Int(i64),
}

/// `value.Valuer` (and the `value.Value` it is built on).
#[derive(Clone)]
pub(super) struct Valuer {
    pub expr: Option<Expr>,
    pub tv: Option<TypeAndValue>,
    pub kind: VKind,
}

impl Valuer {
    /// `value.New`.
    pub fn plain(tc: &Tc<'_>, orig: &Expr, clone: &Expr) -> Valuer {
        Valuer {
            expr: Some(clone.clone()),
            tv: tc.tv(orig).cloned(),
            kind: VKind::Unspecified,
        }
    }

    /// `value.GetValuer`.
    pub fn get(tc: &Tc<'_>, orig: &Expr, clone: &Expr) -> Valuer {
        Valuer::get_with_error(tc, orig, clone, false)
    }

    /// `value.GetValuerWithError`.
    pub fn get_with_error(tc: &Tc<'_>, orig: &Expr, clone: &Expr, with_error_method: bool) -> Valuer {
        let mut v = Valuer::plain(tc, orig, clone);
        if is_expr_error(tc, orig, with_error_method) {
            v.kind = VKind::Err;
            return v;
        }
        let Some(tv) = &v.tv else {
            return v;
        };
        let Some(val) = &tv.val else {
            return v;
        };
        if !tv_is_value(tv) {
            return v;
        }
        if val.kind() == guff_constant::Kind::Int {
            if let Some(n) = int64(val) {
                v.kind = VKind::Int(n);
            }
        }
        v
    }

    pub fn value_expr(&self) -> Option<Expr> {
        self.expr.clone()
    }
    pub fn get_type(&self) -> Option<TypeId> {
        self.tv.as_ref().map(|tv| tv.typ)
    }
    pub fn is_value_zero(&self) -> bool {
        matches!(self.kind, VKind::Int(0))
    }
    pub fn is_value_numeric(&self) -> bool {
        matches!(self.kind, VKind::Int(_))
    }
    pub fn int_value(&self) -> Option<i64> {
        match self.kind {
            VKind::Int(n) => Some(n),
            _ => None,
        }
    }
    pub fn is_error(&self) -> bool {
        matches!(self.kind, VKind::Err)
    }
    pub fn is_func(&self) -> bool {
        match self.kind {
            VKind::Int(_) => false,
            _ => matches!(self.expr, Some(Expr::CallExpr(_))),
        }
    }
    pub fn is_interface(&self, tc: &Tc<'_>) -> bool {
        tc.is_interface(self.get_type())
    }
    pub fn is_pointer(&self, tc: &Tc<'_>) -> bool {
        tc.is_pointer(self.get_type())
    }
    pub fn is_nil(&self) -> bool {
        self.tv.as_ref().is_some_and(tv_is_nil)
    }
}

/// `value.IsExprError`.
pub(super) fn is_expr_error(tc: &Tc<'_>, expr: &Expr, with_error_method: bool) -> bool {
    let Some(t) = tc.type_of(expr) else {
        return false;
    };
    match tc.data(t) {
        TypeData::Named(_) => tc.implements_error(t),
        TypeData::Pointer(p) => {
            if tc.implements_error(t) {
                return true;
            }
            let elem = p.elem();
            tc.is_named(elem) && tc.implements_error(elem)
        }
        TypeData::Tuple(_) => {
            let elems = tc.tuple(t).unwrap_or_default();
            let Some(ret) = (if with_error_method { elems.last() } else { elems.first() }) else {
                return false;
            };
            matches!(tc.data(*ret), TypeData::Named(_) | TypeData::Pointer(_)) && tc.implements_error(*ret)
        }
        _ => false,
    }
}

/// `value.IsNil`.
pub(super) fn is_nil(tc: &Tc<'_>, e: &Expr) -> bool {
    matches!(e, Expr::Ident(_)) && tc.tv(e).is_some_and(tv_is_nil)
}

// ---------------------------------------------------------------------------
// actual

pub(super) mod arg {
    pub const UNKNOWN: u64 = 1 << 0;
    pub const ERR_ACTUAL: u64 = 1 << 1;
    pub const LEN_FUNC: u64 = 1 << 2;
    pub const CAP_FUNC: u64 = 1 << 3;
    pub const COMPARISON: u64 = 1 << 4;
    pub const LEN_COMPARISON: u64 = 1 << 5;
    pub const CAP_COMPARISON: u64 = 1 << 6;
    pub const NIL_COMPARISON: u64 = 1 << 7;
    pub const BINARY_COMPARISON: u64 = 1 << 8;
    pub const FUNC_SIG: u64 = 1 << 9;
    pub const ERR_FUNC: u64 = 1 << 10;
    pub const GOMEGA_PARAM: u64 = 1 << 11;
    pub const TB_PARAM: u64 = 1 << 12;
    pub const MULTI_RETS: u64 = 1 << 13;
    pub const ERROR_METHOD: u64 = 1 << 14;
    pub const ERROR_METHOD_NO_ERR: u64 = 1 << 15;
    pub const ERROR_TYPE: u64 = 1 << 16;
    pub const EQUAL_ZERO: u64 = 1 << 17;
    pub const GREATER_THAN_ZERO: u64 = 1 << 18;
}

/// `actual.ArgPayload` and its implementations.
pub(super) enum ArgPayload {
    /// `RegularArgPayload` — embeds `value.Value`, which is not a `Valuer`.
    Regular(Valuer),
    /// `FuncCallArgPayload`: `len(x)` / `cap(x)`.
    FuncCall { arg_type: u64 },
    /// `ErrPayload`.
    Err(Valuer),
    /// `ErrorMethodPayload`.
    ErrorMethod(Valuer),
    /// `ErrorMethodNoErrPayload`.
    ErrorMethodNoErr,
    /// `FuncComparisonPayload`.
    FuncComparison {
        op: Token,
        arg_type: u64,
        val: Valuer,
        left: Valuer,
        arg: Expr,
    },
    /// `ComparisonArgPayload`.
    Comparison { left: Valuer, right: Valuer, op: Token },
    /// `NilComparisonPayload`.
    NilComparison { val: Valuer, right: Valuer, op: Token },
    /// `FuncSigArgPayload`.
    FuncSig { arg_type: u64 },
}

impl ArgPayload {
    pub fn arg_type(&self) -> u64 {
        match self {
            ArgPayload::Regular(_) => arg::UNKNOWN,
            ArgPayload::FuncCall { arg_type } => *arg_type,
            ArgPayload::Err(_) => arg::ERR_ACTUAL | arg::ERROR_TYPE,
            ArgPayload::ErrorMethod(_) => arg::ERROR_METHOD | arg::ERR_ACTUAL | arg::ERROR_TYPE,
            ArgPayload::ErrorMethodNoErr => arg::ERROR_METHOD_NO_ERR,
            ArgPayload::FuncComparison { arg_type, .. } => *arg_type,
            ArgPayload::Comparison { .. } => arg::BINARY_COMPARISON | arg::COMPARISON,
            ArgPayload::NilComparison { .. } => arg::NIL_COMPARISON,
            ArgPayload::FuncSig { arg_type } => *arg_type,
        }
    }

    /// The payload as a `value.Valuer`, when its Go type implements one.
    /// `FuncComparisonPayload` answers `IsFunc` with `true` regardless.
    pub fn as_valuer(&self) -> Option<(&Valuer, bool)> {
        match self {
            ArgPayload::Err(v) | ArgPayload::ErrorMethod(v) => Some((v, v.is_func())),
            ArgPayload::FuncComparison { val, .. } => Some((val, true)),
            ArgPayload::NilComparison { val, .. } => Some((val, val.is_func())),
            _ => None,
        }
    }

    /// `actual.ComparisonActualPayload`: `(op, left, right)`.
    pub fn as_comparison(&self) -> Option<(Token, &Valuer, &Valuer)> {
        match self {
            ArgPayload::FuncComparison { op, left, val, .. } => Some((*op, left, val)),
            ArgPayload::Comparison { left, right, op } => Some((*op, left, right)),
            ArgPayload::NilComparison { val, right, op } => Some((*op, val, right)),
            _ => None,
        }
    }
}

const ACTUAL_OFFSETS: &[(&str, usize)] = &[
    ("Expect", 0),
    ("ExpectWithOffset", 1),
    ("Ω", 0),
    ("Eventually", 0),
    ("EventuallyWithOffset", 1),
    ("Consistently", 0),
    ("ConsistentlyWithOffset", 1),
];

/// `gomegainfo.ActualArgOffset`.
fn actual_arg_offset(method: &str) -> usize {
    ACTUAL_OFFSETS
        .iter()
        .find(|(m, _)| *m == method)
        .map_or(0, |(_, o)| *o)
}

/// `ginkgoinfo.IsGinkgoContext`.
fn is_ginkgo_context(tc: &Tc<'_>, t: Option<TypeId>) -> bool {
    let Some(t) = t else {
        return false;
    };
    let name = tc.type_string(Some(tc.unalias(t)));
    name == "context.Context" || name.ends_with("github.com/onsi/ginkgo/v2/internal.SpecContext")
}

/// `gomegainfo.IsGomegaType`.
pub(super) fn is_gomega_type(tc: &Tc<'_>, t: TypeId) -> bool {
    let t = tc.unalias(t);
    match tc.data(t) {
        TypeData::Pointer(p) => is_gomega_type(tc, p.elem()),
        TypeData::Named(_) => {
            let name = tc.type_string(Some(t));
            name.ends_with("github.com/onsi/gomega/internal.Gomega")
                || name.ends_with("github.com/onsi/gomega/types.Gomega")
        }
        _ => false,
    }
}

/// `builtinFuncName`.
fn builtin_func_name(call: &CallExpr) -> Option<&str> {
    let Expr::Ident(id) = call.fun.as_ref() else {
        return None;
    };
    if call.args.len() != 1 {
        return None;
    }
    matches!(id.name.as_str(), "len" | "cap" | "min" | "max").then_some(id.name.as_str())
}

/// `getActualArgPayload` → `(payload, actualOffset)`.
fn actual_arg_payload(tc: &Tc<'_>, actual_clone: &CallExpr, info: &BasicInfo<'_>) -> Option<(ArgPayload, usize)> {
    let root = info.root_call;
    let mut offset = actual_arg_offset(&info.method_name);
    if root.args.len() <= offset {
        return None;
    }
    let mut orig = &root.args[offset];
    let mut clone = actual_clone.args.get(offset)?;
    if info.call_type == CallType::Async && is_ginkgo_context(tc, tc.type_of(orig)) {
        offset += 1;
        if root.args.len() <= offset {
            return None;
        }
        orig = &root.args[offset];
        clone = actual_clone.args.get(offset)?;
    }

    let payload = if is_expr_error(tc, orig, info.has_error_method) {
        let v = Valuer::get_with_error(tc, orig, clone, info.has_error_method);
        Some(if info.has_error_method {
            ArgPayload::ErrorMethod(v)
        } else {
            ArgPayload::Err(v)
        })
    } else if info.has_error_method {
        Some(ArgPayload::ErrorMethodNoErr)
    } else {
        match (orig, clone) {
            (Expr::CallExpr(o), Expr::CallExpr(_)) => func_call_payload(o),
            (Expr::BinaryExpr(o), Expr::BinaryExpr(c)) => parse_binary_expr(tc, o, c),
            _ => None,
        }
    };
    if let Some(p) = payload {
        return Some((p, offset));
    }

    if let Some(t) = tc.type_of(orig) {
        if let Some(sig) = tc.signature(t) {
            return Some((async_func_arg(tc, sig), offset));
        }
    }
    Some((ArgPayload::Regular(Valuer::plain(tc, orig, clone)), offset))
}

/// `newFuncCallArgPayload`.
fn func_call_payload(orig: &CallExpr) -> Option<ArgPayload> {
    let arg_type = match builtin_func_name(orig)? {
        "len" => arg::LEN_FUNC,
        "cap" => arg::CAP_FUNC,
        _ => return None,
    };
    Some(ArgPayload::FuncCall { arg_type })
}

/// `getAsyncFuncArg`.
fn async_func_arg(tc: &Tc<'_>, (params, results, _): (Vec<TypeId>, Vec<TypeId>, bool)) -> ArgPayload {
    let mut arg_type = arg::FUNC_SIG;
    if results.len() == 1 && tc.implements_error(tc.underlying(results[0])) {
        arg_type |= arg::ERR_FUNC | arg::ERROR_TYPE;
    }
    if let Some(first) = params.first() {
        if results.is_empty() {
            if is_gomega_type(tc, *first) {
                arg_type |= arg::FUNC_SIG | arg::GOMEGA_PARAM;
            }
            if tc.implements_tb(*first) {
                arg_type |= arg::FUNC_SIG | arg::TB_PARAM;
            }
        }
    }
    if results.len() > 1 {
        arg_type |= arg::FUNC_SIG | arg::MULTI_RETS;
    }
    ArgPayload::FuncSig { arg_type }
}

/// `reverseassertion.ChangeCompareOperator`.
fn reverse_compare(op: Token) -> Token {
    match op {
        Token::LSS => Token::GTR,
        Token::GTR => Token::LSS,
        Token::LEQ => Token::GEQ,
        Token::GEQ => Token::LEQ,
        other => other,
    }
}

/// `parseBinaryExpr`.
fn parse_binary_expr(tc: &Tc<'_>, orig: &BinaryExpr, clone: &BinaryExpr) -> Option<ArgPayload> {
    let (mut left, mut right, mut op) = (orig.x.as_ref(), orig.y.as_ref(), orig.op);
    let replace = match left {
        Expr::Ident(_) => tc.tv(left).is_some_and(|tv| {
            tc.basic_kind(tv.typ).is_some() && (tv.val.is_some() || tv_is_nil(tv))
        }),
        Expr::BasicLit(_) => true,
        _ => false,
    };
    if replace {
        std::mem::swap(&mut left, &mut right);
    }
    match op {
        Token::EQL | Token::NEQ => {}
        Token::GTR | Token::GEQ | Token::LSS | Token::LEQ => {
            if replace {
                op = reverse_compare(op);
            }
        }
        _ => return None,
    }
    let (mut left_clone, mut right_clone) = (clone.x.as_ref(), clone.y.as_ref());
    if replace {
        std::mem::swap(&mut left_clone, &mut right_clone);
    }
    let left_val = Valuer::get(tc, left, left_clone);
    let right_val = Valuer::get(tc, right, right_clone);

    if is_nil(tc, right) {
        return Some(ArgPayload::NilComparison {
            val: left_val,
            right: right_val,
            op,
        });
    }
    if let (Expr::CallExpr(first), Expr::CallExpr(first_clone)) = (left, left_clone) {
        if let Some(p) = func_comparison_payload(tc, first, first_clone, right, right_clone, op) {
            return Some(p);
        }
    }
    Some(ArgPayload::Comparison {
        left: left_val,
        right: right_val,
        op,
    })
}

/// `newFuncComparisonPayload`.
fn func_comparison_payload(
    tc: &Tc<'_>,
    orig_left: &CallExpr,
    left_clone: &CallExpr,
    orig_right: &Expr,
    right_clone: &Expr,
    op: Token,
) -> Option<ArgPayload> {
    let name = builtin_func_name(orig_left)?;
    let left = Valuer::get(tc, &Expr::CallExpr(orig_left.clone()), &Expr::CallExpr(left_clone.clone()));
    let val = Valuer::get(tc, orig_right, right_clone);

    let mut arg_type = arg::COMPARISON;
    match name {
        "len" => {
            arg_type |= arg::LEN_COMPARISON;
            if val.is_value_numeric() {
                if val.is_value_zero() {
                    match op {
                        Token::EQL => arg_type |= arg::EQUAL_ZERO,
                        Token::NEQ | Token::GTR => arg_type |= arg::GREATER_THAN_ZERO,
                        _ => {}
                    }
                } else if val.int_value() == Some(1) && op == Token::GEQ {
                    arg_type |= arg::GREATER_THAN_ZERO;
                }
            }
            if arg_type & arg::GREATER_THAN_ZERO == 0 && op != Token::EQL && op != Token::NEQ {
                return None;
            }
        }
        "cap" => {
            if op != Token::EQL && op != Token::NEQ {
                return None;
            }
            arg_type |= arg::CAP_COMPARISON;
        }
        _ => return None,
    }
    Some(ArgPayload::FuncComparison {
        op,
        arg_type,
        val,
        left,
        arg: left_clone.args[0].clone(),
    })
}

/// `isValidAsyncValueType`.
fn is_valid_async_value_type(tc: &Tc<'_>, t: Option<TypeId>) -> bool {
    let Some(t) = t else {
        return false;
    };
    match tc.data(t) {
        TypeData::Signature(_) | TypeData::Chan(_) | TypeData::Pointer(_) => true,
        TypeData::Named(_) => is_valid_async_value_type(tc, Some(tc.underlying(t))),
        _ => false,
    }
}

/// `actual.AsyncArg`.
pub(super) struct AsyncArg {
    pub valid: bool,
    /// The function call passed as the actual, in the copy.
    pub fun: Option<CallExpr>,
    pub timeout: Option<Duration>,
    pub polling: Option<Duration>,
    pub too_many_timeouts: bool,
    pub too_many_polling: bool,
}

/// `actual.Actual`.
pub(super) struct Actual {
    pub arg: ArgPayload,
    pub arg_type: Option<TypeId>,
    pub is_tuple: bool,
    pub async_arg: Option<AsyncArg>,
    pub offset: usize,
}

// ---------------------------------------------------------------------------
// intervals

/// `intervals.DurationValue`. Durations are nanoseconds, as `time.Duration`.
pub(super) enum Duration {
    Real(i64),
    UnknownType,
    Numeric {
        num_seconds: i64,
        offset: usize,
        dur: i64,
        expr: Expr,
        use_orig_expr: bool,
    },
    Unknown,
    Str { dur: i64, offset: usize },
}

const SECOND: i64 = 1_000_000_000;

impl Duration {
    pub fn duration(&self) -> i64 {
        match self {
            Duration::Real(d) => *d,
            Duration::Numeric { dur, .. } | Duration::Str { dur, .. } => *dur,
            Duration::Unknown | Duration::UnknownType => 0,
        }
    }
}

/// `intervals.GetDuration`.
fn get_duration(tc: &Tc<'_>, offset: usize, orig: &Expr, clone: &Expr) -> Duration {
    let Some(tv) = tc.tv(orig) else {
        return Duration::Unknown;
    };
    if matches!(tc.data(tv.typ), TypeData::Named(_)) && tc.type_string(Some(tv.typ)) == "time.Duration" {
        if let Some(n) = tv.val.as_ref().and_then(int64) {
            return Duration::Real(n);
        }
        return Duration::UnknownType;
    }
    let (Some((_, info, name)), Some(val)) = (tc.basic_kind(tv.typ), tv.val.as_ref()) else {
        return Duration::Unknown;
    };
    if info.0 & IS_INTEGER.0 != 0 {
        if let Some(num) = int64(val) {
            return Duration::Numeric {
                num_seconds: num,
                offset,
                dur: num.wrapping_mul(SECOND),
                expr: clone.clone(),
                use_orig_expr: name == "int",
            };
        }
    }
    if info.0 & IS_FLOAT.0 != 0 && val.kind() == guff_constant::Kind::Float {
        let (num, exact) = guff_constant::float64_val(val);
        if exact {
            return Duration::Numeric {
                num_seconds: num as i64,
                offset,
                dur: (num as i64).wrapping_mul(SECOND),
                expr: clone.clone(),
                use_orig_expr: false,
            };
        }
    }
    if info.0 & IS_STRING.0 != 0 && val.kind() == guff_constant::Kind::String {
        let s = guff_constant::string_val_lossy(val);
        return match parse_duration(&s) {
            Some(dur) => Duration::Str { dur, offset },
            None => Duration::Unknown,
        };
    }
    Duration::Unknown
}

/// `intervals.GetDurationFromValue`.
fn get_duration_from_value(tc: &Tc<'_>, orig: &Expr) -> Duration {
    match tc.tv(orig).and_then(|tv| tv.val.as_ref()).and_then(int64) {
        Some(n) => Duration::Real(n),
        None => Duration::UnknownType,
    }
}

fn ident(name: &str) -> Ident {
    Ident {
        name_pos: Pos(0),
        name: name.to_string(),
        obj: Default::default(),
        id: 0,
    }
}

fn int_lit(n: i64) -> Expr {
    Expr::BasicLit(BasicLit {
        value_pos: Pos(0),
        value_end: Pos(0),
        kind: Some(Token::INT),
        value: n.to_string(),
        id: 0,
    })
}

fn binary(x: Expr, op: Token, y: Expr) -> Expr {
    Expr::BinaryExpr(BinaryExpr {
        x: Box::new(x),
        op_pos: Pos(0),
        op,
        y: Box::new(y),
        id: 0,
    })
}

fn unit(time_pkg: &str, name: &str) -> Expr {
    Expr::SelectorExpr(SelectorExpr {
        x: Box::new(Expr::Ident(ident(time_pkg))),
        sel: ident(name),
        id: 0,
    })
}

/// `NumericValue.GetDurationExpr` for the two kinds that have one.
fn duration_expr(d: &Duration, time_pkg: &str) -> Option<Expr> {
    match d {
        Duration::Numeric {
            num_seconds,
            expr,
            use_orig_expr,
            ..
        } => {
            let second = unit(time_pkg, "Second");
            if *num_seconds == 1 {
                return Some(second);
            }
            let y = if *use_orig_expr { expr.clone() } else { int_lit(*num_seconds) };
            Some(binary(second, Token::MUL, y))
        }
        Duration::Str { dur, .. } => Some(duration_to_expr(*dur, time_pkg)),
        _ => None,
    }
}

/// `durationToExpr`.
fn duration_to_expr(mut d: i64, time_pkg: &str) -> Expr {
    const UNITS: &[(&str, i64)] = &[
        ("Hour", 3600 * SECOND),
        ("Minute", 60 * SECOND),
        ("Second", SECOND),
        ("Millisecond", 1_000_000),
        ("Microsecond", 1_000),
    ];
    let mut out: Option<Expr> = None;
    let push = |e: Expr, out: &mut Option<Expr>| {
        *out = Some(match out.take() {
            None => e,
            Some(prev) => binary(prev, Token::ADD, e),
        });
    };
    for (name, size) in UNITS {
        if d >= *size {
            let n = d / size;
            d -= n * size;
            push(amount(time_pkg, name, n), &mut out);
        }
    }
    if d > 0 {
        push(amount(time_pkg, "Nanosecond", d), &mut out);
    }
    // A zero duration produces a nil expression upstream; the printer then
    // writes nothing for it.
    out.unwrap_or_else(|| Expr::BadExpr(Default::default()))
}

fn amount(time_pkg: &str, name: &str, n: i64) -> Expr {
    let u = unit(time_pkg, name);
    if n == 1 {
        u
    } else {
        binary(u, Token::MUL, int_lit(n))
    }
}

/// `time.ParseDuration`.
pub(super) fn parse_duration(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let mut i = 0;
    let mut neg = false;
    if i < b.len() && (b[i] == b'-' || b[i] == b'+') {
        neg = b[i] == b'-';
        i += 1;
    }
    if &b[i..] == b"0" {
        return Some(0);
    }
    if i == b.len() {
        return None;
    }
    let mut d: u64 = 0;
    while i < b.len() {
        if !(b[i] == b'.' || b[i].is_ascii_digit()) {
            return None;
        }
        // leading integer
        let start = i;
        let mut v: u64 = 0;
        let mut overflow = false;
        while i < b.len() && b[i].is_ascii_digit() {
            if v > (1u64 << 63) / 10 {
                overflow = true;
            }
            v = v.wrapping_mul(10).wrapping_add((b[i] - b'0') as u64);
            if v > 1u64 << 63 {
                overflow = true;
            }
            i += 1;
        }
        if overflow {
            return None;
        }
        let pre = i != start;
        let mut f: u64 = 0;
        let mut scale: f64 = 1.0;
        let mut post = false;
        if i < b.len() && b[i] == b'.' {
            i += 1;
            let fs = i;
            let mut of = false;
            while i < b.len() && b[i].is_ascii_digit() {
                if of {
                    i += 1;
                    continue;
                }
                if f > ((1u64 << 63) - 1) / 10 {
                    of = true;
                    i += 1;
                    continue;
                }
                let y = f * 10 + (b[i] - b'0') as u64;
                if y > 1u64 << 63 {
                    of = true;
                    i += 1;
                    continue;
                }
                f = y;
                scale *= 10.0;
                i += 1;
            }
            post = i != fs;
        }
        if !pre && !post {
            return None;
        }
        let us = i;
        while i < b.len() && !(b[i] == b'.' || b[i].is_ascii_digit()) {
            i += 1;
        }
        if us == i {
            return None;
        }
        let unit: u64 = match &s[us..i] {
            "ns" => 1,
            "us" | "µs" | "μs" => 1_000,
            "ms" => 1_000_000,
            "s" => 1_000_000_000,
            "m" => 60 * 1_000_000_000,
            "h" => 3600 * 1_000_000_000,
            _ => return None,
        };
        if v > (1u64 << 63) / unit {
            return None;
        }
        v *= unit;
        if f > 0 {
            v += (f as f64 * (unit as f64 / scale)) as u64;
            if v > 1u64 << 63 {
                return None;
            }
        }
        d += v;
        if d > 1u64 << 63 {
            return None;
        }
    }
    if neg {
        return Some((d as i64).wrapping_neg());
    }
    if d > (1u64 << 63) - 1 {
        return None;
    }
    Some(d as i64)
}

// ---------------------------------------------------------------------------
// matcher

pub(super) mod mt {
    pub const UNSPECIFIED: u64 = 1 << 0;
    pub const EQUAL: u64 = 1 << 1;
    pub const BE_ZERO: u64 = 1 << 2;
    pub const BE_EMPTY: u64 = 1 << 3;
    pub const BE_TRUE: u64 = 1 << 4;
    pub const BE_FALSE: u64 = 1 << 5;
    pub const BE_NUMERICALLY: u64 = 1 << 6;
    pub const HAVE_LEN_ZERO: u64 = 1 << 7;
    pub const BE_EQUIVALENT_TO: u64 = 1 << 8;
    pub const BE_IDENTICAL_TO: u64 = 1 << 9;
    pub const BE_NIL: u64 = 1 << 10;
    pub const MATCH_ERROR: u64 = 1 << 11;
    pub const MULTIPLE: u64 = 1 << 12;
    pub const HAVE_VALUE: u64 = 1 << 13;
    pub const WITH_TRANSFORM: u64 = 1 << 14;
    pub const EQUAL_BOOL_VALUE: u64 = 1 << 15;
    pub const EQUAL_VALUE: u64 = 1 << 16;
    pub const HAVE_OCCURRED: u64 = 1 << 17;
    pub const SUCCEED: u64 = 1 << 18;
    pub const EQUAL_NIL: u64 = 1 << 19;
    pub const BOOL_VALUE_FALSE: u64 = 1 << 20;
    pub const BOOL_VALUE_TRUE: u64 = 1 << 21;
    pub const OR: u64 = 1 << 22;
    pub const AND: u64 = 1 << 23;
    pub const ERR_MATCH_WITH_ERR: u64 = 1 << 24;
    pub const ERR_MATCH_WITH_ERR_FUNC: u64 = 1 << 25;
    pub const ERR_MATCH_WITH_STRING: u64 = 1 << 26;
    pub const ERR_MATCH_WITH_MATCHER: u64 = 1 << 27;
    pub const EQUAL_ZERO: u64 = 1 << 28;
    pub const GREATER_THAN_ZERO: u64 = 1 << 29;
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MatchErrKind {
    Invalid,
    WithErr,
    WithErrFunc { second_is_string: bool },
    WithString,
    WithMatcher,
}

/// `matcher.Info` and its implementations.
pub(super) enum Info {
    Unspecified(String),
    Equal(Valuer),
    EqualNil(Option<TypeId>),
    EqualTrue,
    EqualFalse,
    BeZero,
    BeEmpty,
    BeTrue,
    BeFalse,
    BeNil,
    BeNumerically { op: Token, value: Valuer, ty: u64 },
    HaveLenZero,
    BeEquivalentTo(Valuer),
    BeIdenticalTo(Valuer),
    MatchError { kind: MatchErrKind, num_args: usize },
    HaveValue(Box<Matcher>),
    WithTransform { func_type: Option<TypeId>, nested: Box<Matcher> },
    Multiple { ty: u64, matchers: Vec<Matcher> },
    Succeed,
    HaveOccurred,
}

impl Info {
    pub fn ty(&self) -> u64 {
        match self {
            Info::Unspecified(_) => mt::UNSPECIFIED,
            Info::Equal(_) => mt::EQUAL,
            Info::EqualNil(_) => mt::EQUAL_NIL | mt::EQUAL | mt::EQUAL_VALUE,
            Info::EqualTrue => mt::EQUAL | mt::EQUAL_BOOL_VALUE | mt::BOOL_VALUE_TRUE,
            Info::EqualFalse => mt::EQUAL | mt::EQUAL_BOOL_VALUE | mt::BOOL_VALUE_FALSE,
            Info::BeZero => mt::BE_ZERO,
            Info::BeEmpty => mt::BE_EMPTY,
            Info::BeTrue => mt::BE_TRUE | mt::BOOL_VALUE_TRUE,
            Info::BeFalse => mt::BE_FALSE | mt::BOOL_VALUE_FALSE,
            Info::BeNil => mt::BE_NIL,
            Info::BeNumerically { ty, .. } => *ty,
            Info::HaveLenZero => mt::HAVE_LEN_ZERO,
            Info::BeEquivalentTo(_) => mt::BE_EQUIVALENT_TO,
            Info::BeIdenticalTo(_) => mt::BE_IDENTICAL_TO,
            Info::MatchError { kind, .. } => {
                mt::MATCH_ERROR
                    | match kind {
                        MatchErrKind::Invalid => 0,
                        MatchErrKind::WithErr => mt::ERR_MATCH_WITH_ERR,
                        MatchErrKind::WithErrFunc { .. } => mt::ERR_MATCH_WITH_ERR_FUNC,
                        MatchErrKind::WithString => mt::ERR_MATCH_WITH_STRING,
                        MatchErrKind::WithMatcher => mt::ERR_MATCH_WITH_MATCHER,
                    }
            }
            Info::HaveValue(_) => mt::HAVE_VALUE,
            Info::WithTransform { .. } => mt::WITH_TRANSFORM,
            Info::Multiple { ty, .. } => *ty,
            Info::Succeed => mt::SUCCEED,
            Info::HaveOccurred => mt::HAVE_OCCURRED,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Info::Unspecified(n) => n,
            Info::Equal(_) | Info::EqualNil(_) | Info::EqualTrue | Info::EqualFalse => "Equal",
            Info::BeZero => "BeZero",
            Info::BeEmpty => "BeEmpty",
            Info::BeTrue => "BeTrue",
            Info::BeFalse => "BeFalse",
            Info::BeNil => "BeNil",
            Info::BeNumerically { .. } => "BeNumerically",
            Info::HaveLenZero => "HaveLen",
            Info::BeEquivalentTo(_) => "BeEquivalentTo",
            Info::BeIdenticalTo(_) => "BeIdenticalTo",
            Info::MatchError { .. } => "MatchError",
            Info::HaveValue(_) => "HaveValue",
            Info::WithTransform { .. } => "WithTransform",
            Info::Multiple { ty, .. } => {
                if ty & mt::OR != 0 {
                    "Or"
                } else {
                    "And"
                }
            }
            Info::Succeed => "Succeed",
            Info::HaveOccurred => "HaveOccurred",
        }
    }

    /// The info as a `value.Valuer` — `EqualMatcher` and
    /// `BeNumericallyMatcher` implement it.
    pub fn as_valuer(&self) -> Option<&Valuer> {
        match self {
            Info::Equal(v) | Info::BeNumerically { value: v, .. } => Some(v),
            _ => None,
        }
    }

    /// `matcher.MatchErrorMatcher.AllowedNumArgs`.
    pub fn allowed_num_args(kind: MatchErrKind) -> usize {
        match kind {
            MatchErrKind::WithErrFunc { .. } => 2,
            _ => 1,
        }
    }
}

/// `matcher.Matcher`. `orig` is the innermost call once `Not` wrappers are
/// peeled; the copy is addressed by the caller.
pub(super) struct Matcher {
    pub info: Info,
    pub reverse_logic: bool,
    pub has_not: bool,
}

impl Matcher {
    /// `matcher.New` → `(matcher, innermost orig call, innermost clone call)`.
    pub fn new<'o>(
        tc: &Tc<'_>,
        handler: &Handler,
        mut orig: &'o CallExpr,
        mut clone: &CallExpr,
    ) -> Option<(Matcher, &'o CallExpr, CallExpr)> {
        let mut reverse = false;
        let mut has_not = false;
        let name = loop {
            let info = handler.basic_info(tc, orig)?;
            if info.method_name != "Not" {
                break info.method_name;
            }
            has_not = true;
            reverse = !reverse;
            let Some(Expr::CallExpr(o)) = orig.args.first() else {
                return None;
            };
            let Some(Expr::CallExpr(c)) = clone.args.first() else {
                return None;
            };
            orig = o;
            clone = c;
        };
        let info = matcher_info(tc, handler, orig, clone, &name);
        Some((
            Matcher {
                info,
                reverse_logic: reverse,
                has_not,
            },
            orig,
            clone.clone(),
        ))
    }
}

/// `getMatcherInfo`.
fn matcher_info(tc: &Tc<'_>, handler: &Handler, orig: &CallExpr, clone: &CallExpr, name: &str) -> Info {
    let a0 = orig.args.first().zip(clone.args.first());
    match name {
        "Equal" => {
            if let Some((o, c)) = a0 {
                return equal_matcher(tc, o, c);
            }
        }
        "BeZero" => return Info::BeZero,
        "BeEmpty" => return Info::BeEmpty,
        "BeTrue" => return Info::BeTrue,
        "BeFalse" => return Info::BeFalse,
        "BeNil" => return Info::BeNil,
        "BeNumerically" => {
            if orig.args.len() == 2 && clone.args.len() == 2 {
                return be_numerically_matcher(tc, &orig.args[0], &orig.args[1], &clone.args[1]);
            }
        }
        "HaveLen" => {
            if let Some((o, c)) = a0 {
                if Valuer::get(tc, o, c).is_value_zero() {
                    return Info::HaveLenZero;
                }
            }
        }
        "BeEquivalentTo" => {
            if let Some((o, c)) = a0 {
                return Info::BeEquivalentTo(Valuer::plain(tc, o, c));
            }
        }
        "BeIdenticalTo" => {
            if let Some((o, c)) = a0 {
                return Info::BeIdenticalTo(Valuer::plain(tc, o, c));
            }
        }
        "MatchError" => {
            if !orig.args.is_empty() {
                return match_error_matcher(tc, &orig.args);
            }
        }
        "HaveValue" => {
            if let Some(nested) = nested_matcher(tc, handler, orig, clone, 0) {
                return Info::HaveValue(Box::new(nested));
            }
        }
        "WithTransform" => {
            if let Some(nested) = nested_matcher(tc, handler, orig, clone, 1) {
                let mut func_type = tc.type_of(&orig.args[0]);
                if let Some(t) = func_type {
                    if let Some((_, results, _)) = tc.signature(t) {
                        if let Some(first) = results.first() {
                            func_type = Some(*first);
                        }
                    }
                }
                return Info::WithTransform {
                    func_type,
                    nested: Box::new(nested),
                };
            }
        }
        "Or" | "And" => {
            let ty = mt::MULTIPLE | if name == "Or" { mt::OR } else { mt::AND };
            let mut matchers = Vec::new();
            let mut ok = true;
            for (o, c) in orig.args.iter().zip(&clone.args) {
                let (Expr::CallExpr(o), Expr::CallExpr(c)) = (o, c) else {
                    ok = false;
                    break;
                };
                let Some((mut m, _, _)) = Matcher::new(tc, handler, o, c) else {
                    ok = false;
                    break;
                };
                m.reverse_logic = false;
                matchers.push(m);
            }
            if ok {
                return Info::Multiple { ty, matchers };
            }
        }
        "Succeed" => return Info::Succeed,
        "HaveOccurred" => return Info::HaveOccurred,
        _ => {}
    }
    Info::Unspecified(name.to_string())
}

fn nested_matcher(tc: &Tc<'_>, handler: &Handler, orig: &CallExpr, clone: &CallExpr, offset: usize) -> Option<Matcher> {
    let Some(Expr::CallExpr(o)) = orig.args.get(offset) else {
        return None;
    };
    let Some(Expr::CallExpr(c)) = clone.args.get(offset) else {
        return None;
    };
    Matcher::new(tc, handler, o, c).map(|(m, _, _)| m)
}

/// `newEqualMatcher`.
fn equal_matcher(tc: &Tc<'_>, orig: &Expr, clone: &Expr) -> Info {
    if let Some(v) = tc.tv(orig).and_then(|tv| tv.val.as_ref()) {
        if v.kind() == guff_constant::Kind::Bool {
            return if guff_constant::bool_val(v) {
                Info::EqualTrue
            } else {
                Info::EqualFalse
            };
        }
    }
    if is_nil(tc, orig) {
        return Info::EqualNil(tc.type_of(orig));
    }
    Info::Equal(Valuer::get(tc, orig, clone))
}

/// `newBeNumericallyMatcher`.
fn be_numerically_matcher(tc: &Tc<'_>, op_expr: &Expr, orig: &Expr, clone: &Expr) -> Info {
    let op = match op_expr {
        Expr::BasicLit(lit) if lit.kind == Some(Token::STRING) => match lit.value.as_str() {
            "\"==\"" => Token::EQL,
            "\"<\"" => Token::LSS,
            "\">\"" => Token::GTR,
            "\"=\"" => Token::ASSIGN,
            "\"!=\"" => Token::NEQ,
            "\"<=\"" => Token::LEQ,
            "\">=\"" => Token::GEQ,
            _ => Token::ILLEGAL,
        },
        _ => Token::ILLEGAL,
    };
    if op == Token::ILLEGAL {
        return Info::Unspecified("BeNumerically".to_string());
    }
    let value = Valuer::get(tc, orig, clone);
    let mut ty = mt::BE_NUMERICALLY;
    match value.int_value() {
        Some(0) => match op {
            Token::EQL => ty |= mt::EQUAL_ZERO,
            Token::NEQ | Token::GTR => ty |= mt::GREATER_THAN_ZERO,
            _ => {}
        },
        Some(1) if op == Token::GEQ => ty |= mt::GREATER_THAN_ZERO,
        _ => {}
    }
    Info::BeNumerically { op, value, ty }
}

/// `newMatchErrorMatcher`.
fn match_error_matcher(tc: &Tc<'_>, args: &[Expr]) -> Info {
    let num_args = args.len();
    let kind = if is_expr_error(tc, &args[0], false) {
        MatchErrKind::WithErr
    } else {
        let t = tc.type_of(&args[0]);
        if tc.identical(t, tc_string(tc)) {
            MatchErrKind::WithString
        } else if tc.implements_gomega_matcher(t) {
            MatchErrKind::WithMatcher
        } else if t.is_some_and(|t| is_func_err_bool(tc, t)) {
            let second_is_string = num_args > 1 && tc.identical(tc.type_of(&args[1]), tc_string(tc));
            MatchErrKind::WithErrFunc { second_is_string }
        } else {
            MatchErrKind::Invalid
        }
    };
    Info::MatchError { kind, num_args }
}

fn tc_string(tc: &Tc<'_>) -> Option<TypeId> {
    tc.string_type()
}

/// `isFuncErrBool`.
fn is_func_err_bool(tc: &Tc<'_>, t: TypeId) -> bool {
    let Some((params, results, _)) = tc.signature(t) else {
        return false;
    };
    if params.len() != 1 || results.len() != 1 {
        return false;
    }
    if !tc.implements_error(params[0]) {
        return false;
    }
    matches!(
        tc.basic_kind(results[0]),
        Some((guff_types::basic::BasicKind::Bool, info, name)) if name == "bool" && info.0 == IS_BOOLEAN.0
    )
}

// ---------------------------------------------------------------------------
// expression

/// `expression.GomegaExpression`.
pub(super) struct GomegaExpression<'o> {
    pub orig: &'o CallExpr,
    /// `None` for a missing assertion, which upstream builds without a copy.
    pub clone: Option<CallExpr>,
    pub assertion_func_name: String,
    pub orig_assertion_func_name: String,
    pub actual_func_name: String,
    pub is_async: bool,
    pub actual: Option<Actual>,
    pub matcher: Option<Matcher>,
    /// Selector hops from `clone` down to the actual call in the copy.
    actual_depth: usize,
    time_pkg: String,
    gomega_name: String,
}

/// `reverseassertion.ChangeAssertionLogic`.
fn change_assertion_logic(name: &str) -> &str {
    match name {
        "To" => "ToNot",
        "ToNot" | "NotTo" => "To",
        "Should" => "ShouldNot",
        "ShouldNot" => "Should",
        other => other,
    }
}

fn is_assertion_func(name: &str) -> bool {
    matches!(name, "To" | "ToNot" | "NotTo" | "Should" | "ShouldNot")
}

fn call_at_depth(root: &mut CallExpr, depth: usize) -> &mut CallExpr {
    let mut cur = root;
    for _ in 0..depth {
        let Expr::SelectorExpr(sel) = cur.fun.as_mut() else {
            unreachable!("actual path leaves the selector chain");
        };
        let Expr::CallExpr(next) = sel.x.as_mut() else {
            unreachable!("actual path leaves the call chain");
        };
        cur = next;
    }
    cur
}

impl<'o> GomegaExpression<'o> {
    /// `expression.New`.
    pub fn new(tc: &Tc<'_>, handler: &Handler, orig: &'o CallExpr, time_pkg: &str) -> Option<GomegaExpression<'o>> {
        let info = handler.basic_info(tc, orig)?;
        if !matches!(info.call_type, CallType::Sync | CallType::Async) {
            return None;
        }
        let missing = |info: &BasicInfo<'_>| GomegaExpression {
            orig,
            clone: None,
            assertion_func_name: String::new(),
            orig_assertion_func_name: String::new(),
            actual_func_name: info.method_name.clone(),
            is_async: false,
            actual: None,
            matcher: None,
            actual_depth: 0,
            time_pkg: time_pkg.to_string(),
            gomega_name: handler.name.clone(),
        };
        let Expr::SelectorExpr(orig_sel) = orig.fun.as_ref() else {
            return Some(missing(&info));
        };
        if !is_assertion_func(&orig_sel.sel.name) {
            return Some(missing(&info));
        }

        let mut clone = orig.clone();

        // GetActualExprClone: walk the selector chain until the root call.
        let mut depth = 0usize;
        let mut sel = orig_sel;
        loop {
            let Expr::CallExpr(x) = sel.x.as_ref() else {
                return None;
            };
            depth += 1;
            if std::ptr::eq(x, info.root_call) {
                break;
            }
            let Expr::SelectorExpr(inner) = x.fun.as_ref() else {
                return None;
            };
            sel = inner;
        }

        let actual_clone = call_at_depth(&mut clone, depth).clone();
        let (arg, offset) = actual_arg_payload(tc, &actual_clone, &info)?;

        let mut arg_type = info.root_call.args.get(offset).and_then(|a| tc.type_of(a));
        let mut is_tuple = false;
        if let Some(t) = arg_type {
            if let Some(elems) = tc.tuple(t) {
                arg_type = elems.first().copied();
                is_tuple = elems.len() > 1;
            }
        }
        let is_async = info.call_type == CallType::Async;
        let async_arg = is_async.then(|| async_arg(tc, orig, info.root_call, &actual_clone, arg_type, offset));

        let Some(Expr::CallExpr(orig_matcher)) = orig.args.first() else {
            return None;
        };
        let Some(Expr::CallExpr(matcher_clone)) = clone.args.first() else {
            return None;
        };
        let (matcher, _, inner_clone) = Matcher::new(tc, handler, orig_matcher, matcher_clone)?;
        clone.args[0] = Expr::CallExpr(inner_clone);

        let mut gexp = GomegaExpression {
            orig,
            clone: Some(clone),
            assertion_func_name: orig_sel.sel.name.clone(),
            orig_assertion_func_name: orig_sel.sel.name.clone(),
            actual_func_name: info.method_name.clone(),
            is_async,
            actual: Some(Actual {
                arg,
                arg_type,
                is_tuple,
                async_arg,
                offset,
            }),
            matcher: Some(matcher),
            actual_depth: depth,
            time_pkg: time_pkg.to_string(),
            gomega_name: handler.name.clone(),
        };
        if gexp.matcher.as_ref().is_some_and(|m| m.reverse_logic) {
            gexp.reverse_assertion_func_logic();
        }
        Some(gexp)
    }

    pub fn is_missing_assertion(&self) -> bool {
        self.matcher.is_none()
    }

    fn clone_mut(&mut self) -> &mut CallExpr {
        self.clone.as_mut().expect("assertion copy")
    }

    fn assertion_sel(&mut self) -> &mut Ident {
        let Expr::SelectorExpr(sel) = self.clone_mut().fun.as_mut() else {
            unreachable!("assertion is a selector call");
        };
        &mut sel.sel
    }

    pub fn reverse_assertion_func_logic(&mut self) {
        let sel = self.assertion_sel();
        let new_name = change_assertion_logic(&sel.name).to_string();
        sel.name = new_name.clone();
        self.assertion_func_name = new_name;
    }

    pub fn replace_assertion_method(&mut self, name: &str) {
        self.assertion_sel().name = name.to_string();
    }

    /// The matcher in the copy: always the root's first argument.
    pub fn matcher_clone(&mut self) -> &mut CallExpr {
        let Expr::CallExpr(m) = &mut self.clone_mut().args[0] else {
            unreachable!("matcher is a call");
        };
        m
    }

    pub fn matcher_clone_ref(&self) -> &CallExpr {
        let Expr::CallExpr(m) = &self.clone.as_ref().expect("assertion copy").args[0] else {
            unreachable!("matcher is a call");
        };
        m
    }

    /// `Handler.ReplaceFunction` on the matcher.
    pub fn replace_matcher_func_name(&mut self, name: &str) {
        let m = self.matcher_clone();
        match m.fun.as_mut() {
            Expr::Ident(_) => *m.fun = Expr::Ident(ident(name)),
            Expr::SelectorExpr(sel) => sel.sel = ident(name),
            _ => {}
        }
    }

    pub fn replace_matcher_args(&mut self, args: Vec<Expr>) {
        self.matcher_clone().args = args;
    }

    fn set_matcher_no_args(&mut self, name: &str) {
        self.replace_matcher_func_name(name);
        self.replace_matcher_args(Vec::new());
    }

    pub fn set_matcher_be_zero(&mut self) {
        self.set_matcher_no_args("BeZero");
    }
    pub fn set_matcher_be_empty(&mut self) {
        self.set_matcher_no_args("BeEmpty");
    }
    pub fn set_matcher_succeed(&mut self) {
        self.set_matcher_no_args("Succeed");
    }
    pub fn set_matcher_have_occurred(&mut self) {
        self.set_matcher_no_args("HaveOccurred");
    }
    pub fn set_matcher_be_nil(&mut self) {
        self.set_matcher_no_args("BeNil");
    }
    pub fn set_matcher_be_true(&mut self) {
        self.set_matcher_no_args("BeTrue");
    }
    pub fn set_matcher_be_false(&mut self) {
        self.set_matcher_no_args("BeFalse");
    }

    fn set_matcher_with_arg(&mut self, name: &str, arg: Expr) {
        self.replace_matcher_func_name(name);
        self.replace_matcher_args(vec![arg]);
    }

    pub fn set_matcher_len(&mut self, arg: Expr) {
        self.set_matcher_with_arg("HaveLen", arg);
    }
    pub fn set_matcher_cap(&mut self, arg: Expr) {
        self.set_matcher_with_arg("HaveCap", arg);
    }
    pub fn set_matcher_cap_zero(&mut self) {
        self.set_matcher_with_arg("HaveCap", int_lit(0));
    }
    pub fn set_matcher_equal(&mut self, arg: Expr) {
        self.set_matcher_with_arg("Equal", arg);
    }
    pub fn set_matcher_be_identical_to(&mut self, arg: Expr) {
        self.set_matcher_with_arg("BeIdenticalTo", arg);
    }

    /// `SetMatcherBeNumerically`.
    pub fn set_matcher_be_numerically(&mut self, op: Token, arg: Expr) {
        self.replace_matcher_func_name("BeNumerically");
        let lit = Expr::BasicLit(BasicLit {
            value_pos: Pos(0),
            value_end: Pos(0),
            kind: Some(Token::STRING),
            value: format!("\"{}\"", op.as_str()),
            id: 0,
        });
        self.replace_matcher_args(vec![lit, arg]);
    }

    /// `SetLenNumericMatcher`: `false` where upstream dereferences a nil
    /// `Valuer` (`Equal(nil)` etc. against `len(x)`) and panics.
    pub fn set_len_numeric_matcher(&mut self) -> bool {
        let Some(v) = self.matcher.as_ref().and_then(|m| m.info.as_valuer()).cloned() else {
            return false;
        };
        if v.is_value_zero() {
            self.set_matcher_be_empty();
        } else {
            let Some(e) = v.value_expr() else {
                return false;
            };
            self.replace_matcher_func_name("HaveLen");
            self.replace_matcher_args(vec![e]);
        }
        true
    }

    /// `SetMatcherHaveValue`: wrap the matcher copy in `HaveValue(...)`.
    pub fn set_matcher_have_value(&mut self) {
        let existing = std::mem::replace(&mut self.clone_mut().args[0], Expr::BadExpr(Default::default()));
        let fun = if self.gomega_name.is_empty() {
            Expr::Ident(ident("HaveValue"))
        } else {
            Expr::SelectorExpr(SelectorExpr {
                x: Box::new(Expr::Ident(ident(&self.gomega_name))),
                sel: ident("HaveValue"),
                id: 0,
            })
        };
        self.clone_mut().args[0] = Expr::CallExpr(CallExpr {
            fun: Box::new(fun),
            lparen: Pos(0),
            args: vec![existing],
            ellipsis: Pos(0),
            rparen: Pos(0),
            id: 0,
        });
    }

    pub fn is_negative_assertion(&self) -> bool {
        matches!(self.assertion_func_name.as_str(), "ToNot" | "NotTo" | "ShouldNot")
    }

    // Actual proxies

    pub fn actual(&self) -> &Actual {
        self.actual.as_ref().expect("actual")
    }

    pub fn actual_clone(&mut self) -> &mut CallExpr {
        let depth = self.actual_depth;
        call_at_depth(self.clone_mut(), depth)
    }

    pub fn replace_actual(&mut self, e: Expr) {
        let offset = self.actual().offset;
        self.actual_clone().args[offset] = e;
    }

    /// `ReplaceActualWithItsFirstArg`.
    pub fn replace_actual_with_its_first_arg(&mut self) {
        let offset = self.actual().offset;
        let first = match &self.actual_clone().args[offset] {
            Expr::CallExpr(c) => c.args[0].clone(),
            _ => return,
        };
        self.replace_actual(first);
    }

    /// `Actual.AppendWithArgsMethod`.
    pub fn append_with_args_to_actual(&mut self) {
        let offset = self.actual().offset;
        let Some(fun) = self.actual().async_arg.as_ref().and_then(|a| a.fun.clone()) else {
            return;
        };
        let depth = self.actual_depth;
        let actual = self.actual_clone();
        if fun.args.is_empty() {
            actual.args[offset] = *fun.fun;
            return;
        }
        let mut orig_args = std::mem::take(&mut actual.args);
        orig_args[offset] = *fun.fun.clone();
        let orig_fun = std::mem::replace(actual.fun.as_mut(), Expr::BadExpr(Default::default()));
        *actual.fun = Expr::SelectorExpr(SelectorExpr {
            x: Box::new(Expr::CallExpr(CallExpr {
                fun: Box::new(orig_fun),
                lparen: Pos(0),
                args: orig_args,
                ellipsis: Pos(0),
                rparen: Pos(0),
                id: 0,
            })),
            sel: ident("WithArguments"),
            id: 0,
        });
        actual.args = fun.args;
        self.actual_depth = depth + 1;
    }

    pub fn arg(&self) -> &ArgPayload {
        &self.actual().arg
    }

    pub fn actual_arg_type_is(&self, other: u64) -> bool {
        self.arg().arg_type() & other != 0
    }

    pub fn actual_arg_expr(&mut self) -> Expr {
        let offset = self.actual().offset;
        self.actual_clone().args[offset].clone()
    }

    pub fn info(&self) -> &Info {
        &self.matcher.as_ref().expect("matcher").info
    }

    pub fn matcher_type_is(&self, other: u64) -> bool {
        self.info().ty() & other != 0
    }

    pub fn has_description(&self) -> bool {
        self.orig.args.len() > 1
    }

    /// `checkNumericInterval` on the actual call in the copy.
    pub fn replace_interval(&mut self, d: &Duration) -> bool {
        let (offset, expr) = match d {
            Duration::Numeric { offset, .. } | Duration::Str { offset, .. } => {
                (*offset, duration_expr(d, &self.time_pkg))
            }
            _ => return false,
        };
        if offset == 0 {
            return false;
        }
        let Some(expr) = expr else {
            return false;
        };
        let actual = self.actual_clone();
        if offset < actual.args.len() {
            actual.args[offset] = expr;
        }
        true
    }
}

/// `newAsyncArg`.
fn async_arg(
    tc: &Tc<'_>,
    orig_expr: &CallExpr,
    root: &CallExpr,
    actual_clone: &CallExpr,
    arg_type: Option<TypeId>,
    offset: usize,
) -> AsyncArg {
    let mut fun = None;
    let mut valid = true;
    if let Some(Expr::CallExpr(_)) = root.args.get(offset) {
        if let Some(Expr::CallExpr(c)) = actual_clone.args.get(offset) {
            fun = Some(c.clone());
        }
        valid = is_valid_async_value_type(tc, arg_type);
    }

    let mut timeout = None;
    let mut polling = None;
    if root.args.len() > offset + 1 {
        timeout = Some(get_duration(tc, offset + 1, &root.args[offset + 1], &actual_clone.args[offset + 1]));
        if root.args.len() > offset + 2 {
            polling = Some(get_duration(tc, offset + 2, &root.args[offset + 2], &actual_clone.args[offset + 2]));
        }
    }

    let mut too_many_timeouts = false;
    let mut too_many_polling = false;
    let Expr::SelectorExpr(first) = orig_expr.fun.as_ref() else {
        unreachable!("assertion is a selector call");
    };
    let mut sel: &SelectorExpr = first;
    loop {
        let Expr::CallExpr(call) = sel.x.as_ref() else {
            break;
        };
        let Expr::SelectorExpr(fun_sel) = call.fun.as_ref() else {
            break;
        };
        match fun_sel.sel.name.as_str() {
            "WithTimeout" | "Within" => {
                if timeout.is_some() {
                    too_many_timeouts = true;
                } else if call.args.len() == 1 {
                    timeout = Some(get_duration_from_value(tc, &call.args[0]));
                }
            }
            "WithPolling" | "ProbeEvery" => {
                if polling.is_some() {
                    too_many_polling = true;
                } else if call.args.len() == 1 {
                    polling = Some(get_duration_from_value(tc, &call.args[0]));
                }
            }
            _ => {}
        }
        sel = fun_sel;
    }

    AsyncArg {
        valid,
        fun,
        timeout,
        polling,
        too_many_timeouts,
        too_many_polling,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_duration;

    #[test]
    fn parse_duration_matches_go() {
        assert_eq!(parse_duration("1s"), Some(1_000_000_000));
        assert_eq!(parse_duration("1.5s"), Some(1_500_000_000));
        assert_eq!(parse_duration("2h45m"), Some((2 * 3600 + 45 * 60) * 1_000_000_000));
        assert_eq!(parse_duration("300ms"), Some(300_000_000));
        assert_eq!(parse_duration("-1m"), Some(-60_000_000_000));
        assert_eq!(parse_duration("0"), Some(0));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("10"), None);
        assert_eq!(parse_duration("1x"), None);
    }
}
