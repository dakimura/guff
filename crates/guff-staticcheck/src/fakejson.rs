//! Static JSON marshalability check (no reflection).
//!
//! Port of `honnef.co/go/tools/staticcheck/fakejson` for SA1026 — a copy of
//! `encoding/json`'s encoder with reflection replaced by go/types, so it keeps
//! json's rules for tags, shadowing and addressability.

use std::collections::{HashMap, HashSet};

use guff_analysis::callcheck::render_type;
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::object::is_exported;
use guff_types::TypeId;

/// Answers upstream's `t.Implements(knowledge.Interfaces[…])` and
/// `fakereflect.PtrTo(t).Implements(…)` — a method set question the type arena
/// alone cannot answer, so the caller supplies it.
pub trait MarshalerLookup {
    /// Does the method set of `typ` (or of `*typ` when `ptr`) hold `method`
    /// with the signature `func() ([]byte, error)`?
    fn implements(&self, typ: TypeId, method: &str, ptr: bool) -> bool;
}

/// Error returned when a type cannot be JSON-marshaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedTypeError {
    pub typ: TypeId,
    pub path: String,
}

/// Returns an error if `typ` cannot be JSON-marshaled (Go `fakejson.Marshal`).
pub fn marshal(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    lookup: &dyn MarshalerLookup,
    typ: TypeId,
) -> Option<UnsupportedTypeError> {
    let mut enc = Encoder::default();
    // `fakejson.Marshal` starts from `fakereflect.TypeAndCanAddr{Type: v}`,
    // whose `canAddr` is the zero value — **false**. It matters: the
    // `PtrTo(t).Implements(…)` short-circuits are gated on it, so a type whose
    // `MarshalJSON` has a pointer receiver is still walked when the argument
    // was passed by value.
    enc.check_type(arena, objects, packages, lookup, typ, false, "x".to_string())
}

#[derive(Default)]
struct Encoder {
    seen_addr: HashSet<TypeId>,
    seen_no_addr: HashSet<TypeId>,
}

impl Encoder {
    fn check_type(
        &mut self,
        arena: &TypeArena,
        objects: &ObjectArena,
        packages: &PackageArena,
        lookup: &dyn MarshalerLookup,
        typ: TypeId,
        can_addr: bool,
        path: String,
    ) -> Option<UnsupportedTypeError> {
        let seen = if can_addr {
            &mut self.seen_addr
        } else {
            &mut self.seen_no_addr
        };
        if !seen.insert(typ) {
            return None;
        }

        // Four short-circuits, in upstream's order. A type that marshals
        // itself is never walked, so the `chan` inside it is not a finding.
        //
        //     if t.Implements(Interfaces["encoding/json.Marshaler"]) { return nil }
        //     if !t.IsPtr() && t.CanAddr() && PtrTo(t).Implements(…) { return nil }
        //     if t.Implements(Interfaces["encoding.TextMarshaler"]) { return nil }
        //     if !t.IsPtr() && t.CanAddr() && PtrTo(t).Implements(…) { return nil }
        let u = unalias_readonly(arena, typ).underlying(arena);
        let is_ptr = matches!(arena.get(u), TypeData::Pointer(_));
        for method in ["MarshalJSON", "MarshalText"] {
            if lookup.implements(typ, method, false) {
                return None;
            }
            if !is_ptr && can_addr && lookup.implements(typ, method, true) {
                return None;
            }
        }
        match arena.get(u) {
            TypeData::Basic(_) | TypeData::Interface(_) => None,
            TypeData::Struct(_) => {
                type_fields(self, arena, objects, packages, lookup, typ, can_addr, &path)
            }
            TypeData::Map(m) => {
                // `if typeparams.IsTypeParam(t.Key().Type)` — upstream skips
                // the key check entirely and walks straight to the element.
                // Its own comment says why: nothing is known about the concrete
                // instantiation, and the key might well implement
                // TextMarshaler. `json.Marshal(m.keyToValue)` on a
                // `map[K]V` field is ava-labs/avalanchego's `BiMap`.
                if !guff_types::predicates::is_type_param(arena, m.key())
                    && !map_key_ok(arena, lookup, m.key())
                {
                    return Some(UnsupportedTypeError { typ, path });
                }
                // `Elem()` of a map is explicitly `canAddr: false`.
                self.check_type(
                    arena,
                    objects,
                    packages,
                    lookup,
                    m.elem(),
                    false,
                    format!("{path}[k]"),
                )
            }
            TypeData::Slice(s) => {
                if is_byte_elem(arena, s.elem()) {
                    return None;
                }
                // `Elem()` of a slice is `canAddr: true`.
                self.check_type(
                    arena,
                    objects,
                    packages,
                    lookup,
                    s.elem(),
                    true,
                    format!("{path}[0]"),
                )
            }
            // An array's element inherits; a pointer's is addressable.
            TypeData::Array(a) => self.check_type(
                arena,
                objects,
                packages,
                lookup,
                a.elem(),
                can_addr,
                format!("{path}[0]"),
            ),
            TypeData::Pointer(p) => {
                self.check_type(arena, objects, packages, lookup, p.elem(), true, path)
            }
            TypeData::Chan(_) | TypeData::Signature(_) => {
                Some(UnsupportedTypeError { typ, path })
            }
            _ => Some(UnsupportedTypeError { typ, path }),
        }
    }
}

/// A field `encoding/json` will encode — `typeFields`' `field`. The type is
/// not kept: it is re-derived from `index` by [`by_index`], as upstream's
/// `typeByIndex` does.
#[derive(Clone)]
struct JsonField {
    name: String,
    /// The name came from a json tag.
    tag: bool,
    index: Vec<usize>,
}

/// `fakereflect.TypeAndCanAddr.IsPtr` / `IsStruct` / `Elem` / `Name` over the
/// arena.
fn is_ptr(arena: &TypeArena, t: TypeId) -> bool {
    matches!(arena.get(unalias_readonly(arena, t).underlying(arena)), TypeData::Pointer(_))
}

fn is_struct(arena: &TypeArena, t: TypeId) -> bool {
    matches!(arena.get(unalias_readonly(arena, t).underlying(arena)), TypeData::Struct(_))
}

fn ptr_elem(arena: &TypeArena, t: TypeId) -> TypeId {
    match arena.get(unalias_readonly(arena, t).underlying(arena)) {
        TypeData::Pointer(p) => p.elem(),
        _ => t,
    }
}

fn has_name(arena: &TypeArena, t: TypeId) -> bool {
    matches!(arena.get(unalias_readonly(arena, t)), TypeData::Named(_))
}

/// `(field object, tag)` of field `i` of struct type `t`.
fn struct_field(arena: &TypeArena, t: TypeId, i: usize) -> Option<(guff_types::arena::ObjectId, &str)> {
    match arena.get(unalias_readonly(arena, t).underlying(arena)) {
        TypeData::Struct(s) if i < s.num_fields() => Some((s.field(i), s.tag(i))),
        _ => None,
    }
}

fn num_fields(arena: &TypeArena, t: TypeId) -> usize {
    match arena.get(unalias_readonly(arena, t).underlying(arena)) {
        TypeData::Struct(s) => s.num_fields(),
        _ => 0,
    }
}

/// `typeByIndex` and `pathByIndex` in one walk: the type (and addressability)
/// of the field at `index` below `t`, and the `.A.B.C` it is reached by.
/// A pointer on the way is dereferenced (and so addressable); a field
/// inherits its parent's addressability.
fn by_index(
    arena: &TypeArena,
    objects: &ObjectArena,
    mut t: TypeId,
    mut can_addr: bool,
    index: &[usize],
) -> Option<(TypeId, bool, String)> {
    let mut path = String::new();
    for &i in index {
        if is_ptr(arena, t) {
            t = ptr_elem(arena, t);
            can_addr = true;
        }
        let (field, _) = struct_field(arena, t, i)?;
        let ObjectData::Var(v) = objects.get(field) else {
            return None;
        };
        path.push('.');
        path.push_str(v.name());
        t = v.typ();
    }
    Some((t, can_addr, path))
}

/// `isValidTag`.
fn is_valid_tag(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            "!#$%&()*+-./:;<=>?@[]^_{|}~ ".contains(c) || c.is_alphabetic() || c.is_numeric()
        })
}

/// `typeFields`: the fields `encoding/json` encodes for struct type `t` — a
/// breadth-first walk over the struct and the structs it embeds, then Go's
/// rules for embedded fields modified by json tags: a shallower field hides
/// a deeper one of the same name, a tagged one beats an untagged one at the
/// same depth, and two at the same depth with the same standing hide each
/// other. Each survivor is then checked in index order, and the first
/// unsupported one is the finding.
fn type_fields(
    enc: &mut Encoder,
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    lookup: &dyn MarshalerLookup,
    t: TypeId,
    can_addr: bool,
    stack: &str,
) -> Option<UnsupportedTypeError> {
    // (type, canAddr, index) of the anonymous structs to explore at the
    // current level and the next.
    let mut next: Vec<(TypeId, bool, Vec<usize>)> = vec![(t, can_addr, Vec::new())];
    let mut next_count: HashMap<(TypeId, bool), usize> = HashMap::new();
    let mut visited: HashSet<(TypeId, bool)> = HashSet::new();
    let mut fields: Vec<JsonField> = Vec::new();

    while !next.is_empty() {
        let current = std::mem::take(&mut next);
        let count = std::mem::take(&mut next_count);
        for (ftyp, faddr, findex) in current {
            if !visited.insert((ftyp, faddr)) {
                continue;
            }
            for i in 0..num_fields(arena, ftyp) {
                let Some((field, tag)) = struct_field(arena, ftyp, i) else {
                    continue;
                };
                let ObjectData::Var(v) = objects.get(field) else {
                    continue;
                };
                let exported = is_exported(v.name());
                if v.embedded() {
                    let mut et = v.typ();
                    if is_ptr(arena, et) {
                        et = ptr_elem(arena, et);
                    }
                    // Embedded fields of unexported non-struct types are
                    // ignored; unexported struct types may have exported
                    // fields.
                    if !exported && !is_struct(arena, et) {
                        continue;
                    }
                } else if !exported {
                    continue;
                }
                let json = struct_tag_get(tag, "json");
                if json == "-" {
                    continue;
                }
                let mut name = json.split(',').next().unwrap_or("");
                if !is_valid_tag(name) {
                    name = "";
                }
                let mut index = findex.clone();
                index.push(i);

                let (mut ft, mut ft_addr) = (v.typ(), faddr);
                if !has_name(arena, ft) && is_ptr(arena, ft) {
                    ft = ptr_elem(arena, ft);
                    ft_addr = true;
                }

                if !name.is_empty() || !v.embedded() || !is_struct(arena, ft) {
                    let tagged = !name.is_empty();
                    let name = if tagged { name } else { v.name() };
                    fields.push(JsonField {
                        name: name.to_string(),
                        tag: tagged,
                        index,
                    });
                    // Reached more than once at this level: add a second
                    // copy so the dominance pass sees a duplicate.
                    if count.get(&(ftyp, faddr)).copied().unwrap_or(0) > 1 {
                        let dup = fields[fields.len() - 1].clone();
                        fields.push(dup);
                    }
                    continue;
                }

                let n = next_count.entry((ft, ft_addr)).or_insert(0);
                *n += 1;
                if *n == 1 {
                    next.push((ft, ft_addr, index));
                }
            }
        }
    }

    // By name, then depth, then "the name came from a tag", then index.
    fields.sort_by(|a, b| {
        a.name
            .cmp(&b.name)
            .then(a.index.len().cmp(&b.index.len()))
            .then(b.tag.cmp(&a.tag))
            .then(a.index.cmp(&b.index))
    });
    // One survivor per name: the first, unless the first two tie on depth and
    // on tag (an error in Go: every field of that name is dropped).
    let mut out: Vec<JsonField> = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let mut advance = 1;
        while i + advance < fields.len() && fields[i + advance].name == fields[i].name {
            advance += 1;
        }
        let group = &fields[i..i + advance];
        let hidden = group.len() > 1
            && group[0].index.len() == group[1].index.len()
            && group[0].tag == group[1].tag;
        if !hidden {
            out.push(group[0].clone());
        }
        i += advance;
    }
    out.sort_by(|a, b| a.index.cmp(&b.index));

    for f in out {
        let Some((ft, faddr, path)) = by_index(arena, objects, t, can_addr, &f.index) else {
            continue;
        };
        if let Some(err) =
            enc.check_type(arena, objects, packages, lookup, ft, faddr, format!("{stack}{path}"))
        {
            return Some(err);
        }
    }
    None
}

/// Minimal `reflect.StructTag.Get` — returns the value for `key:"value"`.
fn struct_tag_get<'a>(tag: &'a str, key: &str) -> &'a str {
    let mut rest = tag;
    while !rest.is_empty() {
        rest = rest.trim_start();
        let Some(colon) = rest.find(':') else {
            break;
        };
        let k = rest[..colon].trim();
        rest = &rest[colon + 1..];
        rest = rest.trim_start();
        if !rest.starts_with('"') {
            break;
        }
        let bytes = rest.as_bytes();
        let mut i = 1;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                i += 2;
                continue;
            }
            if bytes[i] == b'"' {
                let val = &rest[1..i];
                if k == key {
                    return val;
                }
                rest = rest[i + 1..].trim_start();
                break;
            }
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
    }
    ""
}

/// `newMapEncoder`: a basic key is always fine, and any other key has to
/// marshal itself.
///
/// ```go
/// switch t.Key().Type.Underlying().(type) {
/// case *types.Basic:
/// default:
///     if !t.Key().Implements(knowledge.Interfaces["encoding.TextMarshaler"]) {
///         return &UnsupportedTypeError{Type: t.Type, Path: stack}
///     }
/// }
/// ```
///
/// Note there is no `PtrTo` variant here: a key whose `MarshalText` has a
/// pointer receiver is still a finding. moby's `network.PortMap` is
/// `map[Port][]PortBinding`, and `Port` is a struct with a **value**-receiver
/// `MarshalText` — telegraf marshals one and guff reported it.
fn map_key_ok(arena: &TypeArena, lookup: &dyn MarshalerLookup, key: TypeId) -> bool {
    if matches!(
        arena.get(key.underlying(arena)),
        TypeData::Basic(b) if b.kind() != BasicKind::UntypedNil
    ) {
        return true;
    }
    lookup.implements(key, "MarshalText", false)
}

fn is_byte_elem(arena: &TypeArena, elem: TypeId) -> bool {
    matches!(
        arena.get(elem.underlying(arena)),
        TypeData::Basic(b) if b.kind() == BasicKind::Uint8
    )
}

/// Formats a marshal error like Go's SA1026.
pub fn format_marshal_error(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    err: &UnsupportedTypeError,
) -> String {
    let typ = render_type(arena, objects, packages, err.typ);
    if err.path == "x" {
        format!("trying to marshal unsupported type {typ}")
    } else {
        format!(
            "trying to marshal unsupported type {typ}, via {}",
            err.path
        )
    }
}
