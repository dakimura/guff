//! `struct-tag` — check common struct tags (`json`, `xml`, `yaml`, …).
//!
//! DEFERRED: full upstream tag checker table (asn1, bson, protobuf, …) and
//! user-defined tag options from `linters.settings.revive`.

use guff::ast::Field;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::basic_lit_string;

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
                    let NodeRef::StructType(st) = n else { return; };
                    if st.fields.list.is_empty() {
                        return;
                    }
                    for field in &st.fields.list {
                        check_field(field, &mut self.failures);
                    }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}


fn check_field(field: &Field, failures: &mut Vec<Failure>) {
    let Some(tag_lit) = &field.tag else {
        return;
    };
    let Some(raw) = basic_lit_string(tag_lit) else {
        return;
    };
    let tags = match parse_struct_tags(raw) {
        Ok(tags) => tags,
        Err(()) => {
            failures.push(Failure {
                rule: "struct-tag",
                pos: tag_lit.value_pos.0 as u32,
                message: "malformed tag".into(),
                ..Failure::default()
            });
            return;
        }
    };

    for tag in tags {
        if let Some(msg) = check_options_on_ignored_field(&tag) {
            failures.push(Failure {
                rule: "struct-tag",
                pos: tag_lit.value_pos.0 as u32,
                message: msg,
                ..Failure::default()
            });
        }
        if let Some(msg) = check_tag(&tag) {
            failures.push(Failure {
                rule: "struct-tag",
                pos: tag_lit.value_pos.0 as u32,
                // Upstream: `w.addFailuref(n, "%s in %s tag", msg, tagKey)`.
                message: format!("{msg} in {} tag", tag.key),
                ..Failure::default()
            });
        }
    }

    if shall_warn_on_unexported_field(field) {
        let name = field
            .names
            .first()
            .map(|n| n.name.as_str())
            .unwrap_or("<field>");
        failures.push(Failure {
            rule: "struct-tag",
            pos: field.pos().0 as u32,
            message: format!("tag on not-exported field {name}"),
            ..Failure::default()
        });
    }
}

fn shall_warn_on_unexported_field(field: &Field) -> bool {
    let Some(name) = field.names.first() else {
        return false;
    };
    if name.name.is_empty() || name.name.starts_with('_') {
        return false;
    }
    name.name.chars().next().is_some_and(|c| !c.is_uppercase())
}

#[derive(Debug, Clone)]
struct StructTag {
    key: String,
    name: String,
    options: Vec<String>,
}

fn parse_struct_tags(raw: &str) -> Result<Vec<StructTag>, ()> {
    let mut tags = Vec::new();
    let mut rest = raw.trim();
    while !rest.is_empty() {
        let key_end = rest.find(':').ok_or(())?;
        let key = rest[..key_end].trim();
        if key.is_empty() {
            return Err(());
        }
        rest = &rest[key_end + 1..];
        rest = rest.trim_start();
        if !rest.starts_with('"') {
            return Err(());
        }
        rest = &rest[1..];
        let (value, consumed) = parse_quoted_value(rest)?;
        rest = &rest[consumed..];
        let (name, options) = split_tag_value(&value);
        tags.push(StructTag {
            key: key.to_string(),
            name,
            options,
        });
        rest = rest.trim();
    }
    Ok(tags)
}

fn parse_quoted_value(s: &str) -> Result<(String, usize), ()> {
    let mut out = String::new();
    let mut escaped = false;
    for (i, ch) in s.char_indices() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '"' {
            return Ok((out, i + 1));
        }
        out.push(ch);
    }
    Err(())
}

fn split_tag_value(value: &str) -> (String, Vec<String>) {
    let mut parts = value.split(',');
    let name = parts.next().unwrap_or("").to_string();
    let options = parts.map(|p| p.to_string()).collect();
    (name, options)
}

fn check_options_on_ignored_field(tag: &StructTag) -> Option<String> {
    if tag.name != "-" {
        return None;
    }
    let useful: Vec<_> = tag
        .options
        .iter()
        .map(|o| o.trim())
        .filter(|o| !o.is_empty())
        .collect();
    if useful.is_empty() {
        None
    } else {
        Some(format!(
            "options on ignored field (tag key {}) are useless",
            tag.key
        ))
    }
}

fn check_tag(tag: &StructTag) -> Option<String> {
    match tag.key.as_str() {
        "json" => check_json_tag(tag),
        "yaml" => check_yaml_tag(tag),
        "xml" => check_xml_tag(tag),
        _ => None,
    }
}

fn check_json_tag(tag: &StructTag) -> Option<String> {
    for opt in &tag.options {
        match opt.as_str() {
            "omitempty" | "string" | "omitzero" => {}
            "" if tag.name == "-" => {}
            "" => return Some("option can not be empty".into()),
            other => return Some(format!("unknown option \"{other}\"")),
        }
    }
    None
}

fn check_yaml_tag(tag: &StructTag) -> Option<String> {
    for opt in &tag.options {
        match opt.as_str() {
            "flow" | "inline" | "omitempty" => {}
            other => return Some(format!("unknown option \"{other}\"")),
        }
    }
    None
}

fn check_xml_tag(tag: &StructTag) -> Option<String> {
    for opt in &tag.options {
        match opt.as_str() {
            "any"
            | "attr"
            | "cdata"
            | "chardata"
            | "comment"
            | "innerxml"
            | "omitempty"
            | "typeattr" => {}
            other => return Some(format!("unknown option \"{other}\"")),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_tag() {
        let tags = parse_struct_tags(r#"json:"name,omitempty" yaml:"name""#).unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].key, "json");
        assert_eq!(tags[0].name, "name");
        assert_eq!(tags[0].options, vec!["omitempty"]);
    }
}
