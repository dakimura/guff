//! `structtag` — check struct field tags.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, Field, StructType};
use guff::node_mask;
use guff::position::Pos;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_gostd::strconv;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TagKey {
    key: String,
    name: String,
    level: i32,
}

/// The tag's value, as `go/types` records it.
///
/// Upstream does not re-read the literal at all: `structtag` walks
/// `types.Struct`, whose `Tag(i)` the type checker filled in with
/// `strconv.Unquote(lit.Value)` (and left empty when that failed). This port
/// walks the AST, so it unquotes here — with the same function, ported.
///
/// An earlier revision hand-rolled it as "drop the backslash, keep the next
/// character". That is right for `\"` and `\\`, which is every tag anyone
/// writes, and wrong for every other escape: `"json:\t"` came out as `json:t`,
/// so the *value* — not only its rendering — was wrong wherever a tag was
/// written as an interpreted string literal with a real escape in it.
///
/// Returning `None` where `strconv.Unquote` errors matches the checker's own
/// fallback: the tag becomes `""`, and `validate_struct_tag("")` has nothing to
/// report.
fn raw_tag_value(raw: &str) -> Option<String> {
    strconv::unquote(raw).ok()
}

fn quoted_value_end(s: &str) -> Option<usize> {
    // `s` starts with `"`. Scan like reflect.StructTag / skip_value.
    if !s.starts_with('"') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

/// `reflect.StructTag.Get`: the whole value, options included.
///
/// The options matter twice. `xml:"a,attr"` names an attribute, and vet keeps
/// attributes in a namespace of their own so they cannot collide with an
/// element of the same name; and a tag that is *only* options (`json:",inline"`)
/// is what tells the unexported-field check that the field really is tagged.
fn tag_get_raw(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let colon = rest.find(':')?;
        let k = &rest[..colon];
        if k != key {
            rest = skip_value(&rest[colon + 1..]);
            continue;
        }
        rest = rest[colon + 1..].trim_start();
        let end = quoted_value_end(rest)?;
        return strconv::unquote(&rest[..end]).ok();
    }
    None
}

fn skip_value(s: &str) -> &str {
    let s = s.trim_start();
    match quoted_value_end(s) {
        Some(end) => &s[end..],
        None => "",
    }
}

fn validate_struct_tag(tag: &str) -> Option<&'static str> {
    let mut n = 0;
    let mut rest = tag;
    while !rest.is_empty() {
        if n > 0 && !rest.starts_with(' ') {
            return Some("key:\"value\" pairs not separated by spaces");
        }
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let colon = match rest.find(':') {
            Some(i) if i > 0 => i,
            _ => return Some("bad syntax for struct tag pair"),
        };
        let key = &rest[..colon];
        if key.is_empty() || key.bytes().any(|b| b <= b' ' || b == b':' || b == b'"' || b == 0x7f) {
            return Some("bad syntax for struct tag key");
        }
        rest = &rest[colon + 1..];
        let Some(end) = quoted_value_end(rest) else {
            return Some("bad syntax for struct tag value");
        };
        let q = &rest[..end];
        let Ok(value) = strconv::unquote(q) else {
            return Some("bad syntax for struct tag value");
        };
        if let Some(err) = check_tag_spaces(key, &value) {
            return Some(err);
        }
        rest = &rest[end..];
        n += 1;
    }
    None
}

/// Upstream `checkTagSpaces` for json/xml/asn1 (go/analysis/passes/structtag).
fn check_tag_spaces(key: &str, value: &str) -> Option<&'static str> {
    match key {
        "xml" => {
            // Leading/trailing space, or more than one space, is suspicious.
            if value.trim() != value || value.bytes().filter(|&b| b == b' ').count() > 1 {
                return Some("suspicious space in struct tag value");
            }
            let Some(comma) = value.find(',') else {
                return None;
            };
            if comma > 0 && value.as_bytes()[comma - 1] == b' ' {
                return Some("suspicious space in struct tag value");
            }
            // Options after the name must not contain spaces.
            if value[comma + 1..].contains(' ') {
                return Some("suspicious space in struct tag value");
            }
            None
        }
        "json" => {
            // JSON allows spaces in the name; only flag spaces in options.
            let Some(comma) = value.find(',') else {
                return None;
            };
            if value[comma + 1..].contains(' ') {
                return Some("suspicious space in struct tag value");
            }
            None
        }
        "asn1" => {
            if value.contains(' ') {
                return Some("suspicious space in struct tag value");
            }
            None
        }
        _ => None,
    }
}

fn field_exported(name: &str) -> bool {
    name
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

/// `types.Var.Name()` for a struct field: the declared name, or — for an
/// embedded field — the name of the embedded type.
fn field_name(field: &Field) -> Option<String> {
    if let Some(ident) = field.names.first() {
        return Some(ident.name.clone());
    }
    fn type_name(expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident(id) => Some(id.name.clone()),
            Expr::SelectorExpr(sel) => Some(sel.sel.name.clone()),
            Expr::StarExpr(star) => type_name(&star.x),
            Expr::IndexExpr(ix) => type_name(&ix.x),
            _ => None,
        }
    }
    field.ty.as_ref().and_then(type_name)
}

/// The `also at …` half of the duplicate message.
///
/// Upstream zeroes the column and rewrites the filename relative to the
/// directory of the field being reported, so that a collision reached through
/// an embedded field in another package still names a path the reader can
/// follow. A zero column makes `token.Position` print `file:line`.
fn also_at(pass: &Pass<'_>, this_pos: u32, also_pos: u32) -> String {
    let this = pass.fset().position(Pos(this_pos as i64));
    let also = pass.fset().position(Pos(also_pos as i64));
    let also_path = Path::new(&also.filename);
    let name = match (Path::new(&this.filename).parent(), also_path.parent()) {
        (Some(a), Some(b)) if a == b => also_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| also.filename.clone()),
        // Upstream leaves the filename alone when it cannot relativise it.
        _ => also.filename.clone(),
    };
    if also.line > 0 {
        format!("{name}:{}", also.line)
    } else {
        name
    }
}

fn check_field(
    pass: &Pass<'_>,
    pkg_path: &str,
    field: &Field,
    seen: &mut HashMap<TagKey, u32>,
    pending: &mut Vec<(u32, String)>,
) {
    if pkg_path.starts_with("encoding/") {
        return;
    }
    let tag = field
        .tag
        .as_ref()
        .and_then(|BasicLit { value, .. }| raw_tag_value(value));
    let Some(tag) = tag else {
        return;
    };
    if let Some(err) = validate_struct_tag(&tag) {
        pending.push((
            field.pos().0 as u32,
            format!(
                "struct field tag {} not compatible with reflect.StructTag.Get: {err}",
                // `%#q`, not `%q`: upstream prefers a backquoted rendering, and
                // a struct tag nearly always can be backquoted.
                strconv::quote_sharp(&tag)
            ),
        ));
    }
    for key in ["json", "xml"] {
        let Some(val) = tag_get_raw(&tag, key) else {
            continue;
        };
        if val == "-" {
            // Ignored, even if the field is anonymous.
            continue;
        }
        if val.is_empty() || val.starts_with(',') {
            // A tag with no encoding name. Upstream recurses into an anonymous
            // field's struct here at level+1; DEFERRED — that needs the field's
            // type, and this port walks the AST.
            continue;
        }
        // `XMLName` names the element of the struct being checked, so it cannot
        // collide with the element or attribute names of that struct's own
        // fields. gitea's codebase downloader declares an `XMLName` and a field
        // for the same element name, one level apart.
        if key == "xml" && field_name(field).as_deref() == Some("XMLName") {
            continue;
        }
        // Tag options are not part of the encoding name: `json:"a,omitempty"`
        // names `a`. XML attributes get a namespace of their own, which
        // upstream spells by extending the key — and the key is part of the
        // message.
        let (tag_kind, name) = match val.find(',') {
            Some(i) => {
                let mut kind = key.to_string();
                if key == "xml" && val[i..].split(',').any(|opt| opt == "attr") {
                    kind.push_str(" attribute");
                }
                (kind, val[..i].to_string())
            }
            None => (key.to_string(), val.clone()),
        };
        let tkey = TagKey {
            key: tag_kind.clone(),
            name: name.clone(),
            level: 1,
        };
        let pos = field.pos().0 as u32;
        if let Some(earlier) = seen.insert(tkey, pos) {
            let who = field_name(field).unwrap_or_default();
            pending.push((
                pos,
                format!(
                    "struct field {who} repeats {tag_kind} tag {} also at {}",
                    // Plain `%q` here — this arm has no backquote form.
                    strconv::quote(&name),
                    also_at(pass, pos, earlier)
                ),
            ));
        }
    }
    // Upstream skips anonymous (embedded) fields for the unexported-tag
    // check (`field.Anonymous()` → return). Empty `names` means embedded.
    let Some(ident) = field.names.first() else {
        return;
    };
    let name = ident.name.as_str();
    if !field_exported(name) {
        for enc in ["json", "xml"] {
            match tag_get_raw(&tag, enc) {
                Some(v) if !v.is_empty() && v != "-" => {
                    pending.push((
                        field.pos().0 as u32,
                        format!("struct field {name} has {enc} tag but is not exported"),
                    ));
                    return;
                }
                _ => {}
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "structtag requires inspect analyzer".to_string())?
        .clone();

    let pkg_path = pass.pkg().pkg_path.clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(StructType), pass.files(), |n| {
        let NodeRef::StructType(StructType { fields, .. }) = n else {
            return;
        };
        // Duplicate json/xml tags are per-struct, not package-wide (go vet).
        let mut seen = HashMap::new();
        for field in &fields.list {
            check_field(pass, &pkg_path, field, &mut seen, &mut pending);
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "structtag",
        doc: "check that struct field tags conform to reflect.StructTag.Get",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/structtag",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
