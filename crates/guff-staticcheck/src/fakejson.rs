//! Static JSON marshalability check (no reflection).
//!
//! Minimal port of `honnef.co/go/tools/staticcheck/fakejson` for SA1026.

use std::collections::HashSet;

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
            TypeData::Struct(s) => {
                for i in 0..s.num_fields() {
                    let field = s.field(i);
                    let ObjectData::Var(v) = objects.get(field) else {
                        continue;
                    };
                    let name = v.name();
                    if !is_exported(name) {
                        if !v.embedded() {
                            continue;
                        }
                    }
                    let tag = s.tag(i);
                    if json_tag_skip(tag) {
                        continue;
                    }
                    let ftyp = v.typ();
                    let field_path = format!("{path}.{name}");
                    if v.embedded() {
                        if let TypeData::Struct(_) = arena.get(ftyp.underlying(arena)) {
                            if let Some(err) = self.check_type(
                                arena,
                                objects,
                                packages,
                                lookup,
                                ftyp,
                                can_addr,
                                field_path,
                            ) {
                                return Some(err);
                            }
                            continue;
                        }
                    }
                    if let Some(err) = self.check_type(
                        arena,
                        objects,
                        packages,
                        lookup,
                        ftyp,
                        can_addr,
                        field_path,
                    ) {
                        return Some(err);
                    }
                }
                None
            }
            TypeData::Map(m) => {
                if !map_key_ok(arena, lookup, m.key()) {
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

fn json_tag_skip(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }
    // `Struct.tag` stores the full Go struct tag (`json:"-" db:"x"`), not the
    // bare json name. Parse the `json` key like reflect.StructTag.Get.
    let json = struct_tag_get(tag, "json");
    let name = json.split(',').next().unwrap_or("");
    name == "-"
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
