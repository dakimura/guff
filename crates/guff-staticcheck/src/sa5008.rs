//! SA5008 — invalid struct tag.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5008` (simplified).

use std::sync::OnceLock;

use guff::ast::{Field, StructType};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::structtag;

fn check_xml_tag(_pass: &Pass<'_>, field: &Field, tag: &str, pending: &mut Vec<(u32, String)>) {
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
    // encoding/json knows what it is doing, and it tests itself.
    let pkg = pass.pkg().pkg_path.as_str();
    if matches!(
        pkg,
        "encoding/json" | "encoding/json_test" | "encoding/json/v2" | "encoding/json/v2_test"
    ) {
        return;
    }
    crate::sa5008_json::validate_json_tag(pass, field, tag, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5008 requires inspect analyzer".to_string())?
        .clone();

    let go_flags = imports_go_flags(pass);
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(StructType), pass.files(), |n| {
        let NodeRef::StructType(st) = n else {
            return;
        };
        check_struct(pass, st, go_flags, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn check_struct(
    pass: &Pass<'_>,
    st: &StructType,
    imports_go_flags: bool,
    pending: &mut Vec<(u32, String)>,
) {
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
                    // `go-flags` repeats these three by design, so upstream
                    // exempts them in any package that imports it.
                    let is_go_flags_tag = imports_go_flags
                        && matches!(k.as_str(), "choice" | "optional-value" | "default");
                    if v.len() > 1 && !is_go_flags_tag {
                        pending.push((
                            tag_lit.value_pos.0 as u32,
                            format!("duplicate struct tag {k:?}"),
                        ));
                    }
                    // Only the first value is validated, as upstream does.
                    let Some(val) = v.first() else {
                        continue;
                    };
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

/// Upstream reads the *syntax* rather than `(*types.Package).Imports` to work
/// around vendored paths in GOPATH mode, so this does too.
fn imports_go_flags(pass: &Pass<'_>) -> bool {
    pass.files().iter().any(|f| {
        f.imports.iter().any(|imp| {
            let v = imp.path.value.as_str();
            v.len() >= 2 && &v[1..v.len() - 1] == "github.com/jessevdk/go-flags"
        })
    })
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
