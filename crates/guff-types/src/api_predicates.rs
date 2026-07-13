//! Exported type predicates — port of `api_predicates.go`.
//!
//! These are the *public* type-relation predicates a consumer (e.g. a linter)
//! reaches for: `AssignableTo`, `ConvertibleTo`, `Implements`, `Satisfies`,
//! `AssertableTo`, plus `Identical` / `IdenticalIgnoreTags`.
//!
//! Go implements them by calling the Checker-bound `assignableTo` /
//! `convertibleTo` / `implements` / `newAssertableTo` through a *nil* Checker
//! (`(*Checker)(nil)`) — i.e. without a real Checker, so there is no error
//! recording and no `objDecl`. Our equivalents are arena-based free functions,
//! so these wrappers take the three arenas explicitly instead of a nil
//! Checker. To avoid name clashes with the lower-level operand-based
//! `assignable_to` / `convertible_to` (which take an `Operand`), the public
//! predicate API uses the `api_` prefix.
//!
//! `Identical` / `IdenticalIgnoreTags` map directly onto the already-exported
//! [`crate::predicates::identical`] / [`crate::predicates::identical_with`];
//! `api_identical` / `api_identical_ignore_tags` are thin aliases provided for
//! API-surface completeness.

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use crate::assignments::assignable_to as assignable_to_operand;
use crate::check_expr_const::representable_const;
use crate::check_lookup::{implements as implements_fn, missing_method};
use crate::conversions::convertible_to as convertible_to_operand;
use crate::interface::interface_empty;
use crate::lookup::has_invalid_embedded_fields;
use crate::operand::{Operand, OperandMode};
use crate::predicates::{identical, identical_with, is_interface, is_valid, IdenticalCfg};

/// The `implements` closure injected into the operand-based predicates: does
/// `v` implement interface `t` (non-constraint)? Mirrors `check_assign.rs`.
fn implements_closure(
    a: &mut TypeArena,
    o: &ObjectArena,
    p: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    implements_fn(a, o, p, v, t, false).is_ok()
}

/// The `representable` closure: is constant operand `x` representable as `t`?
fn representable_closure(a: &TypeArena, x: &Operand, t: TypeId) -> bool {
    match &x.val {
        Some(v) => representable_const(a, v, t).is_some(),
        None => false,
    }
}

/// A non-constant value operand of type `v` — Go's `operand{mode: value, typ: V}`.
fn value_operand(v: TypeId) -> Operand {
    Operand {
        mode: OperandMode::Value,
        typ: Some(v),
        ..Operand::default()
    }
}

/// `AssignableTo` reports whether a value of type `v` is assignable to a
/// variable of type `t`.
///
/// The behavior is unspecified if `v` or `t` is `Typ[Invalid]` or an
/// uninstantiated generic type. Equivalent to Go's `AssignableTo`.
pub fn api_assignable_to(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    let x = value_operand(v);
    // check not needed for non-constant x
    assignable_to_operand(
        types,
        oarena,
        parena,
        &x,
        t,
        &implements_closure,
        &representable_closure,
    )
    .ok
}

/// `ConvertibleTo` reports whether a value of type `v` is convertible to a
/// value of type `t`.
///
/// The behavior is unspecified if `v` or `t` is `Typ[Invalid]` or an
/// uninstantiated generic type. Equivalent to Go's `ConvertibleTo`.
pub fn api_convertible_to(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    let x = value_operand(v);
    let assignable =
        |a: &mut TypeArena, o: &ObjectArena, p: &PackageArena, x: &Operand, t: TypeId| {
            assignable_to_operand(a, o, p, x, t, &implements_closure, &representable_closure).ok
        };
    // check not needed for non-constant x
    convertible_to_operand(types, oarena, parena, &x, t, &assignable)
}

/// `Implements` reports whether type `v` implements interface `t`.
///
/// The behavior is unspecified if `v` is `Typ[Invalid]` or an uninstantiated
/// generic type. Equivalent to Go's `Implements`.
pub fn api_implements(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    // All types (even Typ[Invalid]) implement the empty interface.
    if interface_is_empty(types, oarena, parena, t) {
        return true;
    }
    // Checker.implements suppresses errors for invalid types, so we need
    // special handling here.
    let vu = v.underlying(types);
    if !is_valid(types, vu) {
        return false;
    }
    implements_fn(types, oarena, parena, v, t, false).is_ok()
}

/// `Satisfies` reports whether type `v` satisfies the constraint `t`.
///
/// The behavior is unspecified if `v` is `Typ[Invalid]` or an uninstantiated
/// generic type. Equivalent to Go's `Satisfies`.
pub fn api_satisfies(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    implements_fn(types, oarena, parena, v, t, true).is_ok()
}

/// `AssertableTo` reports whether a value of interface type `v` can be asserted
/// to have type `t`.
///
/// The behavior is unspecified if `t` is `Typ[Invalid]`, if `v` is a
/// generalized interface (one usable only as a type constraint), or if `t` is
/// an uninstantiated generic type. Equivalent to Go's `AssertableTo`.
pub fn api_assertable_to(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    // Checker.newAssertableTo suppresses errors for invalid types, so we need
    // special handling here.
    let tu = t.underlying(types);
    if !is_valid(types, tu) {
        return false;
    }
    // newAssertableTo: no static check is required if T is an interface
    // (the dynamic type is what is asserted).
    if is_interface(types, t) {
        return true;
    }
    // Otherwise T must have all of V's methods.
    has_all_methods(types, oarena, parena, t, v)
}

/// `Identical` reports whether `x` and `y` are identical types. Receivers of
/// `Signature` types are ignored. Thin alias over
/// [`crate::predicates::identical`].
pub fn api_identical(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
) -> bool {
    identical(types, oarena, parena, x, y)
}

/// `IdenticalIgnoreTags` reports whether `x` and `y` are identical types if
/// struct tags are ignored. Receivers of `Signature` types are ignored.
pub fn api_identical_ignore_tags(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: TypeId,
    y: TypeId,
) -> bool {
    identical_with(
        types,
        oarena,
        parena,
        x,
        y,
        IdenticalCfg {
            ignore_tags: true,
            ignore_invalids: false,
        },
    )
}

/// Reports whether `t`'s underlying type is the empty interface — Go's
/// `(*Interface).Empty()`. A non-interface `t` is not "empty".
fn interface_is_empty(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
) -> bool {
    let tu = t.underlying(types);
    if matches!(types.get(tu), TypeData::Interface(_)) {
        interface_empty(types, oarena, parena, tu)
    } else {
        false
    }
}

/// Reports whether every method of `t` is present on `v` — the pure-arena
/// equivalent of `Checker::has_all_methods` with `static_ = false`.
fn has_all_methods(
    types: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    v: TypeId,
    t: TypeId,
) -> bool {
    // We don't know anything about an invalid V — assume it implements T.
    if !is_valid(types, v) {
        return true;
    }
    match missing_method(types, oarena, parena, v, t, false) {
        None => true,
        // An invalid embedded field could hide the method — assume present.
        Some(_) => has_invalid_embedded_fields(types, oarena, v),
    }
}
