//! Port of `cmd/compile/internal/types2/under.go`.
//!
//! Tier 1 helpers used by every later checker file:
//!
//! - [`under_is`]   — `f(t.Underlying())` for non-TypeParam, else iterate the
//!                    TypeParam's typeset.
//! - [`all`]        — `f(t, u)` for each `(type, underlying)` pair in `t`'s
//!                    implied type set.
//! - [`typeset_iter`] — same as [`all`] but exposed as a higher-order iterator
//!                      (Go's `iter.Seq2`).
//! - [`TypeError`]  — a deferred-formatted error message, returned by helpers
//!                    that produce one error per (type, underlying) pair.
//! - [`type_errorf`] — convenience constructor.
//! - [`common_under`] — common underlying type of all types in a type set,
//!                      with channel-direction reconciliation.
//!
//! All entry points take `&mut TypeArena` + `&ObjectArena` + `&PackageArena`
//! because a TypeParam's typeset is computed lazily by
//! [`interface_compute_typeset`](crate::interface::interface_compute_typeset).
//! Non-TypeParam paths still take `&mut` for API uniformity; they don't
//! actually mutate.

use crate::alias::unalias_readonly;
use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use crate::interface::interface_compute_typeset;
use crate::predicates::identical;
use crate::typeparam::type_param_iface;

/// If `t` is a type parameter, [`under_is`] returns true iff `f(u)` is true
/// for every underlying type `u` in `t`'s typeset. Otherwise it returns
/// `f(t.Underlying())`.
///
/// Equivalent to `underIs`.
pub fn under_is(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
    mut f: impl FnMut(Option<TypeId>) -> bool,
) -> bool {
    all(arena, oarena, parena, t, |_, u| f(u))
}

/// Reports whether `f(t, u)` is true for every `(type, underlying)` pair in
/// the typeset implied by `t`.
///
/// - If `t` is a type parameter, the implied type set is the type set of
///   `t`'s constraint. With no specific terms, `f` is called once with
///   `(None, None)`.
/// - Otherwise the implied type set consists of just `t`, and `f` is called
///   once with `(Some(t), Some(t.underlying()))`.
///
/// In any case, `f` is guaranteed to be called at least once.
///
/// Equivalent to `all`.
pub fn all(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
    mut f: impl FnMut(Option<TypeId>, Option<TypeId>) -> bool,
) -> bool {
    let u = unalias_readonly(arena, t);
    if matches!(arena.get(u), TypeData::TypeParam(_)) {
        type_param_typeset(arena, oarena, parena, u, &mut f)
    } else {
        let und = t.underlying(arena);
        f(Some(t), Some(und))
    }
}

/// Iterator-style variant of [`all`] — the inverse of Go's
/// `iter.Seq2[Type, Type]`. The provided `yield` callback is called once per
/// `(type, underlying)` pair; returning `false` stops iteration.
///
/// Equivalent to `typeset`.
pub fn typeset_iter(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
    yield_: impl FnMut(Option<TypeId>, Option<TypeId>) -> bool,
) {
    let _ = all(arena, oarena, parena, t, yield_);
}

/// Internal: iterate the type set of a `TypeParam`'s constraint. Triggers
/// lazy computation of the constraint Interface's typeset.
fn type_param_typeset(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    tp_id: TypeId,
    f: &mut impl FnMut(Option<TypeId>, Option<TypeId>) -> bool,
) -> bool {
    // Resolve the constraint Interface (may wrap a non-interface bound and
    // memoise on the TypeParam).
    let iface = type_param_iface(arena, oarena, parena, tp_id);
    interface_compute_typeset(arena, oarena, parena, iface);

    // Snapshot terms so we don't hold an arena borrow across the callback.
    let (terms, has_terms) = match arena.get(iface) {
        TypeData::Interface(i) => {
            let ts = i
                .tset
                .as_ref()
                .expect("compute_interface_type_set seeded above");
            // Match Go's `_TypeSet.hasTerms`: non-empty AND non-all.
            let has = ts.num_terms() > 0 && !crate::termlist::is_all(&ts.terms);
            (ts.terms.clone(), has)
        }
        _ => unreachable!("type_param_iface returns Interface"),
    };

    if !has_terms {
        return f(None, None);
    }

    for term in terms.iter().flatten() {
        let typ = term.typ.expect("specific term has a typ");
        // Per Go: `Unalias(t.typ)` for tilde terms, then `Underlying()` for
        // non-tilde. Our `underlying()` already handles the alias case.
        let u = if term.tilde {
            unalias_readonly(arena, typ)
        } else {
            typ.underlying(arena)
        };
        if !f(Some(typ), Some(u)) {
            return false;
        }
    }
    true
}

// ----------------------------------------------------------------------------
// typeError

/// A deferred-formatted type error. Holds a format string + already-rendered
/// arguments so the eventual `Checker.sprintf` (or our fallback) can paste it
/// into a full error message.
///
/// Equivalent to `typeError`. Until the Checker lands, [`TypeError::format`]
/// performs a simple `format!`-style substitution by replacing each `%s` (in
/// order) with the corresponding pre-stringified arg.
#[derive(Debug, Clone, Default)]
pub struct TypeError {
    format_: String,
    args: Vec<String>,
}

impl TypeError {
    /// Constructor mirroring Go's `typeErrorf`. An empty `format` produces a
    /// canonical empty error (matches Go's `&emptyTypeError`).
    pub fn new(format: &str, args: Vec<String>) -> Self {
        if format.is_empty() {
            return Self::default();
        }
        Self {
            format_: format.to_string(),
            args,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.format_.is_empty() && self.args.is_empty()
    }

    /// Render this error to a string. Until the Checker is ported this just
    /// substitutes each `%s` in turn with the next pre-stringified arg —
    /// good enough for tests; the full `Checker.sprintf` (with `%T`, package
    /// qualifiers, etc.) lands with the Checker chunk.
    pub fn format(&self) -> String {
        let mut out = String::new();
        let mut args = self.args.iter();
        let mut chars = self.format_.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '%' {
                if let Some(&n) = chars.peek() {
                    if n == 's' {
                        chars.next();
                        if let Some(a) = args.next() {
                            out.push_str(a);
                            continue;
                        }
                    }
                }
            }
            out.push(c);
        }
        out
    }
}

/// Convenience constructor accepting any `Display` args.
///
/// Equivalent to `typeErrorf`.
pub fn type_errorf(format: &str, args: Vec<String>) -> TypeError {
    TypeError::new(format, args)
}

// ----------------------------------------------------------------------------
// commonUnder

/// If `t` is a type parameter, `cond` is `None`, and `t`'s type set contains
/// no channel types, [`common_under`] returns the common underlying type of
/// all types in `t`'s type set if it exists, or a [`TypeError`] otherwise.
///
/// If `t` is a type parameter, `cond` is `None`, and there are channel
/// types, `t`'s type set must only contain channel types; they must all
/// have the same element types; channel directions must not conflict; and
/// `common_under` returns one of the most restricted channels. Otherwise it
/// returns an error.
///
/// If `cond.is_some()`, each pair `(t, u)` in `t`'s type set must satisfy
/// the condition expressed by `cond`. If `cond` returns `Some(err)`,
/// `common_under` returns that error. `cond` is called before any other
/// conditions, and may be called with `(None, None)` if the type set has no
/// specific types.
///
/// If `t` is not a type parameter, `common_under` behaves as if `t` were a
/// type parameter with the single type `t` in its set.
///
/// Equivalent to `commonUnder`.
pub fn common_under(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
    mut cond: Option<&mut dyn FnMut(Option<TypeId>, Option<TypeId>) -> Option<TypeError>>,
) -> (Option<TypeId>, Option<TypeError>) {
    // ct/cu: the current "common" (type, underlying). `cu == None` ⇒ first
    // iteration not yet performed (matches Go's `cu == nil` sentinel).
    let mut ct: Option<TypeId> = None;
    let mut cu: Option<TypeId> = None;

    // We collect (t, u) pairs first so we don't have to hold a mutable
    // borrow across the (potentially mutating) `identical` call. `all` may
    // call the callback up to N times where N = number of terms; for
    // typical type sets this is small.
    let mut pairs: Vec<(Option<TypeId>, Option<TypeId>)> = Vec::new();
    all(arena, oarena, parena, t, |tt, uu| {
        pairs.push((tt, uu));
        true
    });

    for (tt, uu) in pairs {
        if let Some(ref mut f) = cond {
            if let Some(e) = f(tt, uu) {
                return (None, Some(e));
            }
        }

        let u = match uu {
            Some(u) => u,
            None => {
                return (None, Some(type_errorf("no specific type", Vec::new())));
            }
        };
        let this_t = tt.expect("tt is Some when uu is Some");

        // First iteration: just record.
        if cu.is_none() {
            ct = Some(this_t);
            cu = Some(u);
            continue;
        }
        let cu_id = cu.unwrap();
        let ct_id = ct.unwrap();

        // Channel-vs-channel reconciliation.
        if matches!(arena.get(cu_id), TypeData::Chan(_))
            && matches!(arena.get(u), TypeData::Chan(_))
        {
            let (chu_dir, chu_elem) = match arena.get(cu_id) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            let (ch_dir, ch_elem) = match arena.get(u) {
                TypeData::Chan(c) => (c.dir(), c.elem()),
                _ => unreachable!(),
            };
            if !identical(arena, oarena, parena, chu_elem, ch_elem) {
                return (
                    None,
                    Some(type_errorf(
                        "channels %s and %s have different element types",
                        vec![format!("type#{:?}", ct_id), format!("type#{:?}", this_t)],
                    )),
                );
            }
            use crate::chan::ChanDir::*;
            match (chu_dir, ch_dir) {
                (a, b) if a == b => {}
                (SendRecv, _) => {
                    // Keep restricted channel.
                    ct = Some(this_t);
                    cu = Some(u);
                }
                (_, SendRecv) => {
                    // cu already restricted; nothing to do.
                }
                _ => {
                    return (
                        None,
                        Some(type_errorf(
                            "channels %s and %s have conflicting directions",
                            vec![format!("type#{:?}", ct_id), format!("type#{:?}", this_t)],
                        )),
                    );
                }
            }
            continue;
        }

        // Otherwise the current type must share an underlying with all
        // previous types.
        if !identical(arena, oarena, parena, cu_id, u) {
            return (
                None,
                Some(type_errorf(
                    "%s and %s have different underlying types",
                    vec![format!("type#{:?}", ct_id), format!("type#{:?}", this_t)],
                )),
            );
        }
    }
    (cu, None)
}
