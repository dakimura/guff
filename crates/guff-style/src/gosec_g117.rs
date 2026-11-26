//! Gosec **G117** — an exported struct field whose name looks like a secret,
//! reaching a JSON / YAML / XML / TOML serializer.
//!
//! Port of securego/gosec v2.27.1 `rules/secret_serialization.go` (the version
//! golangci-lint 2.12.2 pins).
//!
//! The rule is a call rule, not a field rule: it fires on the *marshal call*,
//! and everything else is about deciding that the value being marshalled will
//! actually serialize the field. Four gates, each of which was measured against
//! golangci-lint before it was written down:
//!
//! - the call is inside a custom marshaler (`MarshalJSON` and friends) — the
//!   author is controlling serialization by hand;
//! - the marshalled type has that format's marshaler method, promoted methods
//!   included, so the library never touches the fields;
//! - the field is unexported, `_`, tagged `-`, or not a string-ish type;
//! - the composite literal passes a *call result* for the field, which reads as
//!   masking.
//!
//! Only the type's own fields are examined: a nested or embedded struct field
//! is not a "secret candidate type", so the walk stops there. Containers are
//! the exception — a `[]T`, `[N]T`, `map[K]T` or `*T` is unwrapped to `T`.

use std::collections::HashSet;

use regex::Regex;

use guff::ast::{CallExpr, Decl, Expr, File};
use guff::walk::{preorder, NodeRef};
use guff_analysis::Pass;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::TypeId;

use crate::options::GosecOptions;

/// One serialization format: its struct-tag key, the marshaler interface a type
/// can implement to take over, and the calls that serialize.
struct FormatSpec {
    name: &'static str,
    tag_key: &'static str,
    /// Empty when the format has no standard marshaler interface (TOML).
    marshaler_method: &'static str,
    /// `(package path, function names)`.
    function_sinks: &'static [(&'static str, &'static [&'static str])],
    /// `(package path, type name, method)`.
    method_sinks: &'static [(&'static str, &'static str, &'static str)],
}

const FORMATS: &[FormatSpec] = &[
    FormatSpec {
        name: "JSON",
        tag_key: "json",
        marshaler_method: "MarshalJSON",
        function_sinks: &[("encoding/json", &["Marshal", "MarshalIndent"])],
        method_sinks: &[("encoding/json", "Encoder", "Encode")],
    },
    FormatSpec {
        name: "YAML",
        tag_key: "yaml",
        marshaler_method: "MarshalYAML",
        function_sinks: &[
            ("go.yaml.in/yaml/v3", &["Marshal"]),
            ("gopkg.in/yaml.v3", &["Marshal"]),
            ("gopkg.in/yaml.v2", &["Marshal"]),
            ("sigs.k8s.io/yaml", &["Marshal"]),
        ],
        method_sinks: &[
            ("go.yaml.in/yaml/v3", "Encoder", "Encode"),
            ("gopkg.in/yaml.v3", "Encoder", "Encode"),
            ("gopkg.in/yaml.v2", "Encoder", "Encode"),
        ],
    },
    FormatSpec {
        name: "XML",
        tag_key: "xml",
        marshaler_method: "MarshalXML",
        function_sinks: &[("encoding/xml", &["Marshal", "MarshalIndent"])],
        method_sinks: &[("encoding/xml", "Encoder", "Encode")],
    },
    FormatSpec {
        name: "TOML",
        tag_key: "toml",
        marshaler_method: "",
        function_sinks: &[
            ("github.com/pelletier/go-toml", &["Marshal"]),
            ("github.com/pelletier/go-toml/v2", &["Marshal"]),
        ],
        method_sinks: &[
            ("github.com/pelletier/go-toml", "Encoder", "Encode"),
            ("github.com/pelletier/go-toml/v2", "Encoder", "Encode"),
            ("github.com/BurntSushi/toml", "Encoder", "Encode"),
        ],
    },
];

/// Method names that mean "the author is serializing this by hand".
const CUSTOM_MARSHALER_METHODS: &[&str] = &[
    "MarshalJSON",
    "MarshalYAML",
    "MarshalXML",
    "MarshalText",
    "MarshalTOML",
    "MarshalBSON",
];

pub(crate) const G117_WHAT: &str =
    "Exported struct field appears to be a secret and is serialized by JSON/YAML/XML/TOML";

/// Upstream's default `pattern`.
pub(crate) const G117_DEFAULT_PATTERN: &str = r"(?i)\b((?:api|access|auth|bearer|client|oauth|private|refresh|session|jwt)[_-]?(?:key|secret|token)s?|password|passwd|pwd|pass|secret|cred|jwt)\b";

struct Match {
    field_name: String,
    serialized_key: String,
}

pub(crate) fn check_g117(
    pass: &Pass<'_>,
    enabled: &HashSet<&'static str>,
    opts: &GosecOptions,
    pending: &mut Vec<(u32, u32, String)>,
) {
    if !enabled.contains("G117") {
        return;
    }
    let Ok(pattern) = Regex::new(&opts.g117.pattern) else {
        // Upstream compiles with MustCompile and would panic; a bad pattern
        // from a config file must not take the whole analyzer down.
        return;
    };
    for file in pass.files() {
        let marshaler_bodies = custom_marshaler_bodies(file);
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CallExpr(call) = n {
                check_call(pass, file, call, &pattern, &marshaler_bodies, pending);
            }
            true
        });
    }
}

fn check_call(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pattern: &Regex,
    marshaler_bodies: &[(u32, u32)],
    pending: &mut Vec<(u32, u32, String)>,
) {
    let Some((arg, format)) = find_serialized_argument(pass, file, call) else {
        return;
    };
    let pos = call.pos().0 as u32;
    if marshaler_bodies.iter().any(|&(lo, hi)| pos >= lo && pos < hi) {
        return;
    }
    let Some(typ) = expr_type(pass, arg) else {
        return;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    if type_implements_marshaler(&artifacts.types, &artifacts.objects, typ, format.marshaler_method)
    {
        return;
    }
    let mut visited = HashSet::new();
    let Some(m) = find_sensitive_field(
        &artifacts.types,
        &artifacts.objects,
        typ,
        format.tag_key,
        pattern,
        &mut visited,
    ) else {
        return;
    };
    if composite_lit_field_is_transformed(arg, &m.field_name) {
        return;
    }
    pending.push((
        pos,
        call.end().0 as u32,
        format!(
            "G117: Marshaled struct field {:?} ({} key {:?}) matches secret pattern",
            m.field_name, format.name, m.serialized_key
        ),
    ));
}

/// Byte ranges of the bodies of methods named like a custom marshaler.
fn custom_marshaler_bodies(file: &File) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    for decl in &file.decls {
        let Decl::FuncDecl(fd) = decl else {
            continue;
        };
        let Some(recv) = fd.recv.as_ref() else {
            continue;
        };
        if recv.list.is_empty() {
            continue;
        }
        if !CUSTOM_MARSHALER_METHODS.contains(&fd.name.name.as_str()) {
            continue;
        }
        if let Some(body) = fd.body.as_ref() {
            out.push((body.pos().0 as u32, body.end().0 as u32));
        }
    }
    out
}

fn find_serialized_argument<'a>(
    pass: &Pass<'_>,
    file: &File,
    call: &'a CallExpr,
) -> Option<(&'a Expr, &'static FormatSpec)> {
    for format in FORMATS {
        for (pkg, names) in format.function_sinks {
            if call_matches_package_function(pass, file, call, pkg, names) {
                return call.args.first().map(|a| (a, format));
            }
        }
        for &(pkg, type_name, method) in format.method_sinks {
            if call_matches_method_sink(pass, file, call, pkg, type_name, method) {
                return call.args.first().map(|a| (a, format));
            }
        }
    }
    None
}

fn call_matches_package_function(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pkg_path: &str,
    names: &[&str],
) -> bool {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    if !names.contains(&sel.sel.name.as_str()) {
        return false;
    }
    if let Some(path) = selector_object_package(pass, &sel.sel) {
        if package_path_matches(&path, pkg_path) {
            return true;
        }
    }
    let Expr::Ident(pkg_ident) = sel.x.as_ref() else {
        return false;
    };
    import_alias_matches_path(file, &pkg_ident.name, pkg_path)
}

fn call_matches_method_sink(
    pass: &Pass<'_>,
    file: &File,
    call: &CallExpr,
    pkg_path: &str,
    type_name: &str,
    method: &str,
) -> bool {
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return false;
    };
    if sel.sel.name != method {
        return false;
    }
    if let Some(recv) = expr_type(pass, &sel.x) {
        if let Some(artifacts) = pass.pkg().type_artifacts.as_ref() {
            if is_named_type_in_package(
                &artifacts.types,
                &artifacts.objects,
                &artifacts.packages,
                recv,
                pkg_path,
                type_name,
            ) {
                return true;
            }
        }
    }
    // `json.NewEncoder(w).Encode(x)` — the receiver is the constructor call.
    let Expr::CallExpr(ctor) = sel.x.as_ref() else {
        return false;
    };
    let ctor_name = format!("New{type_name}");
    if call_matches_package_function(pass, file, ctor, pkg_path, &[ctor_name.as_str()]) {
        return true;
    }
    if !pkg_path.to_lowercase().contains("toml") {
        return false;
    }
    let Expr::SelectorExpr(ctor_sel) = &*ctor.fun else {
        return false;
    };
    if ctor_sel.sel.name != ctor_name {
        return false;
    }
    let Expr::Ident(pkg_ident) = ctor_sel.x.as_ref() else {
        return false;
    };
    import_alias_path_contains(file, &pkg_ident.name, "toml")
}

/// `packagePathMatches`: exact, except that any path containing `toml` answers
/// a `toml` expectation — upstream's way of covering the several TOML modules.
fn package_path_matches(actual: &str, expected: &str) -> bool {
    if actual == expected {
        return true;
    }
    if expected.contains("toml") {
        return actual.to_lowercase().contains("toml");
    }
    false
}

fn package_name_from_path(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) if i + 1 < path.len() => &path[i + 1..],
        _ => path,
    }
}

fn import_alias_matches_path(file: &File, alias: &str, pkg_path: &str) -> bool {
    for (path, name) in imports(file) {
        if !package_path_matches(&path, pkg_path) {
            continue;
        }
        let import_alias = name.unwrap_or_else(|| package_name_from_path(&path).to_string());
        if import_alias == alias {
            return true;
        }
    }
    false
}

fn import_alias_path_contains(file: &File, alias: &str, fragment: &str) -> bool {
    for (path, name) in imports(file) {
        let import_alias = name
            .clone()
            .unwrap_or_else(|| package_name_from_path(&path).to_string());
        if import_alias == alias && path.to_lowercase().contains(&fragment.to_lowercase()) {
            return true;
        }
    }
    false
}

/// `(import path, explicit alias)` of every import in the file.
fn imports(file: &File) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    for spec in &file.imports {
        let Some(path) = crate::gosec::unquote_string_lit(&spec.path.value) else {
            continue;
        };
        out.push((path, spec.name.as_ref().map(|n| n.name.clone())));
    }
    out
}

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn selector_object_package(pass: &Pass<'_>, sel: &guff::ast::Ident) -> Option<String> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let &obj = info.uses.get(&sel.id)?;
    let pkg = obj.pkg(&artifacts.objects)?;
    Some(artifacts.packages.get(pkg).path().to_string())
}

fn is_named_type_in_package(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    packages: &guff_types::arena::PackageArena,
    typ: TypeId,
    pkg_path: &str,
    type_name: &str,
) -> bool {
    match types.get(typ) {
        TypeData::Pointer(p) => {
            is_named_type_in_package(types, objects, packages, p.elem(), pkg_path, type_name)
        }
        TypeData::Named(n) => {
            let obj = n.obj();
            obj.name(objects) == type_name
                && obj
                    .pkg(objects)
                    .is_some_and(|p| packages.get(p).path() == pkg_path)
        }
        _ => false,
    }
}

/// The innermost `Named` behind pointers, slices, arrays and maps.
fn element_named(types: &guff_types::arena::TypeArena, typ: TypeId) -> Option<TypeId> {
    match types.get(typ) {
        TypeData::Named(_) => Some(typ),
        TypeData::Pointer(p) => element_named(types, p.elem()),
        TypeData::Slice(s) => element_named(types, s.elem()),
        TypeData::Array(a) => element_named(types, a.elem()),
        TypeData::Map(m) => element_named(types, m.elem()),
        _ => None,
    }
}

/// Does the pointer method set of the innermost named type carry `method`?
///
/// Upstream asks `types.NewMethodSet(types.NewPointer(named))`, which includes
/// methods promoted through embedded fields — measured: a type that embeds one
/// with `MarshalJSON` is silent upstream. This walks the embedding graph by
/// breadth instead of building a method set, so it does not model the shadowing
/// and ambiguity rules; for "is there a method spelled like this" the answer is
/// the same, and a name that is ambiguous at one depth is still a marshaler
/// somewhere in the type.
fn type_implements_marshaler(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    typ: TypeId,
    method: &str,
) -> bool {
    if method.is_empty() {
        return false;
    }
    let Some(named) = element_named(types, typ) else {
        return false;
    };
    let mut seen = HashSet::new();
    has_method(types, objects, named, method, &mut seen, 0)
}

fn has_method(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    typ: TypeId,
    method: &str,
    seen: &mut HashSet<TypeId>,
    depth: u32,
) -> bool {
    if depth > 16 || !seen.insert(typ) {
        return false;
    }
    let underlying = match types.get(typ) {
        TypeData::Named(n) => {
            for i in 0..n.num_methods() {
                if n.method(i).name(objects) == method {
                    return true;
                }
            }
            match n.underlying() {
                Some(u) => u,
                None => return false,
            }
        }
        TypeData::Pointer(p) => return has_method(types, objects, p.elem(), method, seen, depth + 1),
        _ => typ,
    };
    let TypeData::Struct(st) = types.get(underlying) else {
        return false;
    };
    for i in 0..st.num_fields() {
        let field = st.field(i);
        let ObjectData::Var(v) = objects.get(field) else {
            continue;
        };
        if !v.embedded() {
            continue;
        }
        let Some(ft) = field.typ(objects) else {
            continue;
        };
        if has_method(types, objects, ft, method, seen, depth + 1) {
            return true;
        }
    }
    false
}

fn find_sensitive_field(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    typ: TypeId,
    tag_key: &str,
    pattern: &Regex,
    visited: &mut HashSet<TypeId>,
) -> Option<Match> {
    if !visited.insert(typ) {
        return None;
    }
    match types.get(typ) {
        TypeData::Named(n) => {
            let u = n.underlying()?;
            find_sensitive_field(types, objects, u, tag_key, pattern, visited)
        }
        TypeData::Pointer(p) => {
            find_sensitive_field(types, objects, p.elem(), tag_key, pattern, visited)
        }
        TypeData::Slice(s) => {
            find_sensitive_field(types, objects, s.elem(), tag_key, pattern, visited)
        }
        TypeData::Array(a) => {
            find_sensitive_field(types, objects, a.elem(), tag_key, pattern, visited)
        }
        TypeData::Map(m) => {
            find_sensitive_field(types, objects, m.elem(), tag_key, pattern, visited)
        }
        TypeData::Interface(i) => {
            for k in 0..i.num_embeddeds() {
                if let Some(m) = find_sensitive_field(
                    types,
                    objects,
                    i.embedded_type(k),
                    tag_key,
                    pattern,
                    visited,
                ) {
                    return Some(m);
                }
            }
            None
        }
        TypeData::Struct(_) => find_sensitive_serialized_field(types, objects, typ, tag_key, pattern),
        _ => None,
    }
}

fn find_sensitive_serialized_field(
    types: &guff_types::arena::TypeArena,
    objects: &guff_types::arena::ObjectArena,
    typ: TypeId,
    tag_key: &str,
    pattern: &Regex,
) -> Option<Match> {
    let TypeData::Struct(st) = types.get(typ) else {
        return None;
    };
    for i in 0..st.num_fields() {
        let field = st.field(i);
        let name = field.name(objects).to_string();
        if name == "_" || !guff_types::object::is_exported(&name) {
            continue;
        }
        let Some(ft) = field.typ(objects) else {
            continue;
        };
        if !is_secret_candidate_type(types, ft, 0) {
            continue;
        }
        let (key, omitted) = serialized_name_from_tag(&name, st.tag(i), tag_key);
        if omitted {
            continue;
        }
        if pattern.is_match(&name) || pattern.is_match(&key) {
            return Some(Match {
                field_name: name,
                serialized_key: key,
            });
        }
    }
    None
}

/// A field type that can hold a secret: a string, or a container of bytes.
fn is_secret_candidate_type(
    types: &guff_types::arena::TypeArena,
    typ: TypeId,
    depth: u32,
) -> bool {
    if depth > 16 {
        return false;
    }
    match types.get(typ) {
        TypeData::Named(n) => match n.underlying() {
            Some(u) => is_secret_candidate_type(types, u, depth + 1),
            None => false,
        },
        TypeData::Basic(b) => b.kind() == guff_types::BasicKind::String,
        TypeData::Pointer(p) => is_secret_candidate_type(types, p.elem(), depth + 1),
        TypeData::Slice(s) => {
            if is_byte(types, s.elem()) {
                return true;
            }
            is_secret_candidate_type(types, s.elem(), depth + 1)
        }
        TypeData::Array(a) => {
            if is_byte(types, a.elem()) {
                return true;
            }
            is_secret_candidate_type(types, a.elem(), depth + 1)
        }
        _ => false,
    }
}

fn is_byte(types: &guff_types::arena::TypeArena, typ: TypeId) -> bool {
    matches!(types.get(typ), TypeData::Basic(b) if b.kind() == guff_types::BasicKind::Uint8)
}

/// `serializedNameFromTag`: the key the format will actually write, and whether
/// the field is omitted entirely (`-`).
fn serialized_name_from_tag(default_name: &str, tag: &str, tag_key: &str) -> (String, bool) {
    if tag.is_empty() {
        return (default_name.to_string(), false);
    }
    let Some(value) = struct_tag_get(tag, tag_key) else {
        return (default_name.to_string(), false);
    };
    if value.is_empty() {
        return (default_name.to_string(), false);
    }
    if value == "-" {
        return (String::new(), true);
    }
    let name = match value.find(',') {
        Some(i) => &value[..i],
        None => value.as_str(),
    };
    if name.is_empty() {
        return (default_name.to_string(), false);
    }
    (name.to_string(), false)
}

/// `reflect.StructTag.Get` — the conventional `key:"value"` scan, including its
/// quirks: keys are read up to the first space or colon, and a malformed tag
/// stops the scan rather than being skipped.
fn struct_tag_get(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    while !rest.is_empty() {
        let start = rest.len() - rest.trim_start_matches(' ').len();
        rest = &rest[start..];
        if rest.is_empty() {
            break;
        }
        let name_end = rest.find(|c: char| c == ':' || c == ' ' || (c as u32) < 0x20 || c == '"')?;
        if rest.as_bytes()[name_end] != b':' {
            return None;
        }
        let name = &rest[..name_end];
        rest = &rest[name_end + 1..];
        if !rest.starts_with('"') {
            return None;
        }
        // Find the closing quote, honouring backslash escapes.
        let bytes = rest.as_bytes();
        let mut i = 1;
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let quoted = &rest[..=i];
        rest = &rest[i + 1..];
        if name == key {
            return crate::gosec::unquote_string_lit(quoted);
        }
    }
    None
}

/// `compositeLitFieldIsTransformed`: the literal assigns a call result to the
/// field, which reads as masking it before serialization.
fn composite_lit_field_is_transformed(expr: &Expr, field_name: &str) -> bool {
    let expr = match expr {
        Expr::UnaryExpr(u) => &*u.x,
        other => other,
    };
    let Expr::CompositeLit(lit) = expr else {
        return false;
    };
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = &*kv.key else {
            continue;
        };
        if key.name != field_name {
            continue;
        }
        return matches!(&*kv.value, Expr::CallExpr(_));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_tag_get_reads_the_conventional_form() {
        assert_eq!(
            struct_tag_get(r#"json:"password" valid:"required""#, "json").as_deref(),
            Some("password")
        );
        assert_eq!(
            struct_tag_get(r#"json:"password" valid:"required""#, "valid").as_deref(),
            Some("required")
        );
        assert_eq!(struct_tag_get(r#"json:"password""#, "yaml"), None);
        assert_eq!(struct_tag_get("", "json"), None);
    }

    #[test]
    fn serialized_name_follows_the_tag() {
        assert_eq!(
            serialized_name_from_tag("Password", r#"json:"pass,omitempty""#, "json"),
            ("pass".to_string(), false)
        );
        // An empty name in the tag falls back to the field name.
        assert_eq!(
            serialized_name_from_tag("Password", r#"json:",omitempty""#, "json"),
            ("Password".to_string(), false)
        );
        // `-` omits the field from the output entirely.
        assert_eq!(
            serialized_name_from_tag("Password", r#"json:"-""#, "json"),
            (String::new(), true)
        );
        // A tag that says nothing about this format keeps the field name.
        assert_eq!(
            serialized_name_from_tag("Password", r#"yaml:"pass""#, "json"),
            ("Password".to_string(), false)
        );
    }

    #[test]
    fn default_pattern_matches_what_upstream_matches() {
        let re = Regex::new(G117_DEFAULT_PATTERN).unwrap();
        for yes in ["Password", "password", "Secret", "cred", "jwt", "api_key", "AccessToken"] {
            assert!(re.is_match(yes), "{yes}");
        }
        // Measured against golangci-lint: a bare `Token` is not on the list —
        // the token alternatives all require a prefix like `api`/`access`.
        for no in ["Token", "Name", "Flow", "UserCode"] {
            assert!(!re.is_match(no), "{no}");
        }
    }
}
