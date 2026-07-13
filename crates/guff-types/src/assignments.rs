//! Port of the assignability decision `(x *operand) assignableTo` — which Go
//! keeps in `operand.go` — plus a place to grow the rest of
//! `assignments.go` once the `Checker` exists.
//!
//! ## What is ported (chunk 16)
//!
//! [`assignable_to`] — "is `x` assignable to a variable of type `T`?" — the
//! structural core of Go's `operand.assignableTo`, returning `(bool,
//! Option<Code>)` (the `Code` is only meaningful when the bool is `false`;
//! `None` stands in for Go's zero `Code`).
//!
//! ## Decoupling from `Checker` (injected closures)
//!
//! Go's `assignableTo` reaches into the Checker in exactly two places, both
//! injected here as closures so this stays structural and Checker-free
//! (matching the convention from chunks 10–15):
//!
//! - **`implements(v, t)`** — does type `v` implement interface `t`? Go calls
//!   `check.implements`, which rests on `missingMethod` (a chunk-11
//!   deferral). Pass `&|_, _| false` until that lands.
//! - **`representable(x, t)`** — is untyped operand `x` representable as a
//!   value of type `t`? Go calls `check.implicitTypeAndValue(x, t)` and tests
//!   `newType != nil`. That needs the constant machinery + Checker, so it is
//!   injected too.
//!
//! The `*cause` error out-parameter is dropped (diagnostics belong to the
//! Checker chunk); the one place it changed control flow — the
//! "need type assertion" hint when `V` is an interface — is preserved for the
//! **boolean/code** result but without the message.
//!
//! ## What is deferred to Tier 4 (`Checker`)
//!
//! The bulk of `assignments.go` proper — `Checker.assignment`, `initConst`,
//! `initVar`, `lhsVar`, `assignVar`, `initVars`, `assignVars`, and the
//! mismatch-error helpers — is deeply Checker-bound (`check.expr`,
//! `check.errorf`, `check.recordDef`, `check.usedVars`, …) and lands with the
//! Checker. See the forward-pointer at the bottom of this file.

use guff_types_errors::Code;

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use crate::chan::{chan_dir, chan_elem, ChanDir};
use crate::conversions::tparam_terms;
use crate::lookup::is_interface_ptr;
use crate::operand::{Operand, OperandMode};
use crate::predicates::{has_name, identical, is_type_param, is_untyped, is_valid};

/// Result of [`assignable_to`]: assignability plus the error code that
/// explains a `false` result.
///
/// `code` is `None` when `ok` is `true` (Go's zero `Code`), and otherwise
/// carries the `internal/types/errors` code Go would report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignableResult {
    pub ok: bool,
    pub code: Option<Code>,
}

impl AssignableResult {
    fn ok() -> Self {
        Self {
            ok: true,
            code: None,
        }
    }
    fn no(code: Code) -> Self {
        Self {
            ok: false,
            code: Some(code),
        }
    }
}

/// Reports whether operand `x` is assignable to a variable of type `target`.
///
/// See the module docs for the injected `implements` / `representable`
/// closures. Equivalent to `(x *operand) assignableTo` (in Go's `operand.go`).
pub fn assignable_to(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: &Operand,
    target: TypeId,
    implements: &dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, TypeId, TypeId) -> bool,
    representable: &dyn Fn(&TypeArena, &Operand, TypeId) -> bool,
) -> AssignableResult {
    // An invalid operand or invalid target avoids spurious errors.
    if x.mode == OperandMode::Invalid || !is_valid(arena, target) {
        return AssignableResult::ok();
    }

    let x_typ = match x.typ {
        Some(t) => t,
        None => return AssignableResult::ok(), // invalid-ish; mirror "avoid spurious errors"
    };

    let v = crate::alias::unalias(arena, x_typ);
    let t = crate::alias::unalias(arena, target);

    // x's type is identical to T.
    if identical(arena, oarena, parena, v, t) {
        return AssignableResult::ok();
    }

    let vu = v.underlying(arena);
    let tu = t.underlying(arena);
    let vp = is_type_param(arena, v);
    let tp = is_type_param(arena, t);

    // x is an untyped value representable by a value of type T.
    if is_untyped(arena, vu) {
        debug_assert!(!vp, "untyped value cannot be a type parameter");
        if tp {
            // T is a type parameter: x must be representable by each specific
            // type in T's type set.
            let terms = tparam_terms(arena, oarena, parena, t);
            let ok = terms_all(&terms, |tt| representable(arena, x, tt));
            return AssignableResult {
                ok,
                code: if ok {
                    None
                } else {
                    Some(Code::IncompatibleAssign)
                },
            };
        }
        let ok = representable(arena, x, t);
        return AssignableResult {
            ok,
            code: if ok {
                None
            } else {
                Some(Code::IncompatibleAssign)
            },
        };
    }
    // Vu is typed.

    // V and T have identical underlying types, at least one is unnamed, and
    // neither is a type parameter.
    if !vp
        && !tp
        && identical(arena, oarena, parena, vu, tu)
        && (!has_name(arena, v) || !has_name(arena, t))
    {
        return AssignableResult::ok();
    }

    // T is an interface (not a type parameter) and V implements T; also the
    // pointer-to-interface case (for the would-be Checker.implements cause).
    if (matches!(arena.get(tu), TypeData::Interface(_)) && !tp) || is_interface_ptr(arena, tu) {
        if implements(arena, oarena, parena, v, t) {
            return AssignableResult::ok();
        }
        // V doesn't implement T; if V isn't a type parameter that's a hard no.
        if !vp {
            return AssignableResult::no(Code::InvalidIfaceAssign);
        }
        // else: fall through (a tparam V may still be assignable).
    }

    // If V is an interface, a missing type assertion may be the problem. This
    // is diagnostic in Go, but it does early-return false, so we keep the
    // control flow (without the "need type assertion" message).
    if matches!(arena.get(vu), TypeData::Interface(_))
        && !vp
        && implements(arena, oarena, parena, t, v)
    {
        return AssignableResult::no(Code::IncompatibleAssign);
    }

    // x is a bidirectional channel value, T is a channel with an identical
    // element type, and at least one of V or T is unnamed.
    if matches!(arena.get(vu), TypeData::Chan(_)) && chan_dir(arena, vu) == ChanDir::SendRecv {
        if matches!(arena.get(tu), TypeData::Chan(_)) {
            let v_elem = chan_elem(arena, vu);
            let t_elem = chan_elem(arena, tu);
            if identical(arena, oarena, parena, v_elem, t_elem) {
                let ok = !has_name(arena, v) || !has_name(arena, t);
                return AssignableResult {
                    ok,
                    code: if ok {
                        None
                    } else {
                        Some(Code::InvalidChanAssign)
                    },
                };
            }
        }
    }

    // optimization: no type parameters ⇒ done.
    if !vp && !tp {
        return AssignableResult::no(Code::IncompatibleAssign);
    }

    // V is not a named type and T is a type parameter: x must be assignable to
    // each specific type in T's type set.
    if !has_name(arena, v) && tp {
        let terms = tparam_terms(arena, oarena, parena, t);
        return assign_over_terms(
            arena,
            oarena,
            parena,
            &terms,
            implements,
            representable,
            |tt| (x.clone(), tt),
        );
    }

    // V is a type parameter and T is not a named type: each specific type in
    // V's type set must be assignable to T.
    if vp && !has_name(arena, t) {
        let terms = tparam_terms(arena, oarena, parena, v);
        return assign_over_terms(
            arena,
            oarena,
            parena,
            &terms,
            implements,
            representable,
            |vt| {
                let mut xx = x.clone(); // don't clobber outer x
                xx.typ = Some(vt);
                (xx, t)
            },
        );
    }

    AssignableResult::no(Code::IncompatibleAssign)
}

/// `true` iff every term has a specific type (`Some`) and `f` holds for it.
/// A `None` term (Go's "no specific types") makes the whole thing `false`.
fn terms_all(terms: &[(bool, Option<TypeId>)], mut f: impl FnMut(TypeId) -> bool) -> bool {
    for &(_, typ) in terms {
        match typ {
            None => return false,
            Some(t) => {
                if !f(t) {
                    return false;
                }
            }
        }
    }
    true
}

/// Drive the type-parameter recursion shared by the last two `assignableTo`
/// branches. For each term, `make` builds the `(operand, target)` pair to
/// recurse on; the first failing (or specifc-type-less) term short-circuits,
/// matching Go's `Tp.is`/`Vp.is` early-return semantics. Returns the last
/// evaluated `(ok, code)` — `(true, None)` when all terms pass.
fn assign_over_terms(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    terms: &[(bool, Option<TypeId>)],
    implements: &dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, TypeId, TypeId) -> bool,
    representable: &dyn Fn(&TypeArena, &Operand, TypeId) -> bool,
    mut make: impl FnMut(TypeId) -> (Operand, TypeId),
) -> AssignableResult {
    let mut result = AssignableResult::no(Code::IncompatibleAssign);
    for &(_, typ) in terms {
        match typ {
            None => {
                result = AssignableResult::no(Code::IncompatibleAssign);
                break;
            }
            Some(term_typ) => {
                let (xx, tgt) = make(term_typ);
                result = assignable_to(arena, oarena, parena, &xx, tgt, implements, representable);
                if !result.ok {
                    break;
                }
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Forward pointer for the Checker chunk (Tier 4) — the rest of assignments.go.
//
// `assignments.go` proper is the Checker's assignment driver and is NOT ported
// here. When the Checker lands it will own these, calling `assignable_to`
// with real `implements` (via `missingMethod`) and `representable` (via
// `implicitTypeAndValue`) closures:
//
//   - Checker.assignment(x, T, context)
//   - Checker.initConst / initVar / lhsVar / assignVar
//   - Checker.initVars / assignVars  (multi-assignment, tuple spreading)
//   - the assignment-count mismatch error helpers
//
// At that point `convertible_to`'s injected `assignable_to` closure can be
// backed by this function instead of the `&|_,_| false` stub.
