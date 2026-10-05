//! Static XML marshalability check (no reflection).
//!
//! Port of `honnef.co/go/tools/staticcheck/fakexml` for SA1026 — a copy of
//! `encoding/xml`'s encoder with reflection replaced by go/types. It is not
//! `fakejson` with a different tag key: xml supports no maps at all, reports
//! the map itself rather than its element, and has a cyclic-pointer error of
//! its own.

use std::collections::HashSet;

use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, TypeArena, TypeData};
use guff_types::basic::BasicKind;
use guff_types::object::is_exported;
use guff_types::typestring::type_string;
use guff_types::TypeId;

use crate::fakejson::{struct_tag_get, MarshalerLookup};

/// The errors SA1026 reports. Every other error `fakexml` returns (bad tags,
/// conflicting tag paths) is [`XmlError::Other`]: SA5008 and vet report those
/// at the struct tag instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum XmlError {
    Unsupported { typ: TypeId, path: String },
    Cyclic { typ: TypeId, path: String },
    Other,
}

/// `fakereflect.TypeAndCanAddr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct Val {
    typ: TypeId,
    can_addr: bool,
}

// `fieldFlags`.
const F_ELEMENT: u32 = 1 << 0;
const F_ATTR: u32 = 1 << 1;
const F_CDATA: u32 = 1 << 2;
const F_CHAR_DATA: u32 = 1 << 3;
const F_INNER_XML: u32 = 1 << 4;
const F_COMMENT: u32 = 1 << 5;
const F_ANY: u32 = 1 << 6;
const F_OMIT_EMPTY: u32 = 1 << 7;
const F_MODE: u32 = F_ELEMENT | F_ATTR | F_CDATA | F_CHAR_DATA | F_INNER_XML | F_COMMENT | F_ANY;

const XML_NAME: &str = "XMLName";

/// `fieldInfo`, less what only the real encoder needs.
#[derive(Debug, Clone)]
struct FieldInfo {
    idx: Vec<usize>,
    name: String,
    xmlns: String,
    flags: u32,
    parents: Vec<String>,
}

/// `typeInfo`.
#[derive(Debug, Clone, Default)]
struct TypeInfo {
    xmlname: Option<FieldInfo>,
    fields: Vec<FieldInfo>,
}

/// `fakereflect.StructField`.
struct StructField<'a> {
    index: Vec<usize>,
    name: &'a str,
    anonymous: bool,
    tag: &'a str,
    typ: Val,
}

/// Returns an error if `typ` cannot be XML-marshaled (Go `fakexml.Marshal`).
pub fn marshal(
    arena: &TypeArena,
    objects: &ObjectArena,
    packages: &PackageArena,
    lookup: &dyn MarshalerLookup,
    typ: TypeId,
) -> Option<XmlError> {
    let mut enc = Encoder {
        arena,
        objects,
        packages,
        lookup,
        seen_addr: HashSet::new(),
        seen_no_addr: HashSet::new(),
    };
    // `TypeAndCanAddr{Type: v}` — not addressable, as in `fakejson`.
    enc.marshal_value(
        Val {
            typ,
            can_addr: false,
        },
        "x",
    )
    .err()
}

struct Encoder<'a> {
    arena: &'a TypeArena,
    objects: &'a ObjectArena,
    packages: &'a PackageArena,
    lookup: &'a dyn MarshalerLookup,
    seen_addr: HashSet<TypeId>,
    seen_no_addr: HashSet<TypeId>,
}

impl Encoder<'_> {
    fn under(&self, t: TypeId) -> &TypeData {
        self.arena
            .get(unalias_readonly(self.arena, t).underlying(self.arena))
    }

    fn is_ptr(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Pointer(_))
    }

    fn is_interface(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Interface(_))
    }

    fn is_struct(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Struct(_))
    }

    fn is_slice(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Slice(_))
    }

    fn is_array(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Array(_))
    }

    /// `TypeAndCanAddr.Elem`: a pointer's or a slice's element is
    /// addressable, an array's inherits, a map's is not.
    fn elem(&self, v: Val) -> Val {
        match self.under(v.typ) {
            TypeData::Pointer(p) => Val {
                typ: p.elem(),
                can_addr: true,
            },
            TypeData::Slice(s) => Val {
                typ: s.elem(),
                can_addr: true,
            },
            TypeData::Array(a) => Val {
                typ: a.elem(),
                can_addr: v.can_addr,
            },
            TypeData::Map(m) => Val {
                typ: m.elem(),
                can_addr: false,
            },
            _ => v,
        }
    }

    fn num_field(&self, v: Val) -> usize {
        match self.under(v.typ) {
            TypeData::Struct(s) => s.num_fields(),
            _ => 0,
        }
    }

    /// `TypeAndCanAddr.Field`: the field inherits its struct's
    /// addressability.
    fn field(&self, v: Val, i: usize) -> Option<StructField<'_>> {
        let TypeData::Struct(s) = self.under(v.typ) else {
            return None;
        };
        if i >= s.num_fields() {
            return None;
        }
        let ObjectData::Var(f) = self.objects.get(s.field(i)) else {
            return None;
        };
        Some(StructField {
            index: vec![i],
            name: f.name(),
            anonymous: f.embedded(),
            tag: s.tag(i),
            typ: Val {
                typ: f.typ(),
                can_addr: v.can_addr,
            },
        })
    }

    fn type_name_is(&self, t: TypeId, want: &str) -> bool {
        type_string(self.arena, self.objects, self.packages, t, None) == want
    }

    /// `[]byte` / `[N]byte` element test (`isByteSlice`, `isByteArray` and
    /// `marshalSimple`'s slice case).
    fn elem_is_uint8(&self, elem: TypeId) -> bool {
        matches!(self.under(elem), TypeData::Basic(b) if b.kind() == BasicKind::Uint8)
    }

    fn is_byte_slice(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Slice(s) if self.elem_is_uint8(s.elem()))
    }

    fn is_byte_array(&self, v: Val) -> bool {
        matches!(self.under(v.typ), TypeData::Array(a) if self.elem_is_uint8(a.elem()))
    }

    /// `implementsMarshaler`: `MarshalXML(*xml.Encoder, xml.StartElement) error`
    /// in the method set of `v` (or of `*v` when `ptr`).
    fn implements_marshaler(&self, v: Val, ptr: bool) -> bool {
        self.method_shape(v, ptr, "MarshalXML", &["*encoding/xml.Encoder", "encoding/xml.StartElement"], &["error"])
    }

    /// `implementsMarshalerAttr`:
    /// `MarshalXMLAttr(xml.Name) (xml.Attr, error)`.
    fn implements_marshaler_attr(&self, v: Val, ptr: bool) -> bool {
        self.method_shape(v, ptr, "MarshalXMLAttr", &["encoding/xml.Name"], &["encoding/xml.Attr", "error"])
    }

    fn method_shape(&self, v: Val, ptr: bool, method: &str, params: &[&str], results: &[&str]) -> bool {
        let Some(sig) = self.lookup.method_signature(v.typ, method, ptr) else {
            return false;
        };
        let TypeData::Signature(sig) = self.arena.get(sig.underlying(self.arena)) else {
            return false;
        };
        let matches = |tuple: Option<TypeId>, want: &[&str]| {
            if guff_types::tuple::tuple_len(self.arena, tuple) != want.len() {
                return false;
            }
            let Some(tuple) = tuple else {
                return want.is_empty();
            };
            want.iter().enumerate().all(|(i, w)| {
                guff_types::tuple::tuple_at(self.arena, tuple, i)
                    .typ(self.objects)
                    .is_some_and(|t| self.type_name_is(t, w))
            })
        };
        matches(sig.params(), params) && matches(sig.results(), results)
    }

    /// `v.Implements(knowledge.Interfaces["encoding.TextMarshaler"])`, or
    /// `PtrTo(v).Implements(…)` when `ptr`.
    fn text_marshaler(&self, v: Val, ptr: bool) -> bool {
        self.lookup.implements(v.typ, "MarshalText", ptr)
    }

    fn marshal_value(&mut self, mut val: Val, stack: &str) -> Result<(), XmlError> {
        let seen = if val.can_addr {
            &mut self.seen_addr
        } else {
            &mut self.seen_no_addr
        };
        if !seen.insert(val.typ) {
            return Ok(());
        }

        // Dereference pointers; a pointer that leads back to itself (`type P
        // *P`) is the cyclic error, at the path of the field that holds it.
        let mut seen_vals: HashSet<Val> = HashSet::new();
        while self.is_interface(val) || self.is_ptr(val) {
            if self.is_interface(val) {
                return Ok(());
            }
            val = self.elem(val);
            if !seen_vals.insert(val) {
                return Err(XmlError::Cyclic {
                    typ: val.typ,
                    path: stack.to_string(),
                });
            }
        }

        if self.implements_marshaler(val, false) {
            return Ok(());
        }
        if val.can_addr && self.implements_marshaler(val, true) {
            return Ok(());
        }
        if self.text_marshaler(val, false) {
            return Ok(());
        }
        if val.can_addr && self.text_marshaler(val, true) {
            return Ok(());
        }

        if (self.is_slice(val) || self.is_array(val))
            && !self.is_byte_array(val)
            && !self.is_byte_slice(val)
        {
            return self.marshal_value(self.elem(val), &format!("{stack}[0]"));
        }

        let tinfo = self.get_type_info(val, 0)?;

        for finfo in tinfo.fields.iter().filter(|f| f.flags & F_ATTR != 0) {
            let fv = self.field_value(finfo, val);
            let path = format!("{stack}{}", self.path_by_index(val, &finfo.idx));
            self.marshal_attr(fv, &path)?;
        }

        if self.is_struct(val) {
            self.marshal_struct(&tinfo, val, stack)
        } else {
            self.marshal_simple(val, stack)
        }
    }

    fn marshal_attr(&mut self, mut val: Val, stack: &str) -> Result<(), XmlError> {
        if self.implements_marshaler_attr(val, false) {
            return Ok(());
        }
        if val.can_addr && self.implements_marshaler_attr(val, true) {
            return Ok(());
        }
        if self.text_marshaler(val, false) {
            return Ok(());
        }
        if val.can_addr && self.text_marshaler(val, true) {
            return Ok(());
        }
        if self.is_ptr(val) {
            val = self.elem(val);
        }
        if self.is_slice(val) && !self.is_byte_slice(val) {
            return self.marshal_attr(self.elem(val), &format!("{stack}[0]"));
        }
        if self.type_name_is(val.typ, "encoding/xml.Attr") {
            return Ok(());
        }
        self.marshal_simple(val, stack)
    }

    fn marshal_simple(&self, val: Val, stack: &str) -> Result<(), XmlError> {
        let unsupported = || XmlError::Unsupported {
            typ: val.typ,
            path: stack.to_string(),
        };
        match self.under(val.typ) {
            TypeData::Basic(_) | TypeData::Interface(_) => Ok(()),
            TypeData::Slice(_) | TypeData::Array(_) => {
                if self.elem_is_uint8(self.elem(val).typ) {
                    Ok(())
                } else {
                    Err(unsupported())
                }
            }
            _ => Err(unsupported()),
        }
    }

    fn indirect(&self, mut v: Val) -> Val {
        while self.is_ptr(v) {
            v = self.elem(v);
        }
        v
    }

    /// `pathByIndex`.
    fn path_by_index(&self, mut t: Val, index: &[usize]) -> String {
        let mut path = String::new();
        for &i in index {
            if self.is_ptr(t) {
                t = self.elem(t);
            }
            let Some(f) = self.field(t, i) else {
                break;
            };
            path.push('.');
            path.push_str(f.name);
            t = f.typ;
        }
        path
    }

    /// `fieldInfo.value`: the field's type, stepping through embedded
    /// pointers to structs.
    fn field_value(&self, finfo: &FieldInfo, mut v: Val) -> Val {
        for (i, &x) in finfo.idx.iter().enumerate() {
            if i > 0 && self.is_ptr(v) && self.is_struct(self.elem(v)) {
                v = self.elem(v);
            }
            let Some(f) = self.field(v, x) else {
                return v;
            };
            v = f.typ;
        }
        v
    }

    fn marshal_struct(&mut self, tinfo: &TypeInfo, val: Val, stack: &str) -> Result<(), XmlError> {
        for finfo in &tinfo.fields {
            if finfo.flags & F_ATTR != 0 {
                continue;
            }
            let mut vf = self.field_value(finfo, val);
            match finfo.flags & F_MODE {
                // Every branch of upstream's case ends in `continue`.
                F_CDATA | F_CHAR_DATA => continue,
                F_COMMENT => {
                    vf = self.indirect(vf);
                    if !(self.is_byte_slice(vf) || self.is_byte_array(vf)) {
                        return Err(XmlError::Other);
                    }
                    continue;
                }
                F_INNER_XML => {
                    vf = self.indirect(vf);
                    // `vf.Type.(*types.Slice)` — the type itself, not its
                    // underlying type: a named `[]byte` is walked.
                    let t = self.arena.get(unalias_readonly(self.arena, vf.typ));
                    let byte_slice = matches!(t, TypeData::Slice(s)
                        if matches!(self.arena.get(unalias_readonly(self.arena, s.elem())),
                            TypeData::Basic(b) if b.kind() == BasicKind::Uint8));
                    let string = matches!(t, TypeData::Basic(b) if b.kind() == BasicKind::String);
                    if byte_slice || string {
                        continue;
                    }
                }
                _ => {}
            }
            let path = format!("{stack}{}", self.path_by_index(val, &finfo.idx));
            self.marshal_value(vf, &path)?;
        }
        Ok(())
    }

    /// `getTypeInfo`. Upstream recurses into embedded structs without a
    /// guard (`type T struct{ *T }` would not terminate); `depth` stops that.
    fn get_type_info(&self, typ: Val, depth: usize) -> Result<TypeInfo, XmlError> {
        let mut tinfo = TypeInfo::default();
        if depth > 64 || !self.is_struct(typ) || self.type_name_is(typ.typ, "encoding/xml.Name") {
            return Ok(tinfo);
        }
        for i in 0..self.num_field(typ) {
            let Some(f) = self.field(typ, i) else {
                continue;
            };
            if (!is_exported(f.name) && !f.anonymous) || struct_tag_get(f.tag, "xml") == "-" {
                continue;
            }
            if f.anonymous {
                let mut t = f.typ;
                if self.is_ptr(t) {
                    t = self.elem(t);
                }
                if self.is_struct(t) {
                    let inner = self.get_type_info(t, depth + 1)?;
                    if tinfo.xmlname.is_none() {
                        tinfo.xmlname = inner.xmlname;
                    }
                    for mut finfo in inner.fields {
                        finfo.idx.insert(0, i);
                        self.add_field_info(&mut tinfo, finfo)?;
                    }
                    continue;
                }
            }
            let finfo = self.struct_field_info(&f)?;
            if f.name == XML_NAME {
                tinfo.xmlname = Some(finfo);
                continue;
            }
            self.add_field_info(&mut tinfo, finfo)?;
        }
        Ok(tinfo)
    }

    /// `StructFieldInfo`.
    fn struct_field_info(&self, f: &StructField<'_>) -> Result<FieldInfo, XmlError> {
        let mut finfo = FieldInfo {
            idx: f.index.clone(),
            name: String::new(),
            xmlns: String::new(),
            flags: 0,
            parents: Vec::new(),
        };
        let full = struct_tag_get(f.tag, "xml");
        let mut tag = full;
        if let Some(i) = tag.find(' ') {
            finfo.xmlns = tag[..i].to_string();
            tag = &tag[i + 1..];
        }

        let tokens: Vec<&str> = tag.split(',').collect();
        if tokens.len() == 1 {
            finfo.flags = F_ELEMENT;
        } else {
            tag = tokens[0];
            for flag in &tokens[1..] {
                finfo.flags |= match *flag {
                    "attr" => F_ATTR,
                    "cdata" => F_CDATA,
                    "chardata" => F_CHAR_DATA,
                    "innerxml" => F_INNER_XML,
                    "comment" => F_COMMENT,
                    "any" => F_ANY,
                    "omitempty" => F_OMIT_EMPTY,
                    _ => 0,
                };
            }
            match finfo.flags & F_MODE {
                0 => finfo.flags |= F_ELEMENT,
                mode @ (F_ATTR | F_CDATA | F_CHAR_DATA | F_INNER_XML | F_COMMENT | F_ANY) => {
                    if f.name == XML_NAME || (!tag.is_empty() && mode != F_ATTR) {
                        return Err(XmlError::Other);
                    }
                }
                mode if mode == F_ANY | F_ATTR => {
                    if f.name == XML_NAME || !tag.is_empty() {
                        return Err(XmlError::Other);
                    }
                }
                _ => return Err(XmlError::Other),
            }
            if finfo.flags & F_MODE == F_ANY {
                finfo.flags |= F_ELEMENT;
            }
            if finfo.flags & F_OMIT_EMPTY != 0 && finfo.flags & (F_ELEMENT | F_ATTR) == 0 {
                return Err(XmlError::Other);
            }
        }

        if !finfo.xmlns.is_empty() && tag.is_empty() {
            return Err(XmlError::Other);
        }

        if f.name == XML_NAME {
            finfo.name = tag.to_string();
            return Ok(finfo);
        }

        if tag.is_empty() {
            if let Some(xmlname) = self.lookup_xml_name(f.typ) {
                finfo.xmlns = xmlname.xmlns;
                finfo.name = xmlname.name;
            } else {
                finfo.name = f.name.to_string();
            }
            return Ok(finfo);
        }

        let mut parents: Vec<String> = tag.split('>').map(str::to_string).collect();
        if parents[0].is_empty() {
            parents[0] = f.name.to_string();
        }
        if parents[parents.len() - 1].is_empty() {
            return Err(XmlError::Other);
        }
        finfo.name = parents.pop().unwrap_or_default();
        if !parents.is_empty() {
            if finfo.flags & F_ELEMENT == 0 {
                return Err(XmlError::Other);
            }
            finfo.parents = parents;
        }

        if finfo.flags & F_ELEMENT != 0 {
            if let Some(xmlname) = self.lookup_xml_name(f.typ) {
                if xmlname.name != finfo.name {
                    return Err(XmlError::Other);
                }
            }
        }
        Ok(finfo)
    }

    /// `lookupXMLName`.
    fn lookup_xml_name(&self, mut typ: Val) -> Option<FieldInfo> {
        let mut seen: HashSet<Val> = HashSet::new();
        while self.is_ptr(typ) {
            typ = self.elem(typ);
            if !seen.insert(typ) {
                return None;
            }
        }
        if !self.is_struct(typ) {
            return None;
        }
        for i in 0..self.num_field(typ) {
            let f = self.field(typ, i)?;
            if f.name != XML_NAME {
                continue;
            }
            return match self.struct_field_info(&f) {
                Ok(finfo) if !finfo.name.is_empty() => Some(finfo),
                _ => None,
            };
        }
        None
    }

    /// `addFieldInfo`: Go's rules for a field hiding another of the same xml
    /// path. Two at the same depth is `TagPathError`, which SA1026 leaves to
    /// vet.
    fn add_field_info(&self, tinfo: &mut TypeInfo, newf: FieldInfo) -> Result<(), XmlError> {
        let mut conflicts: Vec<usize> = Vec::new();
        'outer: for (i, oldf) in tinfo.fields.iter().enumerate() {
            if oldf.flags & F_MODE != newf.flags & F_MODE {
                continue;
            }
            if !oldf.xmlns.is_empty() && !newf.xmlns.is_empty() && oldf.xmlns != newf.xmlns {
                continue;
            }
            let minl = newf.parents.len().min(oldf.parents.len());
            for p in 0..minl {
                if oldf.parents[p] != newf.parents[p] {
                    continue 'outer;
                }
            }
            if oldf.parents.len() > newf.parents.len() {
                if oldf.parents[newf.parents.len()] == newf.name {
                    conflicts.push(i);
                }
            } else if oldf.parents.len() < newf.parents.len() {
                if newf.parents[oldf.parents.len()] == oldf.name {
                    conflicts.push(i);
                }
            } else if newf.name == oldf.name {
                conflicts.push(i);
            }
        }
        if conflicts.is_empty() {
            tinfo.fields.push(newf);
            return Ok(());
        }
        if conflicts
            .iter()
            .any(|&i| tinfo.fields[i].idx.len() < newf.idx.len())
        {
            return Ok(());
        }
        if conflicts
            .iter()
            .any(|&i| tinfo.fields[i].idx.len() == newf.idx.len())
        {
            return Err(XmlError::Other);
        }
        for &i in conflicts.iter().rev() {
            tinfo.fields.remove(i);
        }
        tinfo.fields.push(newf);
        Ok(())
    }
}
