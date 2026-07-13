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
    typ: TypeId,
) -> Option<UnsupportedTypeError> {
    let mut enc = Encoder::default();
    enc.check_type(arena, objects, packages, typ, true, "x".to_string())
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

        let u = unalias_readonly(arena, typ).underlying(arena);
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
                                ftyp,
                                can_addr,
                                field_path,
                            ) {
                                return Some(err);
                            }
                            continue;
                        }
                    }
                    if let Some(err) =
                        self.check_type(arena, objects, packages, ftyp, can_addr, field_path)
                    {
                        return Some(err);
                    }
                }
                None
            }
            TypeData::Map(m) => {
                if !map_key_ok(arena, m.key()) {
                    return Some(UnsupportedTypeError { typ, path });
                }
                self.check_type(
                    arena,
                    objects,
                    packages,
                    m.elem(),
                    can_addr,
                    format!("{path}[k]"),
                )
            }
            TypeData::Slice(s) => {
                if is_byte_elem(arena, s.elem()) {
                    return None;
                }
                self.check_type(
                    arena,
                    objects,
                    packages,
                    s.elem(),
                    can_addr,
                    format!("{path}[0]"),
                )
            }
            TypeData::Array(a) => self.check_type(
                arena,
                objects,
                packages,
                a.elem(),
                can_addr,
                format!("{path}[0]"),
            ),
            TypeData::Pointer(p) => {
                self.check_type(arena, objects, packages, p.elem(), can_addr, path)
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
    let name = tag.split(',').next().unwrap_or("");
    name == "-"
}

fn map_key_ok(arena: &TypeArena, key: TypeId) -> bool {
    matches!(
        arena.get(key.underlying(arena)),
        TypeData::Basic(b) if b.kind() != BasicKind::UntypedNil
    )
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
