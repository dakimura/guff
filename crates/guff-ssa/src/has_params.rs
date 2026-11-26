//! Free-type-parameter detection with memoization.
//!
//! Port of go/ssa's use of `internal/typeparams.Free` (`prog.hasParams`) and the
//! `(*Program).isParameterized` predicate built on it. [`HasParams::has`] reports
//! whether a type contains *any* free type parameter (i.e. whether it is not
//! fully concrete); the SSA builder uses this to decide whether a generic
//! instantiation can be built directly (`InstantiateGenerics`) or must go through
//! an instantiation wrapper, and to skip runtime types for parameterized types.
//!
//! This is distinct from `guff_types::is_parameterized`, which asks whether
//! a type mentions a *specific* set of type parameters (used during inference).
//! Here any `TypeParam` occurrence counts, matching Go's `typeparams.Free.Has`.

use crate::hash::HashMap;

use guff_types::{
    named_type_args, named_underlying, unalias_readonly, ObjectArena, ObjectData, ObjectId,
    TypeArena, TypeData, TypeId,
};

/// A memoization of "does this type contain a free type parameter?", making a
/// sequence of overlapping queries efficient. The empty map is ready for use.
/// (Go: `internal/typeparams.Free`.)
#[derive(Default)]
pub struct HasParams {
    /// Cache keyed by type; also breaks cycles (a type is provisionally marked
    /// `false` while its own traversal is in flight). (Go: `Free.seen`.)
    seen: HashMap<TypeId, bool>,
}

impl HasParams {
    /// Reports whether `typ` has a free type parameter. (Go: `(*Free).Has`.)
    pub fn has(&mut self, arena: &TypeArena, oarena: &ObjectArena, typ: TypeId) -> bool {
        if let Some(&x) = self.seen.get(&typ) {
            return x;
        }
        // Provisionally false to break cycles; overwritten with the real result.
        self.seen.insert(typ, false);
        let res = self.compute(arena, oarena, typ);
        self.seen.insert(typ, res);
        res
    }

    fn compute(&mut self, arena: &TypeArena, oarena: &ObjectArena, typ: TypeId) -> bool {
        match arena.get(typ) {
            TypeData::Basic(_) => false,
            TypeData::TypeParam(_) => true,

            TypeData::Array(a) => self.has(arena, oarena, a.elem()),
            TypeData::Slice(s) => self.has(arena, oarena, s.elem()),
            TypeData::Pointer(p) => self.has(arena, oarena, p.elem()),
            TypeData::Chan(c) => self.has(arena, oarena, c.elem()),
            TypeData::Map(m) => {
                let (k, v) = (m.key(), m.elem());
                self.has(arena, oarena, k) || self.has(arena, oarena, v)
            }

            TypeData::Alias(a) => {
                // An uninstantiated alias (more type params than type args) is
                // parameterized. Otherwise its expansion may still contain free
                // parameters, so unalias and recurse.
                let n_params = a.type_params().map_or(0, |l| l.len());
                let n_args = a.type_args().map_or(0, |l| l.len());
                if n_params > n_args {
                    return true;
                }
                let u = unalias_readonly(arena, typ);
                u != typ && self.has(arena, oarena, u)
            }

            TypeData::Struct(_) => {
                let fields: Vec<ObjectId> = match arena.get(typ) {
                    TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
                    _ => unreachable!(),
                };
                self.any_var(arena, oarena, &fields)
            }
            TypeData::Tuple(_) => {
                let vars: Vec<ObjectId> = match arena.get(typ) {
                    TypeData::Tuple(t) => (0..t.len()).map(|i| t.at(i)).collect(),
                    _ => unreachable!(),
                };
                self.any_var(arena, oarena, &vars)
            }

            TypeData::Signature(sig) => {
                // The signature's own type parameters (and a method receiver's)
                // are declarations, not uses, so only the input/result parameter
                // types matter.
                let (params, results) = (sig.params(), sig.results());
                params.is_some_and(|p| self.has(arena, oarena, p))
                    || results.is_some_and(|r| self.has(arena, oarena, r))
            }

            TypeData::Interface(_) => {
                // Read-only approximation (cannot compute a fresh type set here):
                // walk explicit method signatures and embedded types. Embedded
                // union/term constraints appear as embedded types, so a parameter
                // used only in a constraint term is still caught.
                use guff_types::{
                    interface_embedded_type, interface_explicit_method, interface_num_embeddeds,
                    interface_num_explicit_methods,
                };
                let n_methods = interface_num_explicit_methods(arena, typ);
                let methods: Vec<ObjectId> =
                    (0..n_methods).map(|i| interface_explicit_method(arena, typ, i)).collect();
                let n_embeds = interface_num_embeddeds(arena, typ);
                let embeds: Vec<TypeId> =
                    (0..n_embeds).map(|i| interface_embedded_type(arena, typ, i)).collect();
                for m in methods {
                    if let Some(mt) = m.typ(oarena) {
                        if self.has(arena, oarena, mt) {
                            return true;
                        }
                    }
                }
                embeds.into_iter().any(|e| self.has(arena, oarena, e))
            }

            TypeData::Union(_) => {
                let terms: Vec<TypeId> = match arena.get(typ) {
                    TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).typ()).collect(),
                    _ => unreachable!(),
                };
                terms.into_iter().any(|t| self.has(arena, oarena, t))
            }

            TypeData::Named(_) => {
                // An uninstantiated named type (more type params than args) is
                // parameterized. Otherwise recurse into the type arguments and
                // the underlying (for types local to parameterized functions).
                let n_params = match arena.get(typ) {
                    TypeData::Named(n) => n.type_params().map_or(0, |l| l.len()),
                    _ => unreachable!(),
                };
                let args: Vec<TypeId> = named_type_args(arena, typ)
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default();
                if n_params > args.len() {
                    return true;
                }
                if args.iter().any(|&a| self.has(arena, oarena, a)) {
                    return true;
                }
                named_underlying(arena, typ).is_some_and(|u| self.has(arena, oarena, u))
            }
        }
    }

    fn any_var(&mut self, arena: &TypeArena, oarena: &ObjectArena, vars: &[ObjectId]) -> bool {
        vars.iter().any(|&v| match oarena.get(v) {
            ObjectData::Var(var) => self.has(arena, oarena, var.typ()),
            _ => false,
        })
    }
}
