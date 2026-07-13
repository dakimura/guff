//! Lock-path analysis for the `copylocks` pass.

use std::collections::HashSet;

use guff_analysis::Pass;
use guff_types::arena::{ObjectArena, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::lookup::lookup_field_or_method;
use guff_types::pointer::new_pointer;
use guff_types::{struct_field, struct_num_fields};
use guff_types::typestring::type_string;

/// A path describing where a lock lives inside a type.
pub type LockPath = Vec<String>;

pub fn lock_path_rhs(pass: &Pass<'_>, typ: TypeId) -> Option<LockPath> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !guff_types::predicates::is_valid(&artifacts.types, typ) {
        return None;
    }
    let mut seen = HashSet::new();
    lock_path(pass, typ, &mut seen)
}

fn lock_path(pass: &Pass<'_>, typ: TypeId, seen: &mut HashSet<TypeId>) -> Option<LockPath> {
    if seen.contains(&typ) {
        return None;
    }
    seen.insert(typ);

    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let types = &artifacts.types;
    let objects = &artifacts.objects;
    let packages = &artifacts.packages;

    if is_named_type(types, objects, packages, typ, "sync", "noCopy") {
        return Some(vec![type_name(types, objects, packages, typ)]);
    }

    if is_lock_by_value(types, objects, packages, typ) {
        return Some(vec![type_name(types, objects, packages, typ)]);
    }

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
        for i in 0..struct_num_fields(types, u) {
            let fobj = struct_field(types, u, i);
            let Some(ftyp) = fobj.typ(objects) else {
                continue;
            };
            if let Some(mut sub) = lock_path(pass, ftyp, seen) {
                sub.push(type_name(types, objects, packages, typ));
                return Some(sub);
            }
        }
    }

    None
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

fn is_lock_by_value(
    types: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
) -> bool {
    let mut arena = types.clone();
    let ptr = new_pointer(&mut arena, typ);
    has_locker_methods(&mut arena, objects, packages, ptr)
        && !has_locker_methods(&mut arena, objects, packages, typ)
}

fn has_locker_methods(
    types: &mut TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
) -> bool {
    lookup_field_or_method(types, objects, packages, typ, false, None, "Lock")
        .found()
        .is_some()
        && lookup_field_or_method(types, objects, packages, typ, false, None, "Unlock")
            .found()
            .is_some()
}
