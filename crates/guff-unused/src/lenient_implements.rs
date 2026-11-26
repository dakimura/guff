//! Port of `honnef.co/go/tools/unused/implements.go` (staticcheck v0.8.1).
//!
//! `unused` does **not** ask `types.Implements`. It carries its own relation:
//! each interface method's signature is *unified* with the concrete method's
//! ([`crate::unify`]), one type-parameter mapping shared across the interface,
//! and the bindings are checked against their constraints at the end.
//!
//! v0.7.0 (golangci-lint 2.12.2) had a `methodsChecker` that matched a *bare*
//! type parameter only, and that difference was two corpus targets:
//!
//! - opentofu's `ResultRef[T any]` declares `resultPlaceholderSigil(T)`, a bare
//!   `T`, so `valueResultRef`'s method was used under both versions;
//! - dapr's `streamer[T item]` declares `list() ([]T, error)`: `[]T` is not a
//!   `*types.TypeParam`, so under v0.7.0 `(*components).list() ([]int, error)`
//!   was *not* compatible and all forty of dapr's methods were findings. Under
//!   v0.8.1 `[]T` unifies with `[]int` and they are used.
//!
//! Matching interface methods by name cannot separate either case from a real
//! mismatch: the names are identical. That is why this file exists.

use guff_types::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::check_lookup::implements as strict_implements;
use guff_types::lookup::lookup_field_or_method;
use guff_types::ObjectId;

use crate::unify::{unify, Unifier};

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
    // `mapping := map[*types.TypeParam]types.Type{}`, shared by every method.
    let mut mapping = Unifier::new();
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
        if !unify(types, objects, impl_sig, *iface_sig, &mut mapping) {
            return None;
        }
        out.push(found);
    }
    // "This checks constraints on a best-effort basis, erring on the side of
    // accepting too many types." A parameter unified only with another
    // parameter has no type (`t == nil`) and passes.
    let bound: Vec<(TypeId, TypeId)> =
        mapping.into_iter().filter_map(|(tp, t)| t.map(|t| (tp, t))).collect();
    for (tp, targ) in bound {
        let constraint = match types.get(tp) {
            TypeData::TypeParam(p) => p.constraint(),
            _ => None,
        };
        if let Some(c) = constraint {
            if strict_implements(types, objects, packages, targ, c, true).is_err() {
                return None;
            }
        }
    }
    Some(out)
}
