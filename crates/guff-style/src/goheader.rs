//! Port of [`github.com/denis-tingaikin/go-header`](https://github.com/denis-tingaikin/go-header)
//! (golangci-lint wrapper in `pkg/golinters/goheader`).
//!
//! Checks that each `.go` file's leading copyright / license header matches
//! a configured template. Without `template` / `template-path`, the analyzer
//! is a no-op (golangci / upstream behaviour).
//!
//! Built-in values: `YEAR` (current calendar year) and `YEAR_RANGE`
//! (`((20\d\d\-YEAR)|(YEAR))`). User `values.const` / `values.regexp` are
//! merged (both lower- and UPPER-case keys).
//!
//! DEFERRED (see DEVELOPMENT.md R13/R14):
//! - SuggestedFix / `--fix`
//! - `template-path` `${config-path}` / `${base-path}` placeholders
//! - custom `delims`, `vars` alias, MOD_YEAR from git mtime
//! - CGO positioning quirks / build-directive offset skip edge cases

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{Comment, CommentGroup, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::GoheaderOptions;

#[derive(Debug, Clone)]
enum ValueKind {
    Const,
    Regexp,
}

#[derive(Debug, Clone)]
struct Value {
    /// Distinguishes const vs regexp for SuggestedFix (DEFERRED).
    #[allow(dead_code)]
    kind: ValueKind,
    raw: String,
    calculated: String,
}

impl Value {
    fn const_val(raw: impl Into<String>) -> Self {
        Self {
            kind: ValueKind::Const,
            raw: raw.into(),
            calculated: String::new(),
        }
    }

    fn regexp_val(raw: impl Into<String>) -> Self {
        Self {
            kind: ValueKind::Regexp,
            raw: raw.into(),
            calculated: String::new(),
        }
    }

    fn get(&self) -> &str {
        if self.calculated.is_empty() {
            &self.raw
        } else {
            &self.calculated
        }
    }
}

fn current_year() -> String {
    // chrono-free: use local calendar year via system time.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Civil year approximation via days since 1970-01-01 (UTC). Good enough
    // for YEAR matching; matches upstream's `time.Now().Year()` in practice.
    let days = (secs / 86_400) as i64;
    let mut y = 1970i64;
    let mut rem = days;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if rem < diy {
            break;
        }
        rem -= diy;
        y += 1;
    }
    y.to_string()
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn built_in_values() -> HashMap<String, Value> {
    let year = current_year();
    let mut m = HashMap::new();
    // Upstream nests YEAR_RANGE on YEAR so overrides of YEAR propagate.
    let year_range = r"((20\d\d\-{{.YEAR}})|({{.YEAR}}))";
    m.insert("YEAR".to_string(), Value::const_val(year.clone()));
    m.insert("YEAR_RANGE".to_string(), Value::regexp_val(year_range));
    m.insert("year".to_string(), Value::const_val(year));
    m.insert("year_range".to_string(), Value::regexp_val(year_range));
    m
}

fn insert_both_cases(map: &mut HashMap<String, Value>, key: &str, v: Value) {
    map.insert(key.to_ascii_lowercase(), v.clone());
    map.insert(key.to_ascii_uppercase(), v);
}

/// Expand nested `{{ .NAME }}` / `{{NAME}}` references inside a raw value.
fn calculate_raw(raw: &str, values: &HashMap<String, Value>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err("missed value ending".into());
        };
        let mut name = after[..end].trim();
        if let Some(stripped) = name.strip_prefix('.') {
            name = stripped.trim();
        }
        let Some(val) = values.get(name).or_else(|| {
            values
                .get(&name.to_ascii_uppercase())
                .or_else(|| values.get(&name.to_ascii_lowercase()))
        }) else {
            return Err(format!("unknown value name {name}"));
        };
        // Ensure nested values are calculated (shallow: use get/raw).
        out.push_str(val.get());
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

fn calculate_all(values: &mut HashMap<String, Value>) -> Result<(), String> {
    // Multi-pass to resolve shallow nesting (YEAR_RANGE → YEAR, user nests).
    for _ in 0..8 {
        let keys: Vec<String> = values.keys().cloned().collect();
        for k in keys {
            let raw = values.get(&k).map(|v| v.raw.clone()).unwrap_or_default();
            let calculated = calculate_raw(&raw, values)?;
            if let Some(v) = values.get_mut(&k) {
                v.calculated = calculated;
            }
        }
    }
    Ok(())
}

/// Migrate legacy `{{ YEAR }}` / `{{ SOME VALUE }}` placeholders to
/// `{{ .YEAR }}` / `{{ .SOME_VALUE }}` (upstream `migrateOldConfig`).
fn migrate_old_config(input: &str) -> String {
    let re = Regex::new(r"\{\{\s*([^}]+)\s*\}\}").expect("migrate regexp");
    re.replace_all(input, |caps: &regex::Captures| {
        let inner = caps[1].trim();
        if inner.starts_with('.') {
            format!("{{{{ {inner} }}}}")
        } else {
            let converted = inner.replace(' ', "_");
            format!("{{{{ .{converted} }}}}")
        }
    })
    .into_owned()
}

/// Escape regexp metacharacters outside `{{ … }}` placeholders.
fn quote_meta(text: &str, left: &str, right: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    let n = bytes.len();
    let left_b = left.as_bytes();
    let right_b = right.as_bytes();
    while i < n {
        if i + left_b.len() <= n && &bytes[i..i + left_b.len()] == left_b {
            let mut end = i + left_b.len();
            while end + right_b.len() <= n && &bytes[end..end + right_b.len()] != right_b {
                end += 1;
            }
            if end + right_b.len() <= n {
                result.push_str(&text[i..end + right_b.len()]);
                i = end + right_b.len();
                continue;
            }
        }
        let c = bytes[i] as char;
        if r"\.+*?()|[]{}^$".contains(c) {
            result.push('\\');
        }
        result.push(c);
        i += 1;
    }
    result
}

/// Substitute `{{ .NAME }}` placeholders with each value's `get()` string.
fn execute_template(tmpl: &str, left: &str, right: &str, values: &HashMap<String, Value>) -> Result<String, String> {
    let mut result = String::with_capacity(tmpl.len());
    let mut i = 0;
    let bytes = tmpl.as_bytes();
    let n = bytes.len();
    let left_b = left.as_bytes();
    let right_b = right.as_bytes();
    while i < n {
        if i + left_b.len() <= n && &bytes[i..i + left_b.len()] == left_b {
            let mut end = i + left_b.len();
            while end + right_b.len() <= n && &bytes[end..end + right_b.len()] != right_b {
                end += 1;
            }
            if end + right_b.len() <= n {
                let inner = tmpl[i + left_b.len()..end].trim();
                let name = inner.strip_prefix('.').unwrap_or(inner).trim();
                let Some(val) = values.get(name).or_else(|| {
                    values
                        .get(&name.to_ascii_uppercase())
                        .or_else(|| values.get(&name.to_ascii_lowercase()))
                }) else {
                    return Err(format!("unknown value name {name}"));
                };
                result.push_str(val.get());
                i = end + right_b.len();
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    Ok(result)
}

fn build_values(opts: &GoheaderOptions) -> Result<HashMap<String, Value>, String> {
    let mut values = built_in_values();
    for (k, v) in &opts.const_values {
        insert_both_cases(&mut values, k, Value::const_val(v.clone()));
    }
    for (k, v) in &opts.regexp_values {
        insert_both_cases(&mut values, k, Value::regexp_val(v.clone()));
    }
    calculate_all(&mut values)?;
    Ok(values)
}

fn resolve_template(opts: &GoheaderOptions) -> Result<String, String> {
    if !opts.template.is_empty() {
        return Ok(migrate_old_config(opts.template.trim()));
    }
    if opts.template_path.is_empty() {
        return Ok(String::new());
    }
    // DEFERRED: ${config-path} / ${base-path} substitution.
    let raw = fs::read_to_string(&opts.template_path)
        .map_err(|e| format!("goheader template-path: {e}"))?;
    Ok(migrate_old_config(raw.trim()))
}

fn skip_directives<'a>(file: &'a File) -> Option<&'a CommentGroup> {
    for cg in &file.comments {
        if cg.pos().0 > file.package.0 {
            break;
        }
        let text = cg.text();
        let trimmed = text.trim();
        if trimmed.is_empty()
            || trimmed.starts_with("+build")
            || trimmed.starts_with("Code generated by cmd/cgo")
        {
            continue;
        }
        return Some(cg);
    }
    None
}

fn handle_star_block(header: &str) -> (String, bool) {
    let mut handled = false;
    let lines: Vec<String> = header
        .lines()
        .map(|s| {
            let trimmed = s.trim();
            if let Some(rest) = trimmed.strip_prefix("* ") {
                handled = true;
                rest.to_string()
            } else if let Some(rest) = trimmed.strip_prefix('*') {
                handled = true;
                rest.to_string()
            } else {
                s.to_string()
            }
        })
        .collect();
    (lines.join("\n"), handled)
}

fn extract_header(file: &File) -> String {
    let Some(comment) = skip_directives(file) else {
        return String::new();
    };
    let list = &comment.list;
    if let Some(first) = list.first() {
        if first.text.starts_with("/*") {
            let single = CommentGroup {
                list: vec![Comment {
                    slash: first.slash,
                    text: first.text.clone(),
                }],
            };
            let mut header = single.text();
            let (handled, ok) = handle_star_block(&header);
            if ok {
                header = handled;
            }
            return header.trim().to_string();
        }
    }
    comment.text().trim().to_string()
}

fn match_header(template: &str, header: &str, values: &HashMap<String, Value>) -> Result<bool, String> {
    let left = "{{";
    let right = "}}";
    let quoted = quote_meta(template, left, right);
    let pattern = execute_template(&quoted, left, right, values)?;
    let exp = Regex::new(&format!("(?s){pattern}"))
        .map_err(|e| format!("goheader template regexp: {e}"))?;
    Ok(exp.is_match(header))
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "goheader requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GoheaderOptions>("goheader")
        .cloned()
        .unwrap_or_default();

    let template = match resolve_template(&opts) {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    if template.is_empty() {
        return Ok(None);
    }

    let values = match build_values(&opts) {
        Ok(v) => v,
        Err(e) => return Err(e),
    };

    let mut pending = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let Some(path) = paths.get(i) else {
            continue;
        };
        if path.extension().and_then(|s| s.to_str()) != Some("go") {
            continue;
        }
        let Some((_fset, parsed)) = reparse(path) else {
            continue;
        };
        let header = extract_header(&parsed);
        let report_pos = parsed
            .comments
            .first()
            .filter(|cg| cg.pos().0 < parsed.package.0)
            .map(|cg| cg.pos().0 as u32)
            .unwrap_or(parsed.package.0 as u32);

        if header.is_empty() {
            pending.push((report_pos, "Missed header for check".to_string()));
            continue;
        }

        match match_header(&template, &header, &values) {
            Ok(true) => {}
            Ok(false) => {
                pending.push((report_pos, "template doesn't match".to_string()));
            }
            Err(e) => return Err(e),
        }
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "goheader",
        doc: "Check if file header matches to pattern",
        url: "https://github.com/denis-tingaikin/go-header",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_adds_dot_and_underscores() {
        assert_eq!(migrate_old_config("A {{ YEAR }} B"), "A {{ .YEAR }} B");
        assert_eq!(
            migrate_old_config("{{ SOME VALUE }}"),
            "{{ .SOME_VALUE }}"
        );
        assert_eq!(migrate_old_config("{{ .YEAR }}"), "{{ .YEAR }}");
    }

    #[test]
    fn quote_meta_preserves_placeholders() {
        let q = quote_meta("A ({{ .YEAR }}) B.", "{{", "}}");
        assert!(q.contains("{{ .YEAR }}"));
        assert!(q.contains(r"A \("));
        assert!(q.contains(r"\) B\."));
    }

    #[test]
    fn match_simple_const_template() {
        let mut opts = GoheaderOptions::default();
        opts.template = "A {{ .YEAR }}\nB".into();
        opts.const_values
            .insert("YEAR".into(), "2020".into());
        let values = build_values(&opts).unwrap();
        let tmpl = resolve_template(&opts).unwrap();
        assert!(match_header(&tmpl, "A 2020\nB", &values).unwrap());
        assert!(!match_header(&tmpl, "A 2019\nB", &values).unwrap());
    }

    #[test]
    fn match_regexp_value() {
        let mut opts = GoheaderOptions::default();
        opts.template = "Copyright {{ .AUTHOR }}".into();
        opts.regexp_values
            .insert("AUTHOR".into(), r".*@example\.com".into());
        let values = build_values(&opts).unwrap();
        let tmpl = resolve_template(&opts).unwrap();
        assert!(match_header(&tmpl, "Copyright alice@example.com", &values).unwrap());
        assert!(!match_header(&tmpl, "Copyright bob@other.com", &values).unwrap());
    }
}
