//! Type canonicalization.
//!
//! Port of go/ssa's `canonizer` (util.go). A [`Canonizer`] maps a type `T` to a
//! canonical representative `C` such that `Identical(T, C)`, and maps a list of
//! types to a canonical [`CanonListId`]. The SSA builder keys its per-origin
//! generic-instance cache by the canonical type-argument list, so two
//! structurally-identical (but distinct-`TypeId`) argument lists resolve to the
//! same instantiated `Function`.
//!
//! Go dedups with a `typeutil.Hasher`; here types are bucketed by their
//! `TypeData` discriminant and compared within a bucket via
//! `guff_types::identical`. Lists are canonicalized element-wise (each
//! element through [`Canonizer::canonical_type`]) and interned, so list equality
//! reduces to equality of the canonical-representative vector. The empty list has
//! no representative (`None`), matching Go's nil `*typeList`.

use std::collections::HashMap;
use std::mem::Discriminant;

use guff_types::{
    identical, instantiate, lookup_field_or_method, named_origin, unalias, Context, LookupResult,
    ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData, TypeId,
};

/// A canonical, comparable handle for a non-empty list of types. Two lists whose
/// elements are pairwise `Identical` share the same id. (Go: a `*typeList`
/// pointer, used as an instance-cache map key.)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct CanonListId(pub usize);

/// Maps types (and lists of types) to canonical representatives up to
/// `Identical`. (Go: `canonizer`.)
#[derive(Default)]
pub struct Canonizer {
    /// Canonical representative types, bucketed by `TypeData` discriminant to
    /// keep per-lookup identity scans short. (Go: `canonizer.types`.)
    type_reps: HashMap<Discriminant<TypeData>, Vec<TypeId>>,
    /// Interned canonical type-arg lists, keyed by the vector of canonical
    /// element representatives. (Go: `canonizer.lists`.)
    list_index: HashMap<Vec<TypeId>, usize>,
}

impl Canonizer {
    /// Returns a canonical representative of `t`, removing the top-level alias.
    /// Two `Identical` types map to the same representative. (Go:
    /// `(*canonizer).Type`.)
    pub fn canonical_type(
        &mut self,
        arena: &mut TypeArena,
        oarena: &ObjectArena,
        parena: &PackageArena,
        t: TypeId,
    ) -> TypeId {
        // Remove the top-level alias (Go: types.Unalias(T)).
        let t = unalias(arena, t);
        let tag = std::mem::discriminant(arena.get(t));

        // Snapshot the bucket's current members before the identity scan, which
        // needs `&mut arena` (Interface identity lazily computes type sets).
        let candidates: Vec<TypeId> = self.type_reps.get(&tag).cloned().unwrap_or_default();
        for r in candidates {
            if identical(arena, oarena, parena, r, t) {
                return r;
            }
        }
        self.type_reps.entry(tag).or_default().push(t);
        t
    }

    /// Returns a canonical id for the type list `ts`, or `None` for the empty
    /// list. Lists whose elements are pairwise `Identical` share an id. (Go:
    /// `(*canonizer).List`.)
    pub fn canonical_list(
        &mut self,
        arena: &mut TypeArena,
        oarena: &ObjectArena,
        parena: &PackageArena,
        ts: &[TypeId],
    ) -> Option<CanonListId> {
        if ts.is_empty() {
            return None;
        }
        let key: Vec<TypeId> = ts
            .iter()
            .map(|&t| self.canonical_type(arena, oarena, parena, t))
            .collect();
        let next = self.list_index.len();
        let id = *self.list_index.entry(key).or_insert(next);
        Some(CanonListId(id))
    }

    /// Instantiates method `m` with receiver type arguments `rtargs` and returns
    /// a canonical representative for the instantiated method object. (Go:
    /// `(*canonizer).instantiateMethod`.)
    pub fn instantiate_method(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        parena: &PackageArena,
        method: ObjectId,
        rtargs: &[TypeId],
        ctxt: &mut Context,
    ) -> ObjectId {
        let recv_typ = crate::methods::recv_type_from_objects(arena, oarena, method)
            .expect("instantiate_method: method must have receiver");
        let mut recv = unalias(arena, recv_typ);
        if let TypeData::Pointer(p) = arena.get(recv) {
            recv = p.elem();
        }
        let recv = unalias(arena, recv);
        let named = match arena.get(recv) {
            TypeData::Named(_) => recv,
            other => panic!(
                "instantiate_method: receiver is not a named type: {:?}",
                std::mem::discriminant(other)
            ),
        };
        let orig = named_origin(arena, named);
        let inst = instantiate(arena, oarena, ctxt, orig, rtargs.to_vec());
        let rep = self.canonical_type(arena, oarena, parena, inst);
        let pkg = method.pkg(oarena);
        let name = method.name(oarena);
        let result = lookup_field_or_method(arena, oarena, parena, rep, true, pkg, name);
        match result {
            LookupResult::Found { obj, .. } => {
                assert!(
                    matches!(oarena.get(obj), ObjectData::Func(_)),
                    "instantiate_method: {name} is not a method"
                );
                obj
            }
            other => panic!("instantiate_method: lookup {name} on {rep:?}: {other:?}"),
        }
    }
}
