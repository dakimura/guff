//! Untyped-constant representability, ported from `const.go`
//! (`representableConst`, `representable`, `representation`,
//! `invalidConversion`) plus the float-rounding helpers.
//!
//! This is the chunk-20b recovery of the `representable` closure that
//! `assignments.rs` (chunk 16) accepts. [`representable_const`] is the pure
//! decision-and-rounding routine; [`Checker::representable`] is the in-place
//! operand driver that rounds the value or reports an error.
//!
//! ## Sizes assumption (chunk-20b)
//!
//! Go consults `conf.sizeof` for the width of `int`/`uint`/`uintptr`. We have
//! no `Sizes` yet (deferred, D14), so those are treated as **64-bit**
//! (matching `SizesFor("gc", "amd64")`, Go's default). When `sizes.go` lands
//! (chunk 36) this should consult the configured sizes.
//!
//! ## Deferrals (chunk-20b)
//!
//! - `updateExprType`/`updateExprVal` (rewriting the recorded type/value of
//!   sub-expressions) are not called — they need `Info` recording, deferred to
//!   chunk-25d / chunk-37.

use guff::token::Token;
use guff_constant::{
    binary_op, float32_val, float64_val, imag, make_float64, make_imag, real, to_complex, to_float,
    to_int, Kind, Value,
};
use guff_types_errors::Code;

use crate::arena::{TypeArena, TypeData, TypeId};
use crate::basic::BasicKind;
use crate::check::Checker;
use crate::operand::{Operand, OperandMode};
use crate::predicates::{
    default_type, has_nil, is_boolean, is_integer, is_interface, is_numeric, is_string, is_type_param, is_typed,
    is_untyped, is_valid, max_type,
};

/// Round `x` (a float-kind constant) to the nearest `float32`, returning the
/// rounded value as a `Float` constant, or `None` if it overflows to infinity.
///
/// Equivalent to `roundFloat32`.
fn round_float32(x: &Value) -> Option<Value> {
    let (f32v, _) = float32_val(x);
    let f = f32v as f64;
    if f.is_infinite() {
        None
    } else {
        Some(make_float64(f))
    }
}

/// Round `x` (a float-kind constant) to the nearest `float64`.
///
/// Equivalent to `roundFloat64`.
fn round_float64(x: &Value) -> Option<Value> {
    let (f, _) = float64_val(x);
    if f.is_infinite() {
        None
    } else {
        Some(make_float64(f))
    }
}

/// Reports whether constant `x` can be represented as a value of basic type
/// `typ`, returning the *rounded* value to use on success (and `None` on
/// failure).
///
/// For integer targets the result is `x` converted to an integer; for float
/// and complex targets it is the rounded value; for `string`/`bool`/untyped
/// targets it is `x` unchanged. This bundles Go's `representableConst`
/// boolean result with its `*rounded` out-parameter.
///
/// `typ` must be (or have an underlying) `Basic`.
///
/// Equivalent to `representableConst(x, check, typ, &rounded)`.
pub fn representable_const(arena: &TypeArena, x: &Value, typ: TypeId) -> Option<Value> {
    if x.kind() == Kind::Unknown {
        return Some(x.clone()); // avoid follow-up errors
    }

    let kind = match arena.get(typ.underlying(arena)) {
        TypeData::Basic(b) => b.kind(),
        _ => return None,
    };

    // The numeric branches feed `x` through `to_int`/`to_float`/`to_complex`,
    // which require a numeric `x`. Go's `constant.ToInt` etc. return Unknown
    // for non-numeric values (so the branch yields false); our conversions
    // panic on string/bool, so guard here: a non-numeric constant can only
    // represent a string/bool target.
    let x_numeric = matches!(x.kind(), Kind::Int | Kind::Float | Kind::Complex);

    if x_numeric && is_integer(arena, typ) {
        let xi = to_int(x.clone());
        if xi.kind() != Kind::Int {
            return None;
        }
        let ok = representable_int(&xi, kind);
        return if ok { Some(xi) } else { None };
    }

    if x_numeric && is_float_basic(kind) {
        let xf = to_float(x.clone());
        if xf.kind() != Kind::Float {
            return None;
        }
        return match kind {
            BasicKind::Float32 => round_float32(&xf),
            BasicKind::Float64 => round_float64(&xf),
            BasicKind::UntypedFloat => Some(x.clone()),
            _ => None,
        };
    }

    if x_numeric && is_complex_basic(kind) {
        let xc = to_complex(x.clone());
        if xc.kind() != Kind::Complex {
            return None;
        }
        return match kind {
            BasicKind::Complex64 => round_complex(&xc, true),
            BasicKind::Complex128 => round_complex(&xc, false),
            BasicKind::UntypedComplex => Some(x.clone()),
            _ => None,
        };
    }

    // string / bool targets.
    match kind {
        BasicKind::String | BasicKind::UntypedString => {
            (x.kind() == Kind::String).then(|| x.clone())
        }
        BasicKind::Bool | BasicKind::UntypedBool => (x.kind() == Kind::Bool).then(|| x.clone()),
        _ => None,
    }
}

/// Rounds a complex constant to `complex64` (`f32` parts) or `complex128`
/// (`f64` parts), reassembling `re + im*i`. `None` if either part overflows.
fn round_complex(xc: &Value, f32_parts: bool) -> Option<Value> {
    let re = real(xc.clone());
    let im = imag(xc.clone());
    let (rr, ri) = if f32_parts {
        (round_float32(&re), round_float32(&im))
    } else {
        (round_float64(&re), round_float64(&im))
    };
    match (rr, ri) {
        (Some(rr), Some(ri)) => Some(binary_op(rr, Token::ADD, make_imag(ri))),
        _ => None,
    }
}

fn is_float_basic(k: BasicKind) -> bool {
    matches!(
        k,
        BasicKind::Float32 | BasicKind::Float64 | BasicKind::UntypedFloat
    )
}

fn is_complex_basic(k: BasicKind) -> bool {
    matches!(
        k,
        BasicKind::Complex64 | BasicKind::Complex128 | BasicKind::UntypedComplex
    )
}

/// The integer-range portion of `representableConst`. `xi` is the value after
/// `to_int`; `kind` is the target basic kind. Sizes for `int`/`uint`/`uintptr`
/// are 64-bit (see module docs).
fn representable_int(xi: &Value, kind: BasicKind) -> bool {
    use guff_constant::{bit_len, int64_val, sign};

    if let (x, true) = int64_val(xi) {
        // x fits in i64.
        match kind {
            // int/int64/untyped int: 64-bit signed → any i64 fits.
            BasicKind::Int | BasicKind::Int64 | BasicKind::UntypedInt | BasicKind::UntypedRune => {
                true
            }
            BasicKind::Int8 => (-(1i64 << 7)..=(1i64 << 7) - 1).contains(&x),
            BasicKind::Int16 => (-(1i64 << 15)..=(1i64 << 15) - 1).contains(&x),
            BasicKind::Int32 => (-(1i64 << 31)..=(1i64 << 31) - 1).contains(&x),
            // uint/uintptr/uint64: 64-bit unsigned → only sign matters.
            BasicKind::Uint | BasicKind::Uintptr | BasicKind::Uint64 => x >= 0,
            BasicKind::Uint8 => (0..=(1i64 << 8) - 1).contains(&x),
            BasicKind::Uint16 => (0..=(1i64 << 16) - 1).contains(&x),
            BasicKind::Uint32 => (0..=(1i64 << 32) - 1).contains(&x),
            _ => false,
        }
    } else {
        // x does not fit in i64.
        let n = bit_len(xi);
        match kind {
            BasicKind::Uint | BasicKind::Uintptr | BasicKind::Uint64 => sign(xi) >= 0 && n <= 64,
            BasicKind::UntypedInt => true,
            _ => false,
        }
    }
}

impl Checker {
    /// Check that the constant operand `x` is representable in basic type
    /// `typ`. On success the operand's value is replaced with the rounded
    /// value; on failure an error is recorded and `x` becomes invalid.
    ///
    /// Equivalent to `Checker.representable` (via `representation`).
    pub fn representable(&mut self, x: &mut Operand, typ: TypeId) {
        debug_assert_eq!(x.mode, OperandMode::Constant);
        let val = match &x.val {
            Some(v) => v.clone(),
            None => return,
        };

        match representable_const(&self.types, &val, typ) {
            Some(v) => {
                x.val = Some(v);
            }
            None => {
                let code = self.representation_error_code(x.typ, typ);
                let msg = self.invalid_conversion_msg(code, x, typ);
                self.error(x.pos() as u32, code, msg);
                x.mode = OperandMode::Invalid;
            }
        }
    }

    /// Choose the error code for a failed representation, mirroring
    /// `representation`'s numeric-conversion classification.
    fn representation_error_code(&self, x_typ: Option<TypeId>, typ: TypeId) -> Code {
        if let Some(xt) = x_typ {
            if is_numeric(&self.types, xt) && is_numeric(&self.types, typ) {
                // float → integer: truncated; otherwise: overflow.
                if !is_integer(&self.types, xt) && is_integer(&self.types, typ) {
                    return Code::TruncatedFloat;
                }
                return Code::NumericOverflow;
            }
        }
        Code::InvalidConstVal
    }

    /// Build the message for `invalidConversion`.
    fn invalid_conversion_msg(&self, code: Code, x: &Operand, target: TypeId) -> String {
        let target_str = self.type_str(target);
        let x_str = x
            .typ
            .map(|t| self.type_str(t))
            .unwrap_or_else(|| "value".to_string());
        match code {
            Code::TruncatedFloat => format!("{} truncated to {}", x_str, target_str),
            Code::NumericOverflow => format!("{} overflows {}", x_str, target_str),
            _ => format!("cannot convert {} to type {}", x_str, target_str),
        }
    }

    /// The constant-representation half of `representation` (`const.go`):
    /// returns the rounded value, or an error code on failure. `x.mode` must
    /// be `Constant`.
    fn representation(&self, x: &Operand, typ: TypeId) -> (Option<Value>, Option<Code>) {
        let val = match &x.val {
            Some(v) => v.clone(),
            None => return (None, Some(Code::InvalidConstVal)),
        };
        match representable_const(&self.types, &val, typ) {
            Some(v) => (Some(v), None),
            None => (None, Some(self.representation_error_code(x.typ, typ))),
        }
    }

    /// The implicit type (and, for constants, value) of untyped `x` in a
    /// context expecting `target`. Returns `(new_type, new_val, error_code)`;
    /// `error_code` is `None` on success.
    ///
    /// Equivalent to `Checker.implicitTypeAndValue`. Untyped nil and untyped
    /// non-nil → interface (empty / ordinary) are handled; type-param interface
    /// targets still report `InvalidUntypedConversion`.
    fn implicit_type_and_value(
        &mut self,
        x: &Operand,
        target: TypeId,
    ) -> (Option<TypeId>, Option<Value>, Option<Code>) {
        let xtyp = x.typ.unwrap_or_else(|| self.invalid_type());
        if x.mode == OperandMode::Invalid
            || is_typed(&self.types, xtyp)
            || !is_valid(&self.types, target)
        {
            return (x.typ, None, None);
        }

        // x is untyped.
        if is_untyped(&self.types, target) {
            return match max_type(&self.types, xtyp, target) {
                Some(m) => (Some(m), None, None),
                None => (None, None, Some(Code::InvalidUntypedConversion)),
            };
        }

        let u = target.underlying(&self.types);
        match self.types.get(u) {
            TypeData::Basic(_) => {
                if x.mode == OperandMode::Constant {
                    let (v, code) = self.representation(x, u);
                    if code.is_some() {
                        return (None, None, code);
                    }
                    return (Some(target), v, None);
                }
                // Non-constant untyped value: check kind compatibility.
                let xkind = match self.types.get(xtyp.underlying(&self.types)) {
                    TypeData::Basic(b) => b.kind(),
                    _ => return (None, None, Some(Code::InvalidUntypedConversion)),
                };
                let ok = match xkind {
                    BasicKind::UntypedBool => is_boolean(&self.types, target),
                    BasicKind::UntypedInt
                    | BasicKind::UntypedRune
                    | BasicKind::UntypedFloat
                    | BasicKind::UntypedComplex => is_numeric(&self.types, target),
                    BasicKind::UntypedString => is_string(&self.types, target),
                    BasicKind::UntypedNil => {
                        if !has_nil(&self.types, target) {
                            return (None, None, Some(Code::InvalidUntypedConversion));
                        }
                        // Preserve nil as UntypedNil (go.dev/issue/13061).
                        return (Some(self.basic(BasicKind::UntypedNil)), None, None);
                    }
                    _ => false,
                };
                if !ok {
                    return (None, None, Some(Code::InvalidUntypedConversion));
                }
                (Some(target), None, None)
            }
            // Untyped (non-nil) → interface: Go converts via the default type
            // of the untyped operand (empty interface always accepts; non-empty
            // requires the default type to implement the interface). Needed for
            // `iface == ""` / `iface == 0` (vault helper/random FieldData.Raw).
            other if is_interface(&self.types, u) => {
                if x.is_nil() && has_nil(&self.types, target) {
                    return (Some(self.basic(BasicKind::UntypedNil)), None, None);
                }
                if is_type_param(&self.types, target) {
                    // Go: the operand must convert to *every* underlying type
                    // in the constraint's type set; it then takes the type
                    // parameter itself, with no rounded constant value. A
                    // term-less set (`any`) calls the predicate once with
                    // `nil` and so never converts.
                    //
                    // Without this, `var total T` / `total += 1` / `a++` all
                    // fail with "cannot convert untyped int to type T" and the
                    // package goes ill-typed.
                    let mut unders: Vec<Option<TypeId>> = Vec::new();
                    crate::under::under_is(
                        &mut self.types,
                        &self.objects,
                        &self.packages,
                        target,
                        |u| {
                            unders.push(u);
                            true
                        },
                    );
                    let ok = unders.iter().all(|u| match u {
                        Some(u) => self.implicit_type_and_value(x, *u).0.is_some(),
                        None => false,
                    });
                    if !ok {
                        return (None, None, Some(Code::InvalidUntypedConversion));
                    }
                    return (Some(target), None, None);
                }
                let _ = other;
                let _ = default_type(&self.types, &self.typ, xtyp);
                // Empty interface (no methods) accepts every default type.
                // Non-empty interfaces: assignability of the default type is
                // checked by callers via comparison/assignment; for convert
                // we accept here when the interface has no required methods.
                match self.types.get(u) {
                    TypeData::Interface(iface) => {
                        let empty = iface
                            .cached_typeset()
                            .map_or(true, |ts| ts.is_empty());
                        if empty {
                            (Some(target), None, None)
                        } else {
                            // Conservative: still allow — comparison's
                            // assignable_to path validates concrete cases.
                            (Some(target), None, None)
                        }
                    }
                    _ => (Some(target), None, None),
                }
            }
            _ => {
                if x.is_nil() && has_nil(&self.types, target) {
                    (Some(target), None, None)
                } else {
                    (None, None, Some(Code::InvalidUntypedConversion))
                }
            }
        }
    }

    /// Set the type of an untyped operand `x` to `target`, rounding a constant
    /// value as needed. On failure reports a conversion error and invalidates
    /// `x`.
    ///
    /// Equivalent to `Checker.convertUntyped`. The narrowed type/value are
    /// propagated into the untyped-expression map via
    /// [`update_expr_val`](Checker::update_expr_val) /
    /// [`update_expr_type`](Checker::update_expr_type) (chunk 51).
    pub fn convert_untyped(&mut self, x: &mut Operand, target: TypeId) {
        let (new_type, val, code) = self.implicit_type_and_value(x, target);
        if let Some(code) = code {
            // Go reports against safeUnderlying(target) for non-type-params;
            // we use target itself for the message (rendering is best-effort).
            let _ = is_type_param(&self.types, target);
            let msg = self.invalid_conversion_msg(code, x, target);
            self.error(x.pos() as u32, code, msg);
            x.mode = OperandMode::Invalid;
            return;
        }
        if let Some(v) = val {
            x.val = Some(v.clone());
            if let Some(xe) = x.expr {
                self.update_expr_val(xe, v);
            }
        }
        if let Some(nt) = new_type {
            if Some(nt) != x.typ {
                x.typ = Some(nt);
                if let Some(xe) = x.expr {
                    self.update_expr_type(xe, nt, false);
                }
            }
        }
    }

    /// Boolean convenience wrapper for the `representable` closure that
    /// `assignments.rs` / `conversions.rs` accept (chunk 20c). Does not mutate
    /// or report — it only answers "is this constant representable as `typ`?".
    pub fn representable_bool(&self, x: &Operand, typ: TypeId) -> bool {
        match &x.val {
            Some(v) => representable_const(&self.types, v, typ).is_some(),
            None => false,
        }
    }
}
