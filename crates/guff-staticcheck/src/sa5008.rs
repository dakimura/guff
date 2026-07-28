//! SA5008 — invalid struct tag.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5008` (simplified).

use std::sync::OnceLock;

use guff::ast::{Field, StructType};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::object::is_exported;

use crate::structtag;

fn check_xml_tag(pass: &Pass<'_>, field: &Field, tag: &str, pending: &mut Vec<(u32, String)>) {
    let parts: Vec<&str> = tag.split(',').collect();
    let mut counts = std::collections::HashMap::new();
    for part in parts.iter().skip(1) {
        match *part {
            "attr" | "chardata" | "cdata" | "innerxml" | "comment" | "omitempty" | "any" | "" => {
                if !part.is_empty() {
                    *counts.entry(*part).or_insert(0) += 1;
                }
            }
            other => {
                pending.push((
                    field.tag.as_ref().map(|t| t.value_pos.0 as u32).unwrap_or(0),
                    format!("invalid XML tag: unknown option {other:?}"),
                ));
            }
        }
    }
    for (k, v) in counts {
        if v > 1 {
            pending.push((
                field.tag.as_ref().map(|t| t.value_pos.0 as u32).unwrap_or(0),
                format!("invalid XML tag: duplicate option {k:?}"),
            ));
        }
    }
}

fn check_json_tag(pass: &Pass<'_>, field: &Field, tag: &str, pending: &mut Vec<(u32, String)>) {
    let pkg = pass.pkg().pkg_path.as_str();
    if pkg.starts_with("encoding/json") {
        return;
    }
    if tag.is_empty() {
        return;
    }
    if !field.names.is_empty() && !is_exported(&field.names[0].name) && tag != "-" {
        pending.push((
            field.tag.as_ref().map(|t| t.value_pos.0 as u32).unwrap_or(0),
            format!("unexported struct field cannot have non-ignored `json:{tag:?}` tag"),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5008 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(StructType), pass.files(), |n| {
        let NodeRef::StructType(st) = n else {
            return;
        };
        check_struct(pass, st, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn check_struct(pass: &Pass<'_>, st: &StructType, pending: &mut Vec<(u32, String)>) {
    for field in &st.fields.list {
        let Some(tag_lit) = &field.tag else {
            continue;
        };
        let raw = tag_lit.value.trim_matches('`');
        let inner = raw
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(raw);
        match structtag::parse_struct_tag(inner) {
            Err(err) => pending.push((
                tag_lit.value_pos.0 as u32,
                format!("unparseable struct tag: {err}"),
            )),
            Ok(tags) => {
                for (k, v) in &tags {
                    if v.len() > 1 {
                        pending.push((
                            tag_lit.value_pos.0 as u32,
                            format!("duplicate struct tag {k:?}"),
                        ));
                    }
                    for val in v {
                        match k.as_str() {
                            "json" => check_json_tag(pass, field, val, pending),
                            "xml" => check_xml_tag(pass, field, val, pending),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn sa5008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5008",
        doc: "invalid struct tag",
        url: "https://staticcheck.dev/docs/checks/#SA5008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
