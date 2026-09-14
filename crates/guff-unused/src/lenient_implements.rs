//! Port of `honnef.co/go/tools/unused/implements.go`.
//!
//! `unused` does **not** ask `types.Implements`. It carries its own relation,
//! in which an interface method's *bare type parameter* matches any type that
//! satisfies the parameter's constraint, consistently across the interface:
//!
//! ```go
//! func (c *methodsChecker) typeIsCompatible(implType, interfaceType types.Type) bool {
//! 	if types.Identical(implType, interfaceType) {
//! 		return true
//! 	}
//! 	// We only support trivial use of type parameters. This isn't fully compatible with compiler type checking yet.
//! 	tp, ok := interfaceType.(*types.TypeParam)
//! 	if !ok {
//! 		return false
//! 	}
//! 	…
//! 	if c.typeParams[tp] == nil {
//! 		if !satisfiesConstraint(implType, tp) {
//! 			return false
//! 		}
//! 		c.typeParams[tp] = implType
//! 		return true
//! 	}
//! 	return types.Identical(c.typeParams[tp], implType)
//! }
//! ```
//!
//! The word "trivial" is the whole difference between two corpus targets:
//!
//! - opentofu's `ResultRef[T any]` declares `resultPlaceholderSigil(T)`, a bare
//!   `T`, so `valueResultRef`'s `resultPlaceholderSigil(cty.Value)` is
//!   compatible and its method is used;
//! - dapr's `streamer[T item]` declares `list() ([]T, error)`, and `[]T` is not
//!   a `*types.TypeParam`, so `(*components).list() ([]int, error)` is *not*
//!   compatible and all forty of dapr's methods are findings.
//!
//! Matching interface methods by name cannot separate those two: the names are
//! identical in both. That is why this file exists.

use std::collections::HashMap;

use guff_types::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::check_lookup::implements as strict_implements;
use guff_types::lookup::lookup_field_or_method;
use guff_types::predicates::identical;
use guff_types::signature::{signature_params, signature_results};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::ObjectId;

/// `methodsChecker`: the bindings a single `implements` call has accumulated.
#[derive(Default)]
struct Bindings {
    /// interface type parameter → the type it was first matched against.
    bound: HashMap<TypeId, TypeId>,
}

impl Bindings {
    fn type_is_compatible(
        &mut self,
        types: &mut TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        impl_t: TypeId,
        iface_t: TypeId,
    ) -> bool {
        if identical(types, objects, packages, impl_t, iface_t) {
            return true;
        }
        let constraint = match types.get(iface_t) {
            TypeData::TypeParam(tp) => tp.constraint(),
            _ => return false,
        };
        match self.bound.get(&iface_t).copied() {
            Some(prev) => identical(types, objects, packages, prev, impl_t),
            None => {
                // `satisfiesConstraint`: `types.Satisfies(t, tp.Constraint())`.
                // A type parameter with no constraint (`[T any]`) accepts
                // anything, which is the opentofu case.
                let ok = match constraint {
                    None => true,
                    Some(c) => {
                        strict_implements(types, objects, packages, impl_t, c, true).is_ok()
                    }
                };
                if ok {
                    self.bound.insert(iface_t, impl_t);
                }
                ok
            }
        }
    }

    fn tuples_are_compatible(
        &mut self,
        types: &mut TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        impl_tuple: Option<TypeId>,
        iface_tuple: Option<TypeId>,
    ) -> bool {
        let n = tuple_len(types, impl_tuple);
        if n != tuple_len(types, iface_tuple) {
            return false;
        }
        let (Some(a), Some(b)) = (impl_tuple, iface_tuple) else {
            // Both empty: nothing to compare.
            return n == 0;
        };
        for i in 0..n {
            let at = tuple_at(types, a, i).typ(objects);
            let bt = tuple_at(types, b, i).typ(objects);
            let (Some(at), Some(bt)) = (at, bt) else {
                return false;
            };
            if !self.type_is_compatible(types, objects, packages, at, bt) {
                return false;
            }
        }
        true
    }

    fn method_is_compatible(
        &mut self,
        types: &mut TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        impl_sig: TypeId,
        iface_sig: TypeId,
    ) -> bool {
        if identical(types, objects, packages, impl_sig, iface_sig) {
            return true;
        }
        let (ip, ir) = (
            signature_params(types, impl_sig),
            signature_results(types, impl_sig),
        );
        let (fp, fr) = (
            signature_params(types, iface_sig),
            signature_results(types, iface_sig),
        );
        self.tuples_are_compatible(types, objects, packages, ip, fp)
            && self.tuples_are_compatible(types, objects, packages, ir, fr)
    }
}

/// `implements(V, T, msV)` — the methods of `v` that satisfy every method of
/// the interface `iface`, or `None` when `v` does not implement it.
///
/// Returns the *implementing* method objects, which is what upstream reads
/// through `g.readSelection(sel, named)`.
#[allow(clippy::too_many_arguments)]
pub fn lenient_implements(
    types: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    pkg: guff_types::arena::PackageId,
    v: TypeId,
    iface: TypeId,
    iface_methods: &[(String, TypeId)],
) -> Option<Vec<ObjectId>> {
    let _ = iface;
    if iface_methods.is_empty() {
        // `if T.Empty() { return nil, true }`
        return Some(Vec::new());
    }
    let mut bindings = Bindings::default();
    let mut out = Vec::new();
    for (name, iface_sig) in iface_methods {
        // `msV.Lookup(m.Pkg(), m.Name())`, over the value *and* pointer method
        // sets — upstream calls `processMethodSet` once for each.
        // `msV.Lookup(m.Pkg(), m.Name())` — the package matters, because an
        // unexported method name is only the same identifier inside the
        // package that declared it. Passing no package finds nothing for
        // exactly the sealing-interface shape this is here for.
        let found = lookup_field_or_method(types, objects, packages, v, true, Some(pkg), name)
            .found()
            .map(|(obj, _, _)| obj);
        let Some(found) = found else {
            return None;
        };
        let Some(impl_sig) = found.typ(objects) else {
            return None;
        };
        if !bindings.method_is_compatible(types, objects, packages, impl_sig, *iface_sig) {
            return None;
        }
        out.push(found);
    }
    Some(out)
}
