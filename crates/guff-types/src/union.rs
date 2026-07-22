//! Port of `cmd/compile/internal/types2/union.go`.
//!
//! Chunk 2 ports the data structures ([`Union`], [`Term`]) and their public
//! constructors and accessors. The `parseUnion`/`parseTilde` validation logic
//! is Checker-internal and lands with the type-checker proper.

use serde::{Deserialize, Serialize};

use crate::arena::{TypeArena, TypeData, TypeId};

/// A union of terms embedded in an interface (e.g. the `int | string` part of
/// `interface { int | string }`).
///
/// Equivalent to `types2.Union`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Union {
    terms: Vec<Term>,
}

impl Union {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for t in &mut self.terms {
            t.typ = r.ty(t.typ);
        }
    }
}

impl Union {
    /// Number of terms in the union.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    pub fn is_empty(&self) -> bool {
        // A Union is never legally empty (NewUnion panics on empty input),
        // but we expose the standard accessor for ergonomic consistency.
        self.terms.is_empty()
    }

    /// The `i`'th term; panics if `i >= len()`.
    pub fn term(&self, i: usize) -> &Term {
        &self.terms[i]
    }
}

/// A term in a [`Union`]: a type, optionally prefixed with `~` (meaning "any
/// type whose underlying type is `typ`").
///
/// Equivalent to `types2.Term`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Term {
    tilde: bool,
    typ: TypeId,
}

impl Term {
    /// Reports whether this term uses the `~` prefix.
    pub fn tilde(&self) -> bool {
        self.tilde
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }
}

/// Construct a new union term.
///
/// Equivalent to `types2.NewTerm`.
pub fn new_term(tilde: bool, typ: TypeId) -> Term {
    Term { tilde, typ }
}

/// Construct a new union type. It is an error to create an empty union;
/// these are syntactically impossible in Go.
///
/// Equivalent to `types2.NewUnion`.
///
/// # Panics
/// Panics if `terms` is empty.
pub fn new_union(arena: &mut TypeArena, terms: Vec<Term>) -> TypeId {
    if terms.is_empty() {
        panic!("empty union");
    }
    arena.alloc(TypeData::Union(Union { terms }))
}

// Free-function accessors.

pub fn union_len(arena: &TypeArena, id: TypeId) -> usize {
    as_union(arena, id).len()
}

pub fn union_term<'a>(arena: &'a TypeArena, id: TypeId, i: usize) -> &'a Term {
    as_union(arena, id).term(i)
}

fn as_union(arena: &TypeArena, id: TypeId) -> &Union {
    match arena.get(id) {
        TypeData::Union(u) => u,
        other => panic!("expected Union, got {:?}", std::mem::discriminant(other)),
    }
}
