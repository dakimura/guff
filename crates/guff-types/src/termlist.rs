//! Port of `cmd/compile/internal/types2/termlist.go`.
//!
//! A [`TermList`] represents the type set `t1 ∪ t2 ∪ … ∪ tn` of its terms.
//! A list is in *normal form* if all terms are disjoint; [`norm`] computes
//! that form. Operations don't require their operands to be normalised, but
//! many return normalised results.
//!
//! Term identity uses [`predicates::identical`](crate::predicates::identical)
//! via [`typeterm`] (D01).

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeId};
use crate::typeterm::{self, Term};

/// A list of terms. `None` slots represent the ∅ term (so a list of all
/// `None`s is the empty set).
pub(crate) type TermList = Vec<Option<Term>>;

/// `[𝓤]` — the singleton list representing the set of all types. Already
/// in normal form.
pub(crate) fn all_termlist() -> TermList {
    vec![Some(Term::universe())]
}

/// Reports whether `xl` is the empty set of types (all slots are ∅).
pub(crate) fn is_empty(xl: &TermList) -> bool {
    xl.iter().all(|x| x.is_none())
}

/// Reports whether `xl` is the set of all types (some slot is 𝓤).
pub(crate) fn is_all(xl: &TermList) -> bool {
    xl.iter().any(|x| matches!(x, Some(t) if t.typ.is_none()))
}

/// Returns the normal form of `xl` — a list of pairwise-disjoint terms with
/// no `None` slots. Quadratic, matching Go.
pub(crate) fn norm(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
) -> TermList {
    let mut used = vec![false; xl.len()];
    let mut rl: TermList = Vec::new();
    for i in 0..xl.len() {
        if xl[i].is_none() || used[i] {
            continue;
        }
        let mut xi = xl[i];
        for j in (i + 1)..xl.len() {
            if xl[j].is_none() || used[j] {
                continue;
            }
            let (u1, u2) = typeterm::union(arena, oarena, parena, xi, xl[j]);
            if u2.is_none() {
                // Hit 𝓤? whole list collapses to universe.
                if let Some(t) = u1 {
                    if t.typ.is_none() {
                        return all_termlist();
                    }
                }
                xi = u1;
                used[j] = true;
            }
        }
        rl.push(xi);
    }
    rl
}

/// `xl ∪ yl`.
pub(crate) fn union(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    yl: &TermList,
) -> TermList {
    let mut combined = Vec::with_capacity(xl.len() + yl.len());
    combined.extend_from_slice(xl);
    combined.extend_from_slice(yl);
    norm(arena, oarena, parena, &combined)
}

/// `xl ∩ yl`. Quadratic, matching Go.
pub(crate) fn intersect(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    yl: &TermList,
) -> TermList {
    if is_empty(xl) || is_empty(yl) {
        return Vec::new();
    }
    let mut rl: TermList = Vec::new();
    for &x in xl {
        for &y in yl {
            let r = typeterm::intersect(arena, oarena, parena, x, y);
            if r.is_some() {
                rl.push(r);
            }
        }
    }
    norm(arena, oarena, parena, &rl)
}

/// Reports whether `xl` and `yl` represent the same type set.
pub(crate) fn equal(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    yl: &TermList,
) -> bool {
    subset_of(arena, oarena, parena, xl, yl) && subset_of(arena, oarena, parena, yl, xl)
}

/// Reports whether `t ∈ xl`.
pub(crate) fn includes(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    t: TypeId,
) -> bool {
    xl.iter()
        .any(|&x| typeterm::includes(arena, oarena, parena, x, t))
}

/// Reports whether `y ⊆ xl` — some term in `xl` contains `y`.
pub(crate) fn superset_of(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    y: Option<Term>,
) -> bool {
    xl.iter()
        .any(|&x| typeterm::subset_of(arena, oarena, parena, y, x))
}

/// Reports whether `xl ⊆ yl`.
pub(crate) fn subset_of(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xl: &TermList,
    yl: &TermList,
) -> bool {
    if is_empty(yl) {
        return is_empty(xl);
    }
    xl.iter()
        .all(|&x| superset_of(arena, oarena, parena, yl, x))
}
