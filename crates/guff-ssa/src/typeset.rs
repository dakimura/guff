//! Type-set utilities.
//!
//! Port of go/ssa's `typeset.go`. These helpers reason about the *type set*
//! implied by a type — the set of concrete types a value of that type may
//! take. For a type parameter that is the type set of its constraint; for an
//! interface it is the interface's own type set; for any other type it is just
//! the type itself.
//!
//! # Divergence from go/ssa
//!
//! go/ssa exposes `typeset` as a `yield`-callback iterator over
//! `(type, underlying)` pairs. In Rust the callback cannot borrow the type
//! arena while `typeset` itself holds it `&mut` (computing an interface's type
//! set is lazy and mutating). We therefore expose [`typeset_pairs`], which
//! collects the pairs into an owned `Vec` first; consumers then iterate freely
//! with read access to the arena. [`typeset`] is kept as a thin
//! yield-callback wrapper for API parity (its callback does *not* receive the
//! arena, matching `guff_types::under::typeset_iter`).
//!
//! Note this is genuinely a separate port from `guff_types::under`'s
//! `typeset_iter`/`under_is`: the go/types version (`under.go`) only expands
//! the type set for *type parameters*, yielding `(iface, under(iface))` for a
//! bare interface, whereas go/ssa's `typeset` expands bare interfaces into
//! their term list too. `isBytestring`/`indexType` rely on that behaviour.
//!
//! The `debug`-gated `types.Identical` sanity checks in go/ssa (behind the
//! file-level `const debug = false`) are omitted.

use guff_types::{
    array_elem, interface_typeset, is_string, lookup_basic, map_elem, pointer_elem, slice_elem,
    type_param_iface, unalias, BasicKind, ObjectArena, PackageArena, TypeArena, TypeData, TypeId,
};

/// Collect the `(type, underlying)` pairs of the specific type terms of the
/// type set implied by `typ`.
///
/// - If `typ` is a type parameter, the implied type set is the type set of
///   its constraint. If there are no specific terms, returns a single
///   `(None, None)` pair.
/// - If `typ` is an interface, likewise over the interface's own type set.
/// - Otherwise the implied type set is just `typ`, yielding
///   `(Some(typ), Some(underlying))`.
///
/// The returned `Vec` is always non-empty. (Go: `typeset`, but materialised.)
pub fn typeset_pairs(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
) -> Vec<(Option<TypeId>, Option<TypeId>)> {
    let t = unalias(arena, typ);
    match arena.get(t) {
        TypeData::TypeParam(_) | TypeData::Interface(_) => {
            let terms = term_list_of(arena, oarena, parena, t);
            if terms.is_empty() {
                return vec![(None, None)];
            }
            terms
                .into_iter()
                .map(|(tilde, term_ty)| {
                    // u = Unalias(term.Type()); if !term.Tilde() { u = u.Underlying() }
                    let ua = unalias(arena, term_ty);
                    let u = if tilde { ua } else { ua.underlying(arena) };
                    (Some(term_ty), Some(u))
                })
                .collect()
        }
        _ => {
            let u = t.underlying(arena);
            vec![(Some(t), Some(u))]
        }
    }
}

/// Iterate the type set of `typ`, calling `yield_` once per `(type,
/// underlying)` pair (or once with `(None, None)` for an empty term set).
/// Returning `false` from `yield_` stops iteration. (Go: `typeset`.)
pub fn typeset(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
    mut yield_: impl FnMut(Option<TypeId>, Option<TypeId>) -> bool,
) {
    for (t, u) in typeset_pairs(arena, oarena, parena, typ) {
        if !yield_(t, u) {
            break;
        }
    }
}

/// The type set of `typ` as a normalized term list: `(tilde, type)` per term.
/// Returns an empty `Vec` when the term set is empty *or* is the set of all
/// types (both of which go/ssa's `termListOf` surfaces as a zero-length slice,
/// via `NormalTerms` returning `ErrEmptyTypeSet` / `nil` respectively).
///
/// `typ` must be a type parameter or an interface (Go's `termListOf` is only
/// called on those); any other type yields an empty list. (Go: `termListOf`.)
fn term_list_of(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
) -> Vec<(bool, TypeId)> {
    let iface = match arena.get(typ) {
        TypeData::TypeParam(_) => type_param_iface(arena, oarena, parena, typ),
        TypeData::Interface(_) => typ,
        _ => return Vec::new(),
    };
    let tset = interface_typeset(arena, oarena, parena, iface);
    let mut out = Vec::new();
    tset.is(|tilde, ty| {
        if let Some(t) = ty {
            out.push((tilde, t));
        }
        true
    });
    out
}

/// Reports whether the type set of `typ` is empty. (Go: `typeSetIsEmpty`.)
pub fn typeset_is_empty(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
) -> bool {
    // Go yields once and stops, inspecting the first term: `empty = t == nil`.
    typeset_pairs(arena, oarena, parena, typ)
        .first()
        .is_some_and(|(t, _)| t.is_none())
}

/// Calls `f` with the underlying type of each type term of the type set of
/// `typ` and reports whether *all* calls returned true. If there are no
/// specific terms, returns `f(None)`. (Go: `underIs`.)
pub fn under_is(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
    mut f: impl FnMut(&TypeArena, Option<TypeId>) -> bool,
) -> bool {
    let pairs = typeset_pairs(arena, oarena, parena, typ);
    let mut ok = false;
    for (_, u) in pairs {
        ok = f(arena, u);
        if !ok {
            break;
        }
    }
    ok
}

/// Reports whether `t` has the same terms as `interface{ []byte | string }`.
/// These act like a core type for slice expressions, `append`, and `copy`.
/// (Go: `isBytestring`.)
pub fn is_bytestring(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    t: TypeId,
) -> bool {
    let u = t.underlying(arena);
    if !matches!(arena.get(u), TypeData::Interface(_)) {
        return false;
    }
    let mut has_bytes = false;
    let mut has_string = false;
    let ok = under_is(arena, oarena, parena, u, |arena, tt| match tt {
        Some(x) if is_string(arena, x) => {
            has_string = true;
            true
        }
        Some(x) if is_byte_slice(arena, x) => {
            has_bytes = true;
            true
        }
        _ => false,
    });
    ok && has_bytes && has_string
}

/// Reports whether `t`'s underlying type is `[]byte`. (Go: `isByteSlice`.)
fn is_byte_slice(arena: &TypeArena, t: TypeId) -> bool {
    let u = t.underlying(arena);
    if matches!(arena.get(u), TypeData::Slice(_)) {
        let elem_u = slice_elem(arena, u).underlying(arena);
        return matches!(arena.get(elem_u), TypeData::Basic(b) if b.kind() == BasicKind::Uint8);
    }
    false
}

/// The (addressing) mode of an index operand, derived from the set of
/// underlying types of the indexed value.
///
/// Meet semi-lattice (Hasse diagram):
/// ```text
///   Var       Map
///    |         |
///  ArrVar      |
///    |         |
///  Value       |
///     \       /
///     Invalid
/// ```
/// (Go: `indexMode`.)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IndexMode {
    /// Index is invalid.
    Invalid,
    /// Index is a computed value (not addressable).
    Value,
    /// Like `Var`, but the index operand contains an array.
    ArrVar,
    /// Index is an addressable variable.
    Var,
    /// Map index expression (variable-like on lhs, comma-ok on rhs).
    Map,
}

impl IndexMode {
    /// The address type constrained by both `self` and `y`. (Go: `meet`.)
    pub fn meet(self, y: IndexMode) -> IndexMode {
        if (self == IndexMode::Map || y == IndexMode::Map) && self != y {
            return IndexMode::Invalid;
        }
        // Return the more-constrained (larger discriminant) of the two.
        if (self as u8) < (y as u8) {
            y
        } else {
            self
        }
    }
}

/// Returns the element type and index mode of an index expression over `typ`.
/// Returns `(None, IndexMode::Invalid)` if `typ` is not indexable (which
/// should never occur in a well-typed program). (Go: `indexType`.)
pub fn index_type(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
) -> (Option<TypeId>, IndexMode) {
    let u = typ.underlying(arena);
    match arena.get(u) {
        TypeData::Array(_) => (Some(array_elem(arena, u)), IndexMode::ArrVar),
        TypeData::Pointer(_) => {
            let elem = pointer_elem(arena, u);
            let elem_u = elem.underlying(arena);
            if matches!(arena.get(elem_u), TypeData::Array(_)) {
                (Some(array_elem(arena, elem_u)), IndexMode::Var)
            } else {
                (None, IndexMode::Invalid)
            }
        }
        TypeData::Slice(_) => (Some(slice_elem(arena, u)), IndexMode::Var),
        TypeData::Map(_) => (Some(map_elem(arena, u)), IndexMode::Map),
        // Must be a string: element type is byte, index is a computed value.
        TypeData::Basic(_) => (lookup_basic(arena, BasicKind::Uint8), IndexMode::Value),
        TypeData::Interface(_) => {
            let mut elem: Option<TypeId> = None;
            let mut mode = IndexMode::Invalid;
            for (t, _) in typeset_pairs(arena, oarena, parena, typ) {
                let Some(tt) = t else {
                    // Empty type set.
                    break;
                };
                let (e, m) = index_type(arena, oarena, parena, tt);
                if elem.is_none() {
                    elem = e;
                    mode = m;
                }
                // Update the mode to the most constrained address type.
                mode = mode.meet(m);
                if mode == IndexMode::Invalid {
                    break;
                }
            }
            (elem, mode)
        }
        _ => (None, IndexMode::Invalid),
    }
}
