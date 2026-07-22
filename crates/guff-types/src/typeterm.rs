//! Port of `cmd/compile/internal/types2/typeterm.go`.
//!
//! A [`Term`] is an internal building block of the type-set algebra. Four
//! shapes:
//!
//! | Go               | Rust                                  | Meaning                       |
//! | ---------------- | ------------------------------------- | ----------------------------- |
//! | `(*term)(nil)`   | `None: Option<Term>`                  | ∅ (empty set)                 |
//! | `&term{}`        | `Some(Term { tilde: _, typ: None })`  | 𝓤 (universe)                  |
//! | `&term{false,T}` | `Some(Term { tilde: false, typ: Some(T) })` | `{T}` — set of one type |
//! | `&term{true,t}`  | `Some(Term { tilde: true,  typ: Some(t) })` | `{x | under(x) == t}`   |
//!
//! Term operations live as free functions on `Option<Term>` (so the `None` ⇒
//! ∅ shape is first-class). Type identity uses
//! [`predicates::identical`](crate::predicates::identical) (D01), matching Go's
//! `Identical`.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeId};
use crate::predicates::identical;

/// Internal term (see module docs for the four shapes).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Term {
    pub(crate) tilde: bool,
    /// `None` for the universe term 𝓤; `Some(typ)` for {T} or {~t}.
    pub(crate) typ: Option<TypeId>,
}

impl Term {
    /// The universe term 𝓤. `tilde` doesn't matter; we pick `false`.
    pub(crate) const fn universe() -> Self {
        Self {
            tilde: false,
            typ: None,
        }
    }

    pub(crate) const fn single(typ: TypeId) -> Self {
        Self {
            tilde: false,
            typ: Some(typ),
        }
    }

    pub(crate) const fn tilde(typ: TypeId) -> Self {
        Self {
            tilde: true,
            typ: Some(typ),
        }
    }
}

/// Reports whether `x` and `y` represent the same type set.
///
/// Equivalent to `term.equal`.
pub(crate) fn equal(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    y: Option<Term>,
) -> bool {
    match (x, y) {
        (None, None) => true,
        (None, _) | (_, None) => false,
        (Some(a), Some(b)) => match (a.typ, b.typ) {
            (None, None) => true,           // 𝓤 == 𝓤
            (None, _) | (_, None) => false, // 𝓤 ≠ {T}
            (Some(at), Some(bt)) => {
                a.tilde == b.tilde && identical(arena, oarena, parena, at, bt)
            }
        },
    }
}

/// Returns `x ∪ y` as zero, one, or two terms.
///
/// Equivalent to `term.union`. The result has two slots; the second is
/// `None` if the union collapses to a single term.
pub(crate) fn union(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    y: Option<Term>,
) -> (Option<Term>, Option<Term>) {
    // Easy cases involving ∅ and 𝓤.
    match (x, y) {
        (None, None) => return (None, None), // ∅ ∪ ∅ == ∅
        (None, _) => return (y, None),       // ∅ ∪ y == y
        (_, None) => return (x, None),       // x ∪ ∅ == x
        (Some(xt), _) if xt.typ.is_none() => return (x, None), // 𝓤 ∪ y == 𝓤
        (_, Some(yt)) if yt.typ.is_none() => return (y, None), // x ∪ 𝓤 == 𝓤
        _ => {}
    }
    // Both terms are {T} or {~t}.
    if disjoint(arena, oarena, parena, x, y) {
        return (x, y);
    }
    // Same typ — choose the more permissive (tilde-bearing) form.
    let xt = x.unwrap();
    let yt = y.unwrap();
    if xt.tilde || !yt.tilde {
        (x, None)
    } else {
        (y, None)
    }
}

/// Returns `x ∩ y`.
///
/// Equivalent to `term.intersect`.
pub(crate) fn intersect(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    y: Option<Term>,
) -> Option<Term> {
    match (x, y) {
        (None, _) | (_, None) => return None, // ∅ ∩ y == ∅, x ∩ ∅ == ∅
        (Some(xt), _) if xt.typ.is_none() => return y, // 𝓤 ∩ y == y
        (_, Some(yt)) if yt.typ.is_none() => return x, // x ∩ 𝓤 == x
        _ => {}
    }
    if disjoint(arena, oarena, parena, x, y) {
        return None;
    }
    let xt = x.unwrap();
    let yt = y.unwrap();
    // ~t ∩ ~t == ~t; ~t ∩ T == T; T ∩ ~t == T; T ∩ T == T.
    if !xt.tilde || yt.tilde {
        x
    } else {
        y
    }
}

/// Reports whether `t ∈ x`.
///
/// Equivalent to `term.includes`.
pub(crate) fn includes(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    t: TypeId,
) -> bool {
    match x {
        None => false,
        Some(xt) => match xt.typ {
            None => true, // 𝓤 contains everything.
            Some(xt_typ) => {
                let u = if xt.tilde { t.underlying(arena) } else { t };
                identical(arena, oarena, parena, xt_typ, u)
            }
        },
    }
}

/// Reports whether `x ⊆ y`.
///
/// Equivalent to `term.subsetOf`.
pub(crate) fn subset_of(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    y: Option<Term>,
) -> bool {
    match (x, y) {
        (None, _) => return true,                          // ∅ ⊆ y == true
        (_, None) => return false, // x ⊆ ∅ == false (x is not ∅ by prev arm)
        (_, Some(yt)) if yt.typ.is_none() => return true, // x ⊆ 𝓤 == true
        (Some(xt), _) if xt.typ.is_none() => return false, // 𝓤 ⊆ y, y ≠ 𝓤
        _ => {}
    }
    if disjoint(arena, oarena, parena, x, y) {
        return false;
    }
    let xt = x.unwrap();
    let yt = y.unwrap();
    // ~t ⊆ ~t == true; ~t ⊆ T == false; T ⊆ ~t == true; T ⊆ T == true.
    !xt.tilde || yt.tilde
}

/// Reports whether `x ∩ y == ∅`. `x.typ` and `y.typ` must both be `Some`
/// (caller's responsibility — Go's `disjoint` has the same precondition).
///
/// Equivalent to `term.disjoint`.
pub(crate) fn disjoint(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    x: Option<Term>,
    y: Option<Term>,
) -> bool {
    let xt = x.expect("disjoint: x must not be ∅");
    let yt = y.expect("disjoint: y must not be ∅");
    let x_typ = xt.typ.expect("disjoint: x.typ must not be None");
    let y_typ = yt.typ.expect("disjoint: y.typ must not be None");
    let ux = if yt.tilde {
        x_typ.underlying(arena)
    } else {
        x_typ
    };
    let uy = if xt.tilde {
        y_typ.underlying(arena)
    } else {
        y_typ
    };
    !identical(arena, oarena, parena, ux, uy)
}
