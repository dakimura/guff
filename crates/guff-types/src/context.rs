//! Port of `cmd/compile/internal/types2/context.go`.
//!
//! A [`Context`] dedups type instances created by [`Instantiate`] / the
//! `Checker`. Two purposes:
//!
//! 1. Reduce duplication of identical instances (the same generic type
//!    instantiated with the same type arguments returns a shared
//!    [`TypeId`]).
//! 2. Short-circuit instantiation cycles — when recursive instantiation
//!    encounters an in-flight instance, it stops and returns the partial
//!    placeholder rather than infinitely expanding.
//!
//! Chunk-9 simplification: Go uses a string hash (via `instanceHash` /
//! `typeHasher`) to bucket entries because Go's `Type` lacks a stable
//! integer ID. Our `TypeId(NonZeroU32)` is already stable across the
//! lifetime of the arena, so we key the map directly on
//! `(origin TypeId, type args Vec<TypeId>)`. No string hashing needed —
//! and instance lookup is `O(args.len())` instead of `O(hash) + bucket`.

use std::collections::HashMap;

use crate::arena::TypeId;

/// An opaque type-checking context. Shared across [`Instantiate`]
/// invocations / type-checking passes so that generic instantiations are
/// canonicalised.
///
/// **Thread-safety:** Go's `Context` is `sync.Mutex`-guarded for
/// concurrent use. Our port runs serially (matching the rest of the
/// crate) and is *not* `Send`/`Sync`.
///
/// Equivalent to `types2.Context`.
#[derive(Debug, Default)]
pub struct Context {
    /// `(origin, targs)` → instance TypeId.
    instances: HashMap<(TypeId, Vec<TypeId>), TypeId>,
}

impl Context {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return an existing instantiation of `orig` with `targs`, or `None`
    /// if none has been recorded yet.
    ///
    /// Equivalent to `Context.lookup`.
    pub fn lookup(&self, orig: TypeId, targs: &[TypeId]) -> Option<TypeId> {
        self.instances.get(&(orig, targs.to_vec())).copied()
    }

    /// Record `inst` as the instantiation of `orig` with `targs`. If an
    /// identical instance was already recorded, return that one and
    /// discard `inst` (the caller's responsibility — Go does the same).
    ///
    /// Equivalent to `Context.update`.
    pub fn update(&mut self, orig: TypeId, targs: Vec<TypeId>, inst: TypeId) -> TypeId {
        let key = (orig, targs);
        if let Some(&existing) = self.instances.get(&key) {
            return existing;
        }
        self.instances.insert(key, inst);
        inst
    }

    /// Total number of instances recorded.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }
}
