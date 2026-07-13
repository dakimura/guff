//! `structtag` — check struct field tags.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, Field, StructType};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TagKey {
    key: String,
    name: String,
    level: i32,
}

fn raw_tag_value(raw: &str) -> Option<String> {
    if raw.len() >= 2 && raw.starts_with('`') && raw.ends_with('`') {
        return Some(raw[1..raw.len() - 1].to_string());
    }
    unquote_tag(raw)
}

fn unquote_tag(raw: &str) -> Option<String> {
    if raw.len() < 2 || !raw.starts_with('"') {
        return None;
    }
    let inner = &raw[1..raw.len() - 1];
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next();
            continue;
        }
        out.push(c);
    }
    Some(out)
}

fn tag_get(tag: &str, key: &str) -> Option<String> {
    let mut rest = tag;
    while !rest.is_empty() {
        if !rest.starts_with(' ') {
            if rest.starts_with(' ') == false && rest.chars().next() != Some(' ') {
                // skip to key
            }
        }
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
        if !rest.starts_with('"') {
            return None;
        }
        let end = rest[1..].find('"')? + 2;
        let q = &rest[..end];
        let val = unquote_tag(q)?;
        let comma = val.find(',').map(|i| &val[..i]).unwrap_or(&val);
        return Some(comma.to_string());
    }
    None
}

fn skip_value(s: &str) -> &str {
    let s = s.trim_start();
    if !s.starts_with('"') {
        return "";
    }
    let mut i = 1;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            i += 2;
            continue;
        }
        if bytes[i] == b'"' {
            return &s[i + 1..];
        }
        i += 1;
    }
    ""
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
        if key.is_empty() || key.bytes().any(|b| b <= b' ' || b == b':' || b == b'"') {
            return Some("bad syntax for struct tag key");
        }
        rest = &rest[colon + 1..];
        if !rest.starts_with('"') {
            return Some("bad syntax for struct tag value");
        }
        let end = match rest[1..].find('"') {
            Some(i) => i + 2,
            None => return Some("bad syntax for struct tag value"),
        };
        let q = &rest[..end];
        if unquote_tag(q).is_none() {
            return Some("bad syntax for struct tag value");
        }
        if (key == "json" || key == "xml" || key == "asn1")
            && q.contains(' ')
            && !q.contains("\\ ")
        {
            return Some("suspicious space in struct tag value");
        }
        rest = &rest[end..];
        n += 1;
    }
    None
}

fn field_exported(name: &str) -> bool {
    name
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
}

fn check_field(
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
                "struct field tag {:?} not compatible with reflect.StructTag.Get: {err}",
                tag
            ),
        ));
    }
    for key in ["json", "xml"] {
        if let Some(val) = tag_get(&tag, key) {
            if val.is_empty() || val.starts_with(',') {
                continue;
            }
            let tkey = TagKey {
                key: key.to_string(),
                name: val.clone(),
                level: 1,
            };
            if seen.insert(tkey.clone(), field.pos().0 as u32).is_some() {
                pending.push((
                    field.pos().0 as u32,
                    format!("struct field repeats {key} tag {val:?}"),
                ));
            }
        }
    }
    let name = field.names.first().map(|n| n.name.as_str()).unwrap_or("");
    if !field_exported(name) {
        for enc in ["json", "xml"] {
            match tag_get(&tag, enc) {
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
    let mut seen = HashMap::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::StructType(StructType { fields, .. }) = n else {
            return;
        };
        for field in &fields.list {
            check_field(&pkg_path, field, &mut seen, &mut pending);
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
