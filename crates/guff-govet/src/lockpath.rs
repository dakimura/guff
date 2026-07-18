//! Lock-path analysis for the `copylocks` pass.

use std::collections::{HashMap, HashSet};

use guff_analysis::Pass;
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, TypeArena, TypeData, TypeId};
use guff_types::lookup::lookup_field_or_method;
use guff_types::{
    as_named, concat, deref, func_has_ptr_recv, is_interface, named_lookup_method, struct_field,
    struct_num_fields, LookupResult,
};
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
/// `LockChecker` fixes this three ways:
/// * `is_lock_by_value` first tries a *read-only* method-set probe
///   ([`find_method_ro`]) against the shared package arena, so the common lock
///   shapes (named types like `sync.Mutex`, structs embedding them) never clone
///   the arena at all. With R24.3's shared export seed the type arena holds the
///   union of every root package's dependencies, so a single clone was ~1.6s and
///   many concurrent clones dominated peak memory on a full run.
/// * `scratch` clones the type arena *once per package* (lazily) only as a
///   fallback for the rare shapes the read-only probe can't resolve (embedded
///   interfaces, generic instances), reusing that single mutable copy.
/// * `memo` caches each type's resolved lock path, so shared types across the
///   thousands of call sites are computed only once.
#[derive(Default)]
pub struct LockChecker {
    /// One mutable clone of the package type arena, created on first fallback.
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
    ///
    /// Addressable method set of `T` == method set of `*T` (value + pointer
    /// receivers); non-addressable == value receivers only. This reproduces the
    /// old `new_pointer` + double lookup without materializing `*T`.
    fn is_lock_by_value(&mut self, pass: &Pass<'_>, typ: TypeId) -> bool {
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        // Read-only fast path: no arena clone. `locker_ro` returns `None` only
        // for shapes it can't resolve read-only (embedded interfaces, generic
        // instances), where we fall back to the cloned mutable lookup below.
        {
            let types = &artifacts.types;
            let objects = &artifacts.objects;
            let packages = &artifacts.packages;
            if let (Some(addr), Some(val)) = (
                locker_ro(types, objects, packages, typ, true),
                locker_ro(types, objects, packages, typ, false),
            ) {
                return addr && !val;
            }
        }

        // Fallback: clone the arena once and use the mutable lookup, which
        // computes any missing interface type sets the read-only probe couldn't.
        let scratch = self
            .scratch
            .get_or_insert_with(|| artifacts.types.clone());
        let objects = &artifacts.objects;
        let packages = &artifacts.packages;
        has_locker_methods(scratch, objects, packages, typ, true)
            && !has_locker_methods(scratch, objects, packages, typ, false)
    }
}

/// Read-only: does the (addressable or value) method set of `typ` contain both
/// `Lock` and `Unlock`? Returns `None` when the shape needs the mutable lookup
/// (embedded interface, generic instance) — the caller then falls back to the
/// cloned arena. Mirrors `has_locker_methods` without mutating the arena.
fn locker_ro(
    types: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    typ: TypeId,
    addressable: bool,
) -> Option<bool> {
    let lock = find_method_ro(types, objects, packages, typ, addressable, "Lock")?;
    if lock.found().is_none() {
        return Some(false);
    }
    let unlock = find_method_ro(types, objects, packages, typ, addressable, "Unlock")?;
    Some(unlock.found().is_some())
}

/// One BFS frontier entry, mirroring `lookup::EmbeddedType`.
struct EmbeddedRo {
    typ: TypeId,
    index: Vec<i32>,
    indirect: bool,
    multiples: bool,
}

/// Read-only port of `lookup_field_or_method` restricted to what `copylocks`
/// needs: find a field or method named `name` on `typ`'s method set. Returns
/// `Some(result)` when it can decide by structural reads alone, or `None` when
/// it hits a shape whose resolution needs the mutable arena — an embedded
/// interface (its method set requires computing a type set) or a generic
/// instance (dedup across the BFS frontier uses `identical`). Callers fall back
/// to the cloned mutable [`lookup_field_or_method`] for those.
///
/// The frontier dedup ([`consolidate_ro`]) uses only `TypeId` equality, not
/// `identical`; the two differ only for distinct-but-structurally-identical
/// sibling embeddings, which cannot occur for the `Lock`/`Unlock` lookup on
/// real code.
fn find_method_ro(
    type_arena: &TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    name: &str,
) -> Option<LookupResult> {
    // Named pointer type (`type T *X`): methods of `*X` aren't visible here.
    // This is subtle enough that we defer to the mutable lookup.
    if as_named(type_arena, t).is_some() {
        let u = t.underlying(type_arena);
        if matches!(type_arena.get(u), TypeData::Pointer(_)) {
            return None;
        }
    }

    // Dereference once if `t` is a `*Pointer` (not a named pointer).
    let (typ0, is_ptr0) = deref(type_arena, t);
    if is_ptr0 && is_interface(type_arena, typ0) {
        return Some(LookupResult::NotFound);
    }

    let mut current = vec![EmbeddedRo {
        typ: typ0,
        index: Vec::new(),
        indirect: is_ptr0,
        multiples: false,
    }];
    let mut seen: HashSet<TypeId> = HashSet::new();

    let mut found_obj = None;
    let mut found_index: Vec<i32> = Vec::new();
    let mut found_indirect = false;

    while !current.is_empty() {
        let mut next: Vec<EmbeddedRo> = Vec::new();

        for e in &current {
            let typ = e.typ;

            // Named-type methods first.
            if let Some(named_id) = as_named(type_arena, typ) {
                if let TypeData::Named(n) = type_arena.get(named_id) {
                    if n.instance().is_some() {
                        return None; // generic instance: needs `identical` dedup
                    }
                }
                if !seen.insert(named_id) {
                    continue;
                }
                if let Some((i, m)) = named_lookup_method(
                    type_arena,
                    object_arena,
                    package_arena,
                    named_id,
                    None,
                    name,
                    false,
                ) {
                    let idx = concat(&e.index, i as i32);
                    if found_obj.is_some() || e.multiples {
                        return Some(LookupResult::Ambiguous { index: idx });
                    }
                    found_obj = Some(m);
                    found_index = idx;
                    found_indirect = e.indirect;
                    continue;
                }
            }

            // Underlying: struct fields (recurse) or interface (bail).
            let u = typ.underlying(type_arena);
            let fields: Vec<guff_types::ObjectId> = match type_arena.get(u) {
                TypeData::Struct(s) => {
                    let n = s.num_fields();
                    (0..n).map(|i| s.field(i)).collect()
                }
                TypeData::Interface(_) => return None,
                _ => continue,
            };
            for (i, f) in fields.into_iter().enumerate() {
                if f.same_id(object_arena, package_arena, None, name, false) {
                    let idx = concat(&e.index, i as i32);
                    if found_obj.is_some() || e.multiples {
                        return Some(LookupResult::Ambiguous { index: idx });
                    }
                    found_obj = Some(f);
                    found_index = idx;
                    found_indirect = e.indirect;
                    continue;
                }
                if found_obj.is_none() {
                    let embedded = matches!(
                        object_arena.get(f),
                        ObjectData::Var(v) if v.embedded()
                    );
                    if embedded {
                        let ftyp = match object_arena.get(f) {
                            ObjectData::Var(v) => v.typ(),
                            _ => unreachable!("embedded field is a Var"),
                        };
                        let (etyp, eis_ptr) = deref(type_arena, ftyp);
                        next.push(EmbeddedRo {
                            typ: etyp,
                            index: concat(&e.index, i as i32),
                            indirect: e.indirect || eis_ptr,
                            multiples: e.multiples,
                        });
                    }
                }
            }
        }

        if let Some(obj) = found_obj {
            if let ObjectData::Func(_) = object_arena.get(obj) {
                if func_has_ptr_recv(type_arena, object_arena, obj)
                    && !found_indirect
                    && !addressable
                {
                    return Some(LookupResult::PtrRecvRequired);
                }
            }
            return Some(LookupResult::Found {
                obj,
                index: found_index,
                indirect: found_indirect,
            });
        }

        current = consolidate_ro(next);
    }

    Some(LookupResult::NotFound)
}

/// Deduplicate a BFS frontier by `TypeId`, marking `multiples` on collisions —
/// the read-only analog of `lookup::consolidate_multiples` without `identical`.
fn consolidate_ro(list: Vec<EmbeddedRo>) -> Vec<EmbeddedRo> {
    if list.len() <= 1 {
        return list;
    }
    let mut out: Vec<EmbeddedRo> = Vec::with_capacity(list.len());
    for e in list {
        if let Some(j) = out.iter().position(|x| x.typ == e.typ) {
            out[j].multiples = true;
        } else {
            out.push(e);
        }
    }
    out
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
