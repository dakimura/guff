//! Port of `cmd/compile/internal/types2/typelists.go`.
//!
//! Both lists are thin wrappers over `Vec<TypeId>`. In Go, a `nil` pointer is
//! treated as an empty list (`Len()` and `At()` are nil-safe); we mirror that
//! by returning `Option<...>` from constructors, with `None` meaning empty.
//! Per-method accessors take `Option<&...>` for the same reason.

use serde::{Deserialize, Serialize};

use crate::arena::{TypeArena, TypeData, TypeId};

/// A list of type parameters.
///
/// Equivalent to `types2.TypeParamList`. Each entry is the `TypeId` of a
/// `TypeData::TypeParam`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeParamList {
    tparams: Vec<TypeId>,
}

impl TypeParamList {
    pub fn len(&self) -> usize {
        self.tparams.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tparams.is_empty()
    }

    /// The `i`'th type parameter.
    pub fn at(&self, i: usize) -> TypeId {
        self.tparams[i]
    }

    /// Slice view of the underlying list.
    pub fn list(&self) -> &[TypeId] {
        &self.tparams
    }

    /// Wrap an already-bound list without re-binding it.
    ///
    /// `bind_tparams` panics on an entry whose index is already set, which is
    /// exactly the state `rename_tparams` leaves its output in — it copies the
    /// original indices deliberately. Upstream writes the same thing as
    /// `&TypeParamList{atparams}` when it re-points a renamed signature at its
    /// fresh parameters (`Checker.arguments`, reverse type inference).
    pub fn from_bound(tparams: Vec<TypeId>) -> Self {
        Self { tparams }
    }

    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for t in &mut self.tparams {
            *t = r.ty(*t);
        }
    }
}

/// Length helper that treats `None` as empty (matches Go's nil-safe `Len`).
pub fn type_param_list_len(list: Option<&TypeParamList>) -> usize {
    list.map(|l| l.len()).unwrap_or(0)
}

/// A list of types.
///
/// Equivalent to `types2.TypeList`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeList {
    types: Vec<TypeId>,
}

impl TypeList {
    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }

    pub fn at(&self, i: usize) -> TypeId {
        self.types[i]
    }

    pub fn list(&self) -> &[TypeId] {
        &self.types
    }

    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for t in &mut self.types {
            *t = r.ty(*t);
        }
    }
}

pub fn new_type_list(list: Vec<TypeId>) -> Option<TypeList> {
    if list.is_empty() {
        None
    } else {
        Some(TypeList { types: list })
    }
}

pub fn type_list_len(list: Option<&TypeList>) -> usize {
    list.map(|l| l.len()).unwrap_or(0)
}

/// Bind a list of TypeParam IDs into a [`TypeParamList`]. Mutates each
/// `TypeParam`'s `index` to its position in the list. Returns `None` for an
/// empty list (matches Go's `nil *TypeParamList`).
///
/// Equivalent to `types2.bindTParams`.
///
/// # Panics
/// Panics if any entry already has a non-negative index (i.e. has already
/// been bound), or if any entry isn't actually a `TypeParam`.
pub fn bind_tparams(arena: &mut TypeArena, list: Vec<TypeId>) -> Option<TypeParamList> {
    if list.is_empty() {
        return None;
    }
    for (i, id) in list.iter().copied().enumerate() {
        match arena.get_mut(id) {
            TypeData::TypeParam(tp) => {
                if tp.index() >= 0 {
                    panic!("type parameter bound more than once");
                }
                tp.set_index(i as i32);
            }
            other => panic!(
                "bind_tparams expected TypeParam, got {:?}",
                std::mem::discriminant(other)
            ),
        }
    }
    Some(TypeParamList { tparams: list })
}
