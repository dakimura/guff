//! Lock-path analysis for the `copylocks` pass.

use std::collections::{HashMap, HashSet};

use guff_analysis::Pass;
use guff_types::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::lookup::lookup_field_or_method;
use guff_types::{struct_field, struct_num_fields};
use guff_types::typestring::type_string;

/// A path describing where a lock lives inside a type.
pub type LockPath = Vec<String>;

/// Per-package scratch + memo for lock-path queries.
///
/// `copylocks` calls [`LockChecker::lock_path_rhs`] at every assignment,
/// declaration, literal, return, call argument, parameter and range variable in
/// a package — thousands of sites in a large package. Each query recurses over a
/// type's fields, and every field previously re-cloned the *entire* type arena
/// (once per visited node) so `lookup_field_or_method` could intern method sets.
/// On big packages (e.g. Prometheus `tsdb`, ~40k LoC with large test files) that
/// made `copylocks` take tens of seconds and dominated the analyze phase.
///
/// `LockChecker` fixes this two ways:
/// * `scratch` clones the type arena *once per package* (lazily) and reuses that
///   single mutable copy for all method lookups (they only append, so sharing is
///   sound), instead of cloning per node.
/// * `memo` caches each type's resolved lock path, so shared types across the
///   thousands of call sites are computed only once.
#[derive(Default)]
pub struct LockChecker {
    /// One mutable clone of the package type arena, created on first use.
    scratch: Option<TypeArena>,
    /// Memoized lock paths keyed by type id.
    memo: HashMap<TypeId, Option<LockPath>>,
}

impl LockChecker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lock_path_rhs(&mut self, pass: &Pass<'_>, typ: TypeId) -> Option<LockPath> {
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        if !guff_types::predicates::is_valid(&artifacts.types, typ) {
            return None;
        }
        let mut seen = HashSet::new();
        self.lock_path(pass, typ, &mut seen)
    }

    fn lock_path(
        &mut self,
        pass: &Pass<'_>,
        typ: TypeId,
        seen: &mut HashSet<TypeId>,
    ) -> Option<LockPath> {
        if let Some(cached) = self.memo.get(&typ) {
            return cached.clone();
        }
        if seen.contains(&typ) {
            // Currently being resolved on this path (recursive type) — treat as
            // no lock, matching the previous cycle-guard behavior. Do not cache:
            // the true result is decided by the outermost call.
            return None;
        }
        seen.insert(typ);

        let result = self.lock_path_uncached(pass, typ, seen);
        seen.remove(&typ);
        self.memo.insert(typ, result.clone());
        result
    }

    fn lock_path_uncached(
        &mut self,
        pass: &Pass<'_>,
        typ: TypeId,
        seen: &mut HashSet<TypeId>,
    ) -> Option<LockPath> {
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        let types = &artifacts.types;
        let objects = &artifacts.objects;
        let packages = &artifacts.packages;

        if is_named_type(types, objects, packages, typ, "sync", "noCopy") {
            return Some(vec![type_name(types, objects, packages, typ)]);
        }

        if self.is_lock_by_value(pass, typ) {
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            return Some(vec![type_name(
                &artifacts.types,
                &artifacts.objects,
                &artifacts.packages,
                typ,
            )]);
        }

        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        let types = &artifacts.types;
        let objects = &artifacts.objects;
        let packages = &artifacts.packages;

        let mut cur = typ;
        loop {
            let u = cur.underlying(types);
            if let TypeData::Array(a) = types.get(u) {
                cur = a.elem();
                continue;
            }
            break;
        }

        let u = cur.underlying(types);
        if matches!(types.get(u), TypeData::Struct(_)) {
            let nfields = struct_num_fields(types, u);
            for i in 0..nfields {
                let fobj = struct_field(types, u, i);
                let Some(ftyp) = fobj.typ(objects) else {
                    continue;
                };
                if let Some(mut sub) = self.lock_path(pass, ftyp, seen) {
                    let artifacts = pass.pkg().type_artifacts.as_ref()?;
                    sub.push(type_name(
                        &artifacts.types,
                        &artifacts.objects,
                        &artifacts.packages,
                        typ,
                    ));
                    return Some(sub);
                }
            }
        }

        None
    }

    /// `*T` has `Lock`+`Unlock` but `T` does not — i.e. the lock methods take a
    /// pointer receiver, so copying the value is a bug.
    fn is_lock_by_value(&mut self, pass: &Pass<'_>, typ: TypeId) -> bool {
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        let scratch = self
            .scratch
            .get_or_insert_with(|| artifacts.types.clone());
        let objects = &artifacts.objects;
        let packages = &artifacts.packages;
        // Addressable method set of `T` == method set of `*T` (value + pointer
        // receivers); non-addressable == value receivers only. This reproduces
        // the old `new_pointer` + double lookup without materializing `*T`.
        has_locker_methods(scratch, objects, packages, typ, true)
            && !has_locker_methods(scratch, objects, packages, typ, false)
    }
}

pub fn lock_path_display(path: &LockPath) -> String {
    path.iter()
        .rev()
        .cloned()
        .collect::<Vec<_>>()
        .join(" contains ")
}

fn type_name(
    types: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
) -> String {
    type_string(types, objects, packages, typ, None)
}

fn is_named_type(
    types: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
    pkg: &str,
    name: &str,
) -> bool {
    let u = typ.underlying(types);
    let TypeData::Named(n) = types.get(u) else {
        return false;
    };
    let obj = n.obj();
    let Some(pid) = obj.pkg(objects) else {
        return false;
    };
    packages.get(pid).path() == pkg && obj.name(objects) == name
}

fn has_locker_methods(
    types: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
    addressable: bool,
) -> bool {
    lookup_field_or_method(types, objects, packages, typ, addressable, None, "Lock")
        .found()
        .is_some()
        && lookup_field_or_method(types, objects, packages, typ, addressable, None, "Unlock")
            .found()
            .is_some()
}
