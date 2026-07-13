//! Port of `cmd/compile/internal/types2/tuple.go`.
//!
//! A `Tuple` represents an ordered list of variables; in Go, a `nil *Tuple` is
//! a valid (empty) tuple. In our arena model, we mirror that by having
//! [`new_tuple`] return `None` for the empty case — callers that hold an
//! `Option<TypeId>` interpret `None` as the empty tuple, matching `nil *Tuple`.

use crate::arena::{ObjectId, TypeArena, TypeData, TypeId};

/// An ordered list of variables — used as the parameter and result lists of
/// signatures, and as the type of multi-value assignments. Tuples are not
/// first-class Go types.
///
/// Equivalent to `types2.Tuple`.
#[derive(Debug, Clone, Default)]
pub struct Tuple {
    vars: Vec<ObjectId>,
}

impl Tuple {
    pub fn len(&self) -> usize {
        self.vars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vars.is_empty()
    }

    /// The i'th variable; panics if `i` is out of bounds.
    pub fn at(&self, i: usize) -> ObjectId {
        self.vars[i]
    }
}

/// Construct a new tuple. Returns `None` for the empty case, matching Go's
/// convention that a `nil *Tuple` is the empty tuple.
///
/// Equivalent to `types2.NewTuple`.
pub fn new_tuple(arena: &mut TypeArena, vars: &[ObjectId]) -> Option<TypeId> {
    if vars.is_empty() {
        return None;
    }
    let id = arena.alloc(TypeData::Tuple(Tuple {
        vars: vars.to_vec(),
    }));
    Some(id)
}

/// Allocate a concrete, zero-length tuple type and return its [`TypeId`].
///
/// Unlike [`new_tuple`] — which maps the empty case to `None`, mirroring Go's
/// nil `*Tuple` — this yields a real `TypeId` for the empty tuple. It is the
/// analog of Go storing a nil `*types.Tuple` inside a *non-nil* `types.Type`
/// interface: a value whose type is "the empty tuple" (rendered `()` by the
/// disassembler) rather than "no type at all". go/ssa relies on this for the
/// result type of void calls — `emitTailCall` with zero results, and the
/// synthesized `init()`/wrapper calls — where `Value.Type()` must still be a
/// (printable) tuple, not absent.
pub fn empty_tuple(arena: &mut TypeArena) -> TypeId {
    arena.alloc(TypeData::Tuple(Tuple::default()))
}

/// Number of variables in a tuple. `None` means the empty tuple (length 0),
/// matching the Go `(*Tuple).Len()` semantics where a nil receiver returns 0.
pub fn tuple_len(arena: &TypeArena, id: Option<TypeId>) -> usize {
    match id {
        None => 0,
        Some(id) => as_tuple(arena, id).len(),
    }
}

/// `i`'th variable of the tuple. Panics if `id` is `None` or out of bounds.
pub fn tuple_at(arena: &TypeArena, id: TypeId, i: usize) -> ObjectId {
    as_tuple(arena, id).at(i)
}

fn as_tuple(arena: &TypeArena, id: TypeId) -> &Tuple {
    match arena.get(id) {
        TypeData::Tuple(t) => t,
        other => panic!("expected Tuple, got {:?}", std::mem::discriminant(other)),
    }
}
