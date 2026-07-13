//! Port of `cmd/compile/internal/types2/conversions.go`.
//!
//! This file implements the *structural* core of Go's conversion checking:
//! [`convertible_to`] — "is `T(x)` a valid conversion?" — plus the four small
//! `is_*` shape predicates it relies on.
//!
//! ## Decoupling from `Checker`
//!
//! Go's `convertibleTo` opens with `if x.assignableTo(check, T, ...) { return
//! true }`. `operand.assignableTo` is Checker-dependent (`Checker.implements`,
//! `implicitTypeAndValue`, …) and is still deferred (see `operand.rs`
//! forward-pointer + `assignments.go`, a later chunk). To keep this port
//! purely structural and decoupled — matching the convention used across
//! chunks 10–14 — the assignability decision is **injected** as a closure:
//!
//! ```ignore
//! convertible_to(arena, oarena, parena, &x, target, &|x, t| { /* assignable? */ })
//! ```
//!
//! Callers that haven't ported `assignableTo` yet may pass
//! `&|_, _| false`; the structural rules below still answer correctly for
//! all conversions that don't hinge on assignability (numeric, string ⇄
//! bytes/runes, pointer, unsafe.Pointer, slice→array, and the generic
//! type-parameter cases). Interface-implements and named-type-assignable
//! conversions require a real `assignable_to` and light up once it lands.
//!
//! ## `check == nil` semantics
//!
//! Go gates slice→array (`go1.20`) and slice→array-pointer (`go1.17`)
//! conversions behind `check == nil || check.allowVersion(...)`. We are
//! Checker-less here, which is exactly Go's `check == nil` ("exported API
//! call, all methods type-checked") path — so those conversions are always
//! permitted, with no version error produced.
//!
//! ## Deferred to Tier 4 (`Checker`)
//!
//! `Checker.conversion(x, T)` — the driver that mutates the operand in place,
//! evaluates constant conversions via `representableConst`, emits
//! `InvalidConversion` errors, and calls `updateExprType` — is **not** ported
//! here. It pulls in `Checker.errorf`/`sprintf`/`updateExprType` and the
//! constant machinery, none of which exists until the Checker chunk. See the
//! forward-pointer comment at the bottom of this file.

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use crate::array::array_elem;
use crate::basic::BasicKind;
use crate::interface::interface_compute_typeset;
use crate::operand::Operand;
use crate::pointer::pointer_elem;
use crate::predicates::{
    identical, identical_with, is_complex, is_integer, is_integer_or_float, is_string,
    is_type_param, IdenticalCfg,
};
use crate::slice::slice_elem;
use crate::typeparam::type_param_iface;
use crate::typeset::TypeSet;

/// Reports whether the underlying type of `typ` is the predeclared
/// `uintptr` basic type.
///
/// Equivalent to `isUintptr`.
pub fn is_uintptr(arena: &TypeArena, typ: TypeId) -> bool {
    let u = typ.underlying(arena);
    matches!(arena.get(u), TypeData::Basic(b) if b.kind() == BasicKind::Uintptr)
}

/// Reports whether the underlying type of `typ` is `unsafe.Pointer`.
///
/// Equivalent to `isUnsafePointer`.
pub fn is_unsafe_pointer(arena: &TypeArena, typ: TypeId) -> bool {
    let u = typ.underlying(arena);
    matches!(arena.get(u), TypeData::Basic(b) if b.kind() == BasicKind::UnsafePointer)
}

/// Reports whether the underlying type of `typ` is a pointer type.
///
/// Equivalent to `isPointer`.
pub fn is_pointer(arena: &TypeArena, typ: TypeId) -> bool {
    matches!(arena.get(typ.underlying(arena)), TypeData::Pointer(_))
}

/// Reports whether the underlying type of `typ` is a slice of `byte`
/// (`uint8`) or `rune` (`int32`).
///
/// Equivalent to `isBytesOrRunes`.
pub fn is_bytes_or_runes(arena: &TypeArena, typ: TypeId) -> bool {
    let u = typ.underlying(arena);
    if matches!(arena.get(u), TypeData::Slice(_)) {
        let elem_u = slice_elem(arena, u).underlying(arena);
        return matches!(
            arena.get(elem_u),
            TypeData::Basic(b) if b.kind() == BasicKind::Uint8 || b.kind() == BasicKind::Int32
        );
    }
    false
}

/// Reports whether `T(x)` is a valid conversion.
///
/// `assignable_to(x, t)` must report whether operand `x` is assignable to
/// type `t` (Go's `operand.assignableTo`, ignoring the returned error code).
/// See the module docs for why this is injected rather than called directly.
///
/// Equivalent to `(x *operand) convertibleTo`. The `*cause` out-parameter is
/// dropped — it only feeds error messages, which belong to the Checker chunk.
pub fn convertible_to(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: &Operand,
    target: TypeId,
    assignable_to: &dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, &Operand, TypeId) -> bool,
) -> bool {
    // "x is assignable to T"
    if assignable_to(arena, oarena, parena, x, target) {
        return true;
    }

    // x.typ should be set by the time a conversion is checked; if it isn't
    // we can't say anything structural, so refuse defensively. (Go assumes
    // x.typ != nil here.)
    let x_typ = match x.typ {
        Some(t) => t,
        None => return false,
    };

    let v = crate::alias::unalias(arena, x_typ);
    let t = crate::alias::unalias(arena, target);
    let vu = v.underlying(arena);
    let tu = t.underlying(arena);
    let vp_is_tparam = is_type_param(arena, v);
    let tp_is_tparam = is_type_param(arena, t);

    // "V and T have identical underlying types if tags are ignored
    // and V and T are not type parameters"
    let ignore_tags = IdenticalCfg {
        ignore_tags: true,
        ignore_invalids: false,
    };
    if !vp_is_tparam && !tp_is_tparam && identical_with(arena, oarena, parena, vu, tu, ignore_tags)
    {
        return true;
    }

    // "V and T are unnamed pointer types and their pointer base types
    // have identical underlying types if tags are ignored
    // and their pointer base types are not type parameters"
    if matches!(arena.get(v), TypeData::Pointer(_)) && matches!(arena.get(t), TypeData::Pointer(_))
    {
        let vbase = pointer_elem(arena, v);
        let tbase = pointer_elem(arena, t);
        let vbase_u = vbase.underlying(arena);
        let tbase_u = tbase.underlying(arena);
        if !is_type_param(arena, vbase)
            && !is_type_param(arena, tbase)
            && identical_with(arena, oarena, parena, vbase_u, tbase_u, ignore_tags)
        {
            return true;
        }
    }

    // "V and T are both integer or floating point types"
    if is_integer_or_float(arena, vu) && is_integer_or_float(arena, tu) {
        return true;
    }

    // "V and T are both complex types"
    if is_complex(arena, vu) && is_complex(arena, tu) {
        return true;
    }

    // "V is an integer or a slice of bytes or runes and T is a string type"
    if (is_integer(arena, vu) || is_bytes_or_runes(arena, vu)) && is_string(arena, tu) {
        return true;
    }

    // "V is a string and T is a slice of bytes or runes"
    if is_string(arena, vu) && is_bytes_or_runes(arena, tu) {
        return true;
    }

    // package unsafe:
    // "any pointer or value of underlying type uintptr can be converted into a unsafe.Pointer"
    if (is_pointer(arena, vu) || is_uintptr(arena, vu)) && is_unsafe_pointer(arena, tu) {
        return true;
    }
    // "and vice versa"
    if is_unsafe_pointer(arena, vu) && (is_pointer(arena, tu) || is_uintptr(arena, tu)) {
        return true;
    }

    // "V is a slice, T is an array or pointer-to-array type,
    // and the slice and array types have identical element types."
    //
    // We are Checker-less (= Go's `check == nil`), so the go1.20 / go1.17
    // version gates are always satisfied.
    if matches!(arena.get(vu), TypeData::Slice(_)) {
        let s_elem = slice_elem(arena, vu);
        if matches!(arena.get(tu), TypeData::Array(_)) {
            let a_elem = array_elem(arena, tu);
            if identical(arena, oarena, parena, s_elem, a_elem) {
                return true;
            }
        } else if matches!(arena.get(tu), TypeData::Pointer(_)) {
            let base_u = pointer_elem(arena, tu).underlying(arena);
            if matches!(arena.get(base_u), TypeData::Array(_)) {
                let a_elem = array_elem(arena, base_u);
                if identical(arena, oarena, parena, s_elem, a_elem) {
                    return true;
                }
            }
        }
    }

    // optimization: if we don't have type parameters, we're done
    if !vp_is_tparam && !tp_is_tparam {
        return false;
    }

    // generic cases with specific type terms
    // (generic operands cannot be constants, so we can ignore x.val)
    match (vp_is_tparam, tp_is_tparam) {
        (true, true) => {
            let v_terms = tparam_terms(arena, oarena, parena, v);
            let t_terms = tparam_terms(arena, oarena, parena, t);
            // `Vp.is`: all V terms must hold; "no specific types" ⇒ false.
            v_terms.iter().all(|&(_, v_typ)| {
                let Some(vt) = v_typ else { return false };
                let mut xx = x.clone(); // don't clobber outer x
                xx.typ = Some(vt);
                t_terms.iter().all(|&(_, t_typ)| {
                    let Some(tt) = t_typ else { return false };
                    convertible_to(arena, oarena, parena, &xx, tt, assignable_to)
                })
            })
        }
        (true, false) => {
            let v_terms = tparam_terms(arena, oarena, parena, v);
            v_terms.iter().all(|&(_, v_typ)| {
                let Some(vt) = v_typ else { return false };
                let mut xx = x.clone(); // don't clobber outer x
                xx.typ = Some(vt);
                convertible_to(arena, oarena, parena, &xx, target, assignable_to)
            })
        }
        (false, true) => {
            let t_terms = tparam_terms(arena, oarena, parena, t);
            t_terms.iter().all(|&(_, t_typ)| {
                let Some(tt) = t_typ else { return false };
                convertible_to(arena, oarena, parena, x, tt, assignable_to)
            })
        }
        (false, false) => false,
    }
}

/// Collect the type-set terms of a TypeParam as `(tilde, typ)` pairs.
///
/// Mirrors Go's `Vp.is(func(t *term) ...)` iteration: a single
/// `(false, None)` entry stands in for "the type set has no specific type
/// terms" (Go's `t == nil` callback), which callers treat as a failure.
pub(crate) fn tparam_terms(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    tp: TypeId,
) -> Vec<(bool, Option<TypeId>)> {
    let iface = type_param_iface(arena, oarena, parena, tp);
    interface_compute_typeset(arena, oarena, parena, iface);
    let snapshot: TypeSet = match arena.get(iface) {
        TypeData::Interface(i) => i.tset.as_ref().expect("computed above").clone(),
        _ => unreachable!("type_param_iface returns an Interface"),
    };
    let mut terms: Vec<(bool, Option<TypeId>)> = Vec::new();
    snapshot.is(|tilde, typ| {
        terms.push((tilde, typ));
        true
    });
    terms
}

// ---------------------------------------------------------------------------
// Forward pointer for the Checker chunk (Tier 4) — `Checker.conversion`.
//
// Go's `(check *Checker) conversion(x *operand, T Type)` is the in-place
// conversion driver. It is NOT ported here because it needs:
//
//   - `representableConst` + `constant.*` for the constant-conversion path,
//   - `check.errorf(x, InvalidConversion, ...)` for diagnostics,
//   - `check.updateExprType(x.expr, final, true)` for untyped-result typing,
//   - `check.allowVersion(...)` (we approximate with the `check == nil` path
//     inside `convertible_to`).
//
// When lifted, it will live on the Checker and call `convertible_to` with a
// real `assignable_to` closure backed by `Checker.implements`. Sketch:
//
//     impl Checker {
//         fn conversion(&mut self, x: &mut Operand, t: TypeId) { ... }
//     }
