//! Port of [`github.com/ldez/tagliatelle`](https://github.com/ldez/tagliatelle)
//! (golangci-lint wrapper in `pkg/golinters/tagliatelle`).
//!
//! Checks struct-tag value casing for configured keys. Golangci-lint defaults
//! are `json`/`yaml` → `camel`, `header` → `header`.
//!
//! DEFERRED (see DEVELOPMENT.md R13/R14): package `overrides` (radix tree),
//! `extended-rules` ExtraInitialisms / InitialismOverrides, SuggestedFix.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{Expr, Field, StructType};
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::TagliatelleOptions;

fn default_rules() -> HashMap<String, String> {
    HashMap::from([
        ("json".into(), "camel".into()),
        ("yaml".into(), "camel".into()),
        ("header".into(), "header".into()),
    ])
}

fn unquote_tag_lit(value: &str) -> Option<String> {
    let s = value.trim();
    if s.len() < 2 {
        return None;
    }
    let bytes = s.as_bytes();
    if bytes[0] == b'`' && bytes[s.len() - 1] == b'`' {
        return Some(s[1..s.len() - 1].to_string());
    }
    if bytes[0] == b'"' && bytes[s.len() - 1] == b'"' {
        return Some(s[1..s.len() - 1].replace("\\\"", "\""));
    }
    None
}

/// Lookup a key in a Go struct tag (already unquoted content).
fn lookup_tag_value(content: &str, key: &str) -> Option<(String, Vec<String>)> {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key_start = i;
        while i < bytes.len() && bytes[i] != b':' && !bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == key_start || i >= bytes.len() || bytes[i] != b':' {
            return None;
        }
        let found_key = String::from_utf8_lossy(&bytes[key_start..i]).into_owned();
        i += 1; // ':'
        if i >= bytes.len() || bytes[i] != b'"' {
            return None;
        }
        i += 1; // opening '"'
        let val_start = i;
        while i < bytes.len() && bytes[i] != b'"' {
            if bytes[i] == b'\\' {
                i += 1;
            }
            i += 1;
        }
        if i >= bytes.len() {
            return None;
        }
        let value = String::from_utf8_lossy(&bytes[val_start..i]).into_owned();
        i += 1; // closing '"'
        if found_key == key {
            let mut parts = value.split(',').map(|s| s.to_string()).collect::<Vec<_>>();
            let name = if parts.is_empty() {
                String::new()
            } else {
                parts.remove(0)
            };
            return Some((name, parts));
        }
    }
    None
}

fn split_words(s: &str) -> Vec<String> {
    let chars: Vec<char> = s
        .chars()
        .flat_map(|c| {
            if c == '-' || c == ' ' {
                vec!['_']
            } else {
                vec![c]
            }
        })
        .collect();
    let mut words = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' {
            i += 1;
            continue;
        }
        let start = i;
        if chars[i].is_ascii_uppercase() {
            i += 1;
            while i < chars.len() && chars[i].is_ascii_uppercase() {
                i += 1;
            }
            if i < chars.len() && chars[i].is_ascii_lowercase() && i - start > 1 {
                // HTTPServer → HTTP + Server
                i -= 1;
            }
            while i < chars.len() && chars[i].is_ascii_lowercase() {
                i += 1;
            }
        } else if chars[i].is_ascii_lowercase() || chars[i].is_ascii_digit() {
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_lowercase() || chars[i].is_ascii_digit())
            {
                i += 1;
            }
        } else {
            i += 1;
            continue;
        }
        words.push(chars[start..i].iter().collect());
    }
    words
}

fn title_word(w: &str) -> String {
    let mut chars = w.chars();
    let mut part = String::new();
    if let Some(f) = chars.next() {
        part.push(f.to_ascii_uppercase());
        for c in chars {
            part.push(c.to_ascii_lowercase());
        }
    }
    part
}

fn to_camel(s: &str) -> String {
    let words = split_words(s);
    if words.is_empty() {
        return s.to_string();
    }
    let mut out = words[0].to_ascii_lowercase();
    for w in &words[1..] {
        out.push_str(&title_word(w));
    }
    out
}

fn to_pascal(s: &str) -> String {
    split_words(s).iter().map(|w| title_word(w)).collect()
}

fn to_snake(s: &str) -> String {
    split_words(s)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

fn to_kebab(s: &str) -> String {
    split_words(s)
        .iter()
        .map(|w| w.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("-")
}

fn to_header(s: &str) -> String {
    split_words(s)
        .iter()
        .map(|w| title_word(w))
        .collect::<Vec<_>>()
        .join("-")
}

fn to_upper_snake(s: &str) -> String {
    split_words(s)
        .iter()
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Simple converters matching ettle/strcase (non-go / go without custom initialisms).
/// `go*` variants use the same splitting; Go initialism preservation is DEFERRED.
fn convert(case: &str, s: &str) -> Result<String, String> {
    match case {
        "camel" | "goCamel" => Ok(to_camel(s)),
        "pascal" | "goPascal" => Ok(to_pascal(s)),
        "kebab" | "goKebab" => Ok(to_kebab(s)),
        "snake" | "goSnake" => Ok(to_snake(s)),
        "upperSnake" => Ok(to_upper_snake(s)),
        "header" => Ok(to_header(s)),
        "upper" => Ok(s.to_ascii_uppercase()),
        "lower" => Ok(s.to_ascii_lowercase()),
        other => Err(format!("unsupported case: {other}")),
    }
}

fn get_type_name(exp: &Expr) -> Option<String> {
    match exp {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::StarExpr(st) => get_type_name(&st.x),
        Expr::SelectorExpr(sel) => Some(sel.sel.name.clone()),
        _ => None,
    }
}

fn get_field_name(field: &Field) -> Option<String> {
    for n in &field.names {
        if !n.name.is_empty() {
            return Some(n.name.clone());
        }
    }
    field.ty.as_ref().and_then(get_type_name)
}

fn analyze_struct(
    st: &StructType,
    opts: &TagliatelleOptions,
    rules: &HashMap<String, String>,
    reports: &mut Vec<(u32, String)>,
) {
    if st.fields.list.is_empty() {
        return;
    }
    for field in &st.fields.list {
        let Some(tag) = field.tag.as_ref() else {
            continue;
        };
        let Some(field_name) = get_field_name(field) else {
            continue;
        };
        if opts.ignored_fields.iter().any(|f| f == &field_name) {
            continue;
        }
        let Some(content) = unquote_tag_lit(&tag.value) else {
            continue;
        };

        for (key, conv_name) in rules {
            if conv_name.is_empty() {
                continue;
            }
            let Some((mut value, flags)) = lookup_tag_value(&content, key) else {
                continue;
            };
            if value == "-" {
                continue;
            }
            if key == "xml" && (value.contains('>') || value.contains(':')) {
                continue;
            }
            if flags.iter().any(|f| f == "inline") {
                continue;
            }
            if value.is_empty() {
                value = field_name.clone();
            }
            let expected_src = if opts.use_field_name {
                field_name.clone()
            } else {
                value.clone()
            };
            let expected = match convert(conv_name, &expected_src) {
                Ok(v) => v,
                Err(e) => {
                    reports.push((tag.pos().0 as u32, format!("{key}({conv_name}): {e}")));
                    continue;
                }
            };
            if value != expected {
                reports.push((
                    tag.pos().0 as u32,
                    format!("{key}({conv_name}): got '{value}' want '{expected}'"),
                ));
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "tagliatelle requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<TagliatelleOptions>("tagliatelle")
        .cloned()
        .unwrap_or_default();

    if opts.ignore {
        return Ok(None);
    }

    let mut rules = default_rules();
    for (k, v) in &opts.rules {
        rules.insert(k.clone(), v.clone());
    }
    // Extended-rules keys shadow simple rules (upstream cleanRules).
    for k in opts.extended_rules.keys() {
        rules.remove(k);
    }
    // Apply extended-rules as simple case converters (ExtraInitialisms DEFERRED).
    for (k, case_name) in &opts.extended_rules {
        rules.insert(k.clone(), case_name.clone());
    }

    if rules.is_empty() {
        return Ok(None);
    }

    let mut reports: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::StructType(st) = n {
                analyze_struct(st, &opts, &rules, &mut reports);
            }
            true
        });
    }

    for (pos, message) in reports {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "tagliatelle",
        doc: "Checks the struct tags.",
        url: "https://github.com/ldez/tagliatelle",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camel_matches_golangci_fixtures() {
        assert_eq!(to_camel("ID"), "id");
        assert_eq!(to_camel("UserID"), "userId");
        assert_eq!(to_camel("CommonServiceItem"), "commonServiceItem");
        assert_eq!(to_camel("Value"), "value");
        assert_eq!(to_camel("name"), "name");
    }

    #[test]
    fn header_case() {
        assert_eq!(to_header("ContentType"), "Content-Type");
        assert_eq!(to_header("XRequestID"), "X-Request-Id");
    }

    #[test]
    fn lookup_json_tag() {
        let (name, flags) = lookup_tag_value(r#"json:"userId,omitempty" yaml:"user_id""#, "json")
            .expect("json");
        assert_eq!(name, "userId");
        assert_eq!(flags, vec!["omitempty"]);
        let (name, _) = lookup_tag_value(r#"json:"-""#, "json").unwrap();
        assert_eq!(name, "-");
    }
}
