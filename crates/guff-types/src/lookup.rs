//! Port of `cmd/compile/internal/types2/lookup.go`.
//!
//! The selector-resolution machinery: given a receiver type `T` and a name
//! `f`, find the field or method (and the embedding-index path that gets
//! you there). [`lookup_field_or_method`] is the top-level entry point;
//! [`lookup_selection`] wraps the result into a [`Selection`].
//!
//! ## Decoupling from `Checker`
//!
//! `lookup.go`'s core algorithm (`lookupFieldOrMethodImpl`) is purely
//! structural — no Checker state. The Checker-dependent helpers
//! (`MissingMethod`, `hasAllMethods`, `assertableTo`, `interfacePtrError`,
//! `funcString`) are deferred until the Checker chunk lands; they're
//! comments-only here as a forward-pointer.
//!
//! Like `selection.rs`, all entry points take the arenas explicitly so
//! callers don't need a `Checker` to build with.

use crate::alias::unalias_readonly;
use crate::arena::{
    ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, TypeArena, TypeData, TypeId,
};
use crate::interface::interface_compute_typeset;
use crate::named::named_lookup_method;
use crate::object::func::func_has_ptr_recv;
use crate::predicates::{identical, is_interface, is_valid};
use crate::selection::{Selection, SelectionKind};

// ---------------------------------------------------------------------------
// Outcome types

/// Outcome of [`lookup_field_or_method`] (or its lower-level
/// [`lookup_field_or_method_impl`]).
///
/// Mirrors Go's `(obj Object, index []int, indirect bool)` return —
/// in particular the "collision" case (`obj == nil`, `index != nil`).
#[derive(Debug, Clone)]
pub enum LookupResult {
    /// Found a matching field or method.
    Found {
        obj: ObjectId,
        index: Vec<i32>,
        indirect: bool,
    },
    /// Multiple matches at the same embedding depth (ambiguous selector).
    /// `index` points at the entry that caused the collision.
    Ambiguous { index: Vec<i32> },
    /// A method with a pointer receiver was found, but the receiver wasn't
    /// addressable. Selecting this method through a non-pointer
    /// non-addressable value is rejected by the spec.
    PtrRecvRequired,
    /// No match.
    NotFound,
}

impl LookupResult {
    pub fn found(&self) -> Option<(ObjectId, &[i32], bool)> {
        match self {
            LookupResult::Found {
                obj,
                index,
                indirect,
            } => Some((*obj, index, *indirect)),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Public entry points

/// Look up a field or method with the given package and name on type `T`.
///
/// `addressable` matters only for method lookups: methods with pointer
/// receivers can be selected through a non-pointer receiver if the value
/// is addressable.
///
/// Equivalent to `LookupFieldOrMethod`. `T` must be a valid `TypeId`.
pub fn lookup_field_or_method(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    pkg: Option<PackageId>,
    name: &str,
) -> LookupResult {
    lookup_field_or_method_inner(
        type_arena,
        object_arena,
        package_arena,
        t,
        addressable,
        pkg,
        name,
        false,
    )
}

/// Variant that accepts the `fold_case` flag — for case-insensitive
/// matching (used by `missingMethod` to suggest "wrong case" candidates).
///
/// Equivalent to the unexported `lookupFieldOrMethod` in Go.
pub fn lookup_field_or_method_fold(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    pkg: Option<PackageId>,
    name: &str,
    fold_case: bool,
) -> LookupResult {
    lookup_field_or_method_inner(
        type_arena,
        object_arena,
        package_arena,
        t,
        addressable,
        pkg,
        name,
        fold_case,
    )
}

/// Like [`lookup_field_or_method`] but wraps the result in a [`Selection`].
///
/// Equivalent to `LookupSelection`. Returns `None` for `NotFound`,
/// `Ambiguous`, or `PtrRecvRequired` outcomes.
pub fn lookup_selection(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    pkg: Option<PackageId>,
    name: &str,
) -> Option<Selection> {
    match lookup_field_or_method(
        type_arena,
        object_arena,
        package_arena,
        t,
        addressable,
        pkg,
        name,
    ) {
        LookupResult::Found {
            obj,
            index,
            indirect,
        } => {
            let kind = match object_arena.get(obj) {
                ObjectData::Var(_) => SelectionKind::FieldVal,
                ObjectData::Func(_) => SelectionKind::MethodVal,
                _ => return None,
            };
            Some(Selection::new(kind, t, obj, index, indirect))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Implementation

/// Top-level wrapper handling the "methods on a named pointer type" special
/// case (Go issue #8590): a method declared on `*X` isn't visible through a
/// named pointer type `type T *X`. Falls through to
/// [`lookup_field_or_method_impl`] for the actual BFS.
fn lookup_field_or_method_inner(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    pkg: Option<PackageId>,
    name: &str,
    fold_case: bool,
) -> LookupResult {
    // If T is a named pointer type, look up through the underlying pointer
    // but discard any method result (methods of T cannot be defined here).
    if let Some(_named) = as_named(type_arena, t) {
        let u = t.underlying(type_arena);
        if matches!(type_arena.get(u), TypeData::Pointer(_)) {
            let result = lookup_field_or_method_impl(
                type_arena,
                object_arena,
                package_arena,
                u,
                false,
                pkg,
                name,
                fold_case,
            );
            if let LookupResult::Found { obj, .. } = &result {
                if matches!(object_arena.get(*obj), ObjectData::Func(_)) {
                    return LookupResult::NotFound;
                }
            }
            return result;
        }
    }
    lookup_field_or_method_impl(
        type_arena,
        object_arena,
        package_arena,
        t,
        addressable,
        pkg,
        name,
        fold_case,
    )
    // Note: Go's `enableTParamFieldLookup` block is gated `false` upstream
    // (see go.dev/issue/51576); we omit it.
}

#[derive(Debug, Clone)]
struct EmbeddedType {
    typ: TypeId,
    index: Vec<i32>,
    indirect: bool,
    multiples: bool,
}

/// The BFS through embedded types. See Go's docstring — extremely subtle.
fn lookup_field_or_method_impl(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    t: TypeId,
    addressable: bool,
    pkg: Option<PackageId>,
    name: &str,
    fold_case: bool,
) -> LookupResult {
    if name == "_" {
        return LookupResult::NotFound;
    }

    // Dereference once if T is a *Pointer (but not a named pointer).
    let (typ0, is_ptr0) = deref(type_arena, t);
    // *typ where typ is an interface (incl. a type parameter) has no methods.
    if is_ptr0 && is_interface(type_arena, typ0) {
        return LookupResult::NotFound;
    }

    let mut current: Vec<EmbeddedType> = vec![EmbeddedType {
        typ: typ0,
        index: Vec::new(),
        indirect: is_ptr0,
        multiples: false,
    }];
    let mut seen = InstanceLookup::new();

    let mut found_obj: Option<ObjectId> = None;
    let mut found_index: Vec<i32> = Vec::new();
    let mut found_indirect = false;

    while !current.is_empty() {
        let mut next: Vec<EmbeddedType> = Vec::new();

        for e in &current {
            let typ = e.typ;

            // Try named-type methods first.
            if let Some(named_id) = as_named(type_arena, typ) {
                if let Some(_alt) = seen.lookup(type_arena, object_arena, package_arena, named_id) {
                    continue; // already seen at a shallower depth
                }
                seen.add(type_arena, named_id);

                if let Some((i, m)) = named_lookup_method(
                    type_arena,
                    object_arena,
                    package_arena,
                    named_id,
                    pkg,
                    name,
                    fold_case,
                ) {
                    let idx = concat(&e.index, i as i32);
                    if found_obj.is_some() || e.multiples {
                        return LookupResult::Ambiguous { index: idx };
                    }
                    found_obj = Some(m);
                    found_index = idx;
                    found_indirect = e.indirect;
                    continue;
                }
            }

            // Look at underlying.
            let u = typ.underlying(type_arena);
            match type_arena.get(u) {
                TypeData::Struct(s) => {
                    // Snapshot fields + embedded flags so we don't hold a
                    // type-arena borrow across recursive calls / mutations.
                    let n = s.num_fields();
                    let fields: Vec<ObjectId> = (0..n).map(|i| s.field(i)).collect();
                    for (i, f) in fields.iter().copied().enumerate() {
                        if f.same_id(object_arena, package_arena, pkg, name, fold_case) {
                            let idx = concat(&e.index, i as i32);
                            if found_obj.is_some() || e.multiples {
                                return LookupResult::Ambiguous { index: idx };
                            }
                            found_obj = Some(f);
                            found_index = idx;
                            found_indirect = e.indirect;
                            continue;
                        }
                        // Collect embedded struct fields for next depth — only
                        // if we haven't already matched.
                        if found_obj.is_none() {
                            let embedded = match object_arena.get(f) {
                                ObjectData::Var(v) => v.embedded(),
                                _ => false,
                            };
                            if embedded {
                                let ftyp = f.typ(object_arena).expect("Var has typ");
                                let (etyp, eis_ptr) = deref(type_arena, ftyp);
                                next.push(EmbeddedType {
                                    typ: etyp,
                                    index: concat(&e.index, i as i32),
                                    indirect: e.indirect || eis_ptr,
                                    multiples: e.multiples,
                                });
                            }
                        }
                    }
                }
                TypeData::Interface(_) => {
                    interface_compute_typeset(type_arena, object_arena, package_arena, u);
                    let lookup_result: Option<(usize, ObjectId)> = match type_arena.get(u) {
                        TypeData::Interface(i) => i.tset.as_ref().and_then(|ts| {
                            ts.lookup_method(object_arena, package_arena, pkg, name, fold_case)
                        }),
                        _ => unreachable!(),
                    };
                    if let Some((i, m)) = lookup_result {
                        let idx = concat(&e.index, i as i32);
                        if found_obj.is_some() || e.multiples {
                            return LookupResult::Ambiguous { index: idx };
                        }
                        found_obj = Some(m);
                        found_index = idx;
                        found_indirect = e.indirect;
                    }
                }
                _ => {}
            }
        }

        if let Some(obj) = found_obj {
            // Pointer-receiver check.
            if let ObjectData::Func(_) = object_arena.get(obj) {
                if func_has_ptr_recv(type_arena, object_arena, obj)
                    && !found_indirect
                    && !addressable
                {
                    return LookupResult::PtrRecvRequired;
                }
            }
            return LookupResult::Found {
                obj,
                index: found_index,
                indirect: found_indirect,
            };
        }

        current = consolidate_multiples(type_arena, object_arena, package_arena, next);
    }

    LookupResult::NotFound
}

// ---------------------------------------------------------------------------
// Helpers used by the BFS

fn consolidate_multiples(
    type_arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    list: Vec<EmbeddedType>,
) -> Vec<EmbeddedType> {
    if list.len() <= 1 {
        return list;
    }

    let mut out: Vec<EmbeddedType> = Vec::with_capacity(list.len());
    for e in list.into_iter() {
        // Look up an existing entry with the same type.
        let mut hit: Option<usize> = None;
        for (j, existing) in out.iter().enumerate() {
            if existing.typ == e.typ
                || identical(type_arena, object_arena, package_arena, existing.typ, e.typ)
            {
                hit = Some(j);
                break;
            }
        }
        match hit {
            Some(j) => out[j].multiples = true,
            None => out.push(e),
        }
    }
    out
}

/// Backing store for the "have we seen this Named already?" check. Matches
/// Go's `instanceLookup` — small fixed-size buffer with a HashMap fallback,
/// using `Identical` for instantiated-type equivalence.
struct InstanceLookup {
    buf: [Option<TypeId>; 3],
    overflow: crate::hash::HashMap<ObjectId, Vec<TypeId>>,
}

impl InstanceLookup {
    fn new() -> Self {
        Self {
            buf: [None, None, None],
            overflow: crate::hash::HashMap::default(),
        }
    }

    fn lookup(
        &self,
        type_arena: &mut TypeArena,
        object_arena: &ObjectArena,
        package_arena: &PackageArena,
        inst: TypeId,
    ) -> Option<TypeId> {
        for slot in &self.buf {
            if let Some(t) = slot {
                if identical(type_arena, object_arena, package_arena, inst, *t) {
                    return Some(*t);
                }
            }
        }
        // Look up under inst's Origin's TypeName object.
        let origin = crate::named::named_origin(type_arena, inst);
        let origin_obj = match type_arena.get(origin) {
            TypeData::Named(n) => n.obj(),
            _ => return None,
        };
        let candidates: Vec<TypeId> = self.overflow.get(&origin_obj).cloned().unwrap_or_default();
        for c in candidates {
            if identical(type_arena, object_arena, package_arena, inst, c) {
                return Some(c);
            }
        }
        None
    }

    fn add(&mut self, type_arena: &TypeArena, inst: TypeId) {
        for slot in &mut self.buf {
            if slot.is_none() {
                *slot = Some(inst);
                return;
            }
        }
        let origin = crate::named::named_origin(type_arena, inst);
        let origin_obj = match type_arena.get(origin) {
            TypeData::Named(n) => n.obj(),
            _ => return,
        };
        self.overflow.entry(origin_obj).or_default().push(inst);
    }
}

// ---------------------------------------------------------------------------
// Small utility predicates / helpers

/// Returns `t` as a Named `TypeId` if `unalias(t)` is a Named, else `None`.
///
/// Equivalent to `asNamed`.
pub fn as_named(arena: &TypeArena, t: TypeId) -> Option<TypeId> {
    let u = unalias_readonly(arena, t);
    match arena.get(u) {
        TypeData::Named(_) => Some(u),
        _ => None,
    }
}

/// Dereference `t` if it is a `*Pointer` (but not a named pointer type).
/// Returns `(base, true)` for a Pointer; otherwise `(t, false)`.
///
/// Equivalent to `deref`.
pub fn deref(arena: &TypeArena, t: TypeId) -> (TypeId, bool) {
    let u = unalias_readonly(arena, t);
    match arena.get(u) {
        TypeData::Pointer(p) => (p.elem(), true),
        _ => (t, false),
    }
}

/// Dereference `t` if its underlying is a Pointer-to-Struct. Otherwise
/// returns `t` unchanged.
///
/// Equivalent to `derefStructPtr`.
pub fn deref_struct_ptr(arena: &TypeArena, t: TypeId) -> TypeId {
    let u = t.underlying(arena);
    if let TypeData::Pointer(p) = arena.get(u) {
        let base = p.elem();
        if matches!(arena.get(base.underlying(arena)), TypeData::Struct(_)) {
            return base;
        }
    }
    t
}

/// Reports whether `T` is a pointer to an interface (or a pointer to a
/// type parameter, which the language also rejects).
///
/// Equivalent to `isInterfacePtr`.
pub fn is_interface_ptr(arena: &TypeArena, t: TypeId) -> bool {
    let u = t.underlying(arena);
    let base = match arena.get(u) {
        TypeData::Pointer(p) => p.elem(),
        _ => return false,
    };
    is_interface(arena, base)
}

/// Reports whether `T` is a struct (or pointer to a struct) containing —
/// directly or indirectly — embedded fields with invalid types. Used to
/// avoid follow-on errors during method-set checks.
///
/// Equivalent to `hasInvalidEmbeddedFields`.
pub fn has_invalid_embedded_fields(
    type_arena: &TypeArena,
    object_arena: &ObjectArena,
    t: TypeId,
) -> bool {
    let mut seen: crate::hash::HashSet<TypeId> = crate::hash::HashSet::default();
    has_invalid_embedded_fields_inner(type_arena, object_arena, t, &mut seen)
}

fn has_invalid_embedded_fields_inner(
    type_arena: &TypeArena,
    object_arena: &ObjectArena,
    t: TypeId,
    seen: &mut crate::hash::HashSet<TypeId>,
) -> bool {
    let s_id = {
        let base = deref_struct_ptr(type_arena, t).underlying(type_arena);
        match type_arena.get(base) {
            TypeData::Struct(_) => base,
            _ => return false,
        }
    };
    if !seen.insert(s_id) {
        return false;
    }
    let fields: Vec<ObjectId> = match type_arena.get(s_id) {
        TypeData::Struct(s) => (0..s.num_fields()).map(|i| s.field(i)).collect(),
        _ => unreachable!(),
    };
    for f in fields {
        let v = match object_arena.get(f) {
            ObjectData::Var(v) => v,
            _ => panic!("has_invalid_embedded_fields: non-Var field in Struct"),
        };
        if !v.embedded() {
            continue;
        }
        let ftyp = v.typ();
        if !is_valid(type_arena, ftyp)
            || has_invalid_embedded_fields_inner(type_arena, object_arena, ftyp, seen)
        {
            return true;
        }
    }
    false
}

/// Concatenate `list` and `i` into a fresh `Vec`.
///
/// Equivalent to `concat`.
pub fn concat(list: &[i32], i: i32) -> Vec<i32> {
    let mut out = Vec::with_capacity(list.len() + 1);
    out.extend_from_slice(list);
    out.push(i);
    out
}

/// Return the index of the field with matching package and name, or `None`.
///
/// Equivalent to `fieldIndex`.
pub fn field_index(
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    fields: &[ObjectId],
    pkg: Option<PackageId>,
    name: &str,
    fold_case: bool,
) -> Option<usize> {
    if name == "_" {
        return None;
    }
    for (i, f) in fields.iter().enumerate() {
        if f.same_id(object_arena, package_arena, pkg, name, fold_case) {
            return Some(i);
        }
    }
    None
}

/// Return the index of the method with matching package and name, or `None`.
///
/// Equivalent to `methodIndex`.
pub fn method_index(
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    methods: &[ObjectId],
    pkg: Option<PackageId>,
    name: &str,
    fold_case: bool,
) -> Option<(usize, ObjectId)> {
    if name == "_" {
        return None;
    }
    for (i, m) in methods.iter().enumerate() {
        if m.same_id(object_arena, package_arena, pkg, name, fold_case) {
            return Some((i, *m));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Forward-pointer: Checker-dependent helpers deferred to later chunks.
//
// - `missingMethod` / `MissingMethod` — needs Checker.objDecl + funcString
//   (typestring.go) for error rendering.
// - `hasAllMethods` — depends on missingMethod.
// - `assertableTo` / `newAssertableTo` — depend on hasAllMethods +
//   Checker.implements (decl.go path).
// - `interfacePtrError` — needs Checker.sprintf.
// - `funcString` — needs newTypeWriter (typestring.go).
//
// They live in this comment block instead of in a stub to avoid bit-rot;
// reference Go's lookup.go when porting.
