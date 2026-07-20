//! Port of `cmd/compile/internal/types2/type.go`.
//!
//! In Go, `Type` is a 2-method interface (`Underlying`, `String`). In our
//! arena-based port, every type is represented by a [`TypeId`] and we provide
//! the equivalent operations as methods on `TypeId` (or free functions taking
//! an arena reference).

use crate::arena::{TypeArena, TypeData, TypeId};

/// Discriminant tag for [`TypeData`], for callers that need to switch on a
/// type's kind without holding a borrow into the arena.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum TypeKind {
    Basic,
    Array,
    Slice,
    Pointer,
    Map,
    Chan,
    Tuple,
    Struct,
    Signature,
    Interface,
    Union,
    Named,
    Alias,
    TypeParam,
}

impl TypeId {
    /// Returns the kind of the underlying [`TypeData`].
    ///
    /// Equivalent to a Go type-switch on the type's dynamic kind.
    pub fn kind(self, arena: &TypeArena) -> TypeKind {
        match arena.get(self) {
            TypeData::Basic(_) => TypeKind::Basic,
            TypeData::Array(_) => TypeKind::Array,
            TypeData::Slice(_) => TypeKind::Slice,
            TypeData::Pointer(_) => TypeKind::Pointer,
            TypeData::Map(_) => TypeKind::Map,
            TypeData::Chan(_) => TypeKind::Chan,
            TypeData::Tuple(_) => TypeKind::Tuple,
            TypeData::Struct(_) => TypeKind::Struct,
            TypeData::Signature(_) => TypeKind::Signature,
            TypeData::Interface(_) => TypeKind::Interface,
            TypeData::Union(_) => TypeKind::Union,
            TypeData::Named(_) => TypeKind::Named,
            TypeData::Alias(_) => TypeKind::Alias,
            TypeData::TypeParam(_) => TypeKind::TypeParam,
        }
    }

    /// Returns the underlying type of this type.
    ///
    /// For literal types (Basic, Array, …, Union) this is the type itself.
    /// For Named, this is the cached underlying type set by `SetUnderlying`
    /// (or `self` if still incomplete). For Alias, this walks the alias chain
    /// until a non-alias type is found, then returns *its* underlying. For
    /// TypeParam, this is the underlying of its constraint (a chunk-3 partial
    /// port — the full `iface()` machinery in `typeparam.go` lands with
    /// typeset.go).
    ///
    /// Equivalent to `Type.Underlying`.
    pub fn underlying(self, arena: &TypeArena) -> TypeId {
        self.underlying_depth(arena, 0)
    }

    fn underlying_depth(self, arena: &TypeArena, depth: u32) -> TypeId {
        const LIMIT: u32 = 256;
        if depth > LIMIT {
            return self;
        }
        let next = depth + 1;
        match arena.get(self) {
            TypeData::Named(n) => match n.underlying() {
                Some(u) => u.underlying_depth(arena, next),
                None => self, // incomplete Named — return self (Go returns nil)
            },
            TypeData::Alias(_) => {
                let resolved = crate::alias::unalias_readonly(arena, self);
                if resolved == self {
                    self // incomplete alias chain — return self
                } else {
                    resolved.underlying_depth(arena, next)
                }
            }
            TypeData::TypeParam(tp) => {
                match tp.constraint() {
                    Some(b) => {
                        let u = b.underlying_depth(arena, next);
                        if matches!(arena.get(u), TypeData::Interface(_)) {
                            u
                        } else {
                            self
                        }
                    }
                    None => self,
                }
            }
            _ => self,
        }
    }
}
