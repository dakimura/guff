//! Port of `golang.org/x/tools/internal/typeparams/free.go`.
//!
//! One question: does this type mention a type parameter that is still free —
//! i.e. not pinned down by an instantiation? x/tools' `ifaceassert` asks it to
//! decide whether it may draw a conclusion about a type assertion at all:
//!
//! ```go
//! // Mitigations for interface comparisons and generics.
//! // TODO(https://github.com/golang/go/issues/50658): Support more precise conclusion.
//! if free.Has(V) || free.Has(T) {
//!     return nil
//! }
//! ```
//!
//! Without it, `Merge[Res, Req]` and `Merge[int, string]` look like two
//! unrelated interfaces whose `Send` methods contradict each other, and every
//! `switch v.(type)` inside a generic function becomes a finding. pyroscope
//! writes sixteen of them.

use std::collections::HashMap;

use crate::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};

/// Memoises [`has_free_type_param`] across a sequence of overlapping types.
///
/// Equivalent to `typeparams.Free`; the zero value is ready for use. The `seen`
/// map doubles as cycle detection — a type currently being walked answers
/// `false` so a recursive type terminates, exactly as upstream does.
#[derive(Default)]
pub struct Free {
    seen: HashMap<TypeId, bool>,
}

impl Free {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports whether `t` has a free type parameter.
    ///
    /// Equivalent to `(*typeparams.Free).Has`.
    pub fn has(
        &mut self,
        types: &mut TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        t: TypeId,
    ) -> bool {
        if let Some(&x) = self.seen.get(&t) {
            return x;
        }
        self.seen.insert(t, false);
        let res = self.compute(types, objects, packages, t);
        self.seen.insert(t, res);
        res
    }

    fn compute(
        &mut self,
        types: &mut TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        t: TypeId,
    ) -> bool {
        match types.get(t).clone() {
            TypeData::Basic(_) => false,

            TypeData::Alias(a) => {
                // An uninstantiated parameterized alias is free on its own.
                let nparams = crate::typelists::type_param_list_len(a.type_params());
                let nargs = crate::typelists::type_list_len(a.type_args());
                if nparams > nargs {
                    return true;
                }
                // The expansion can be free whether or not the alias is
                // parameterized, so unalias before recursing.
                let u = crate::alias::unalias_readonly(types, t);
                if u == t {
                    return false;
                }
                self.has(types, objects, packages, u)
            }

            TypeData::Array(a) => self.has(types, objects, packages, a.elem()),
            TypeData::Slice(s) => self.has(types, objects, packages, s.elem()),
            TypeData::Pointer(p) => self.has(types, objects, packages, p.elem()),
            TypeData::Chan(c) => self.has(types, objects, packages, c.elem()),

            TypeData::Map(m) => {
                self.has(types, objects, packages, m.key())
                    || self.has(types, objects, packages, m.elem())
            }

            TypeData::Struct(st) => (0..st.num_fields()).any(|i| {
                match st.field(i).typ(objects) {
                    Some(ft) => self.has(types, objects, packages, ft),
                    None => false,
                }
            }),

            TypeData::Tuple(_) => {
                let n = crate::tuple::tuple_len(types, Some(t));
                (0..n).any(|i| {
                    let v = crate::tuple::tuple_at(types, t, i);
                    match v.typ(objects) {
                        Some(vt) => self.has(types, objects, packages, vt),
                        None => false,
                    }
                })
            }

            // A signature's own type parameters (and a method receiver's) are
            // declarations, not uses: only the inputs and results matter.
            TypeData::Signature(_) => {
                let params = crate::signature::signature_params(types, t);
                let results = crate::signature::signature_results(types, t);
                params.is_some_and(|p| self.has(types, objects, packages, p))
                    || results.is_some_and(|r| self.has(types, objects, packages, r))
            }

            TypeData::Interface(_) => {
                let tset = crate::interface::interface_typeset(types, objects, packages, t);
                for &m in tset.methods() {
                    if let Some(mt) = m.typ(objects) {
                        if self.has(types, objects, packages, mt) {
                            return true;
                        }
                    }
                }
                let terms: Vec<Option<TypeId>> = tset
                    .terms
                    .iter()
                    .filter_map(|slot| slot.as_ref().map(|term| term.typ))
                    .collect();
                terms
                    .into_iter()
                    .flatten()
                    .any(|ty| self.has(types, objects, packages, ty))
            }

            // Upstream has no `*types.Union` case — a union only ever reaches
            // it inside an interface, whose term set is walked above. Walking
            // its terms here is the same set of types, and cannot panic.
            TypeData::Union(u) => {
                let terms: Vec<TypeId> = (0..u.len()).map(|i| u.term(i).typ()).collect();
                terms
                    .into_iter()
                    .any(|ty| self.has(types, objects, packages, ty))
            }

            TypeData::Named(n) => {
                let nparams = crate::typelists::type_param_list_len(n.type_params());
                let nargs = crate::typelists::type_list_len(crate::named::named_type_args(types, t));
                if nparams > nargs {
                    return true; // an uninstantiated named type
                }
                let args: Vec<TypeId> = crate::named::named_type_args(types, t)
                    .map(|l| l.list().to_vec())
                    .unwrap_or_default();
                for a in args {
                    if self.has(types, objects, packages, a) {
                        return true;
                    }
                }
                // Recurse for types local to parameterized functions.
                match crate::named::named_underlying(types, t) {
                    Some(u) if u != t => self.has(types, objects, packages, u),
                    _ => false,
                }
            }

            TypeData::TypeParam(_) => true,
        }
    }
}

/// One-shot [`Free::has`] for callers with a single type to test.
pub fn has_free_type_param(
    types: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    t: TypeId,
) -> bool {
    Free::new().has(types, objects, packages, t)
}
