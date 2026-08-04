//! Port of [`github.com/jgautheron/goconst`](https://github.com/jgautheron/goconst)
//! (golangci-lint defaults).
//!
//! Defaults match golangci-lint: `min-len=3`, `min-occurrences=3`,
//! `match-constant=true`, `find-duplicates=false`, `ignore-calls=true`,
//! `numbers=false`, `min=3`, `max=3`.
//!
//! DEFERRED: `ignore-strings` / `ignore-functions`, `eval-const-expressions`,
//! and remaining settings keys.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, GenDecl, Spec};
use guff::position::Pos;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GoconstOptions;

#[derive(Clone)]
struct ConstEntry {
    name: String,
    filename: String,
    pos: u32,
}

fn unquote_lit(lit: &BasicLit) -> Option<String> {
    let v = &lit.value;
    if v.len() < 2 {
        return None;
    }
    let quote = v.as_bytes()[0];
    if (quote != b'"' && quote != b'`') || v.as_bytes()[v.len() - 1] != quote {
        return None;
    }
    if quote == b'`' {
        return Some(v[1..v.len() - 1].to_string());
    }
    let mut out = String::with_capacity(v.len());
    let mut chars = v[1..v.len() - 1].chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn literal_key(lit: &BasicLit) -> Option<String> {
    match lit.kind {
        Some(Token::STRING) => unquote_lit(lit),
        Some(Token::INT) | Some(Token::FLOAT) => Some(lit.value.clone()),
        _ => None,
    }
}

fn is_supported_lit(lit: &BasicLit, numbers: bool) -> bool {
    match lit.kind {
        Some(Token::STRING) => true,
        Some(Token::INT) | Some(Token::FLOAT) => numbers,
        _ => false,
    }
}

fn passes_min_len(value: &str, min_len: usize) -> bool {
    !value.is_empty() && value.chars().count() >= min_len
}

fn parse_go_int(s: &str) -> Option<i64> {
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        return i64::from_str_radix(rest, 16).ok();
    }
    if s.starts_with('0') && s.len() > 1 && !s.contains(['.', 'e', 'E']) {
        return i64::from_str_radix(s, 8).ok();
    }
    s.parse::<i64>().ok()
}

fn passes_number_range(value: &str, options: GoconstOptions) -> bool {
    if !options.numbers {
        return true;
    }
    if options.number_min == 0 && options.number_max == 0 {
        return true;
    }
    let Some(i) = parse_go_int(value) else {
        return true;
    };
    if options.number_min != 0 && i < options.number_min {
        return false;
    }
    if options.number_max != 0 && i > options.number_max {
        return false;
    }
    true
}

fn add_literal(
    lit: &BasicLit,
    options: GoconstOptions,
    occurrences: &mut HashMap<String, Vec<u32>>,
) {
    if !is_supported_lit(lit, options.numbers) {
        return;
    }
    let Some(key) = literal_key(lit) else {
        return;
    };
    if !passes_min_len(&key, options.min_len) {
        return;
    }
    if !passes_number_range(&key, options) {
        return;
    }
    occurrences
        .entry(key)
        .or_default()
        .push(lit.value_pos.0 as u32);
}

fn add_expr_lit(
    expr: &Expr,
    options: GoconstOptions,
    occurrences: &mut HashMap<String, Vec<u32>>,
) {
    if let Expr::BasicLit(lit) = expr {
        add_literal(lit, options, occurrences);
    }
}

fn collect_constants_from_gendecl(
    g: &GenDecl,
    filename: &str,
    options: GoconstOptions,
    constants: &mut HashMap<String, Vec<ConstEntry>>,
) {
    for spec in &g.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        if vs.values.is_empty() {
            continue;
        }
        for (i, name) in vs.names.iter().enumerate() {
            let value_idx = if vs.values.len() == 1 {
                0
            } else if i < vs.values.len() {
                i
            } else {
                continue;
            };
            let Expr::BasicLit(lit) = &vs.values[value_idx] else {
                continue;
            };
            if !is_supported_lit(lit, options.numbers) {
                continue;
            }
            let Some(key) = literal_key(lit) else {
                continue;
            };
            if !passes_min_len(&key, options.min_len) {
                continue;
            }
            constants.entry(key).or_default().push(ConstEntry {
                name: name.name.clone(),
                filename: filename.to_string(),
                pos: name.name_pos.0 as u32,
            });
        }
    }
}

fn find_matching_const(
    key: &str,
    filename: &str,
    constants: &HashMap<String, Vec<ConstEntry>>,
) -> Option<String> {
    let entries = constants.get(key)?;
    let mut sorted = entries.clone();
    sorted.sort_by(|a, b| (a.filename.as_str(), a.pos).cmp(&(b.filename.as_str(), b.pos)));

    let is_test = filename.ends_with("_test.go");
    if !is_test {
        if let Some(entry) = sorted
            .iter()
            .find(|entry| !entry.filename.ends_with("_test.go"))
        {
            return Some(entry.name.clone());
        }
    }
    sorted.first().map(|entry| entry.name.clone())
}

fn collect(
    pass: &Pass<'_>,
    options: GoconstOptions,
    occurrences: &mut HashMap<String, Vec<u32>>,
    constants: &mut HashMap<String, Vec<ConstEntry>>,
) {
    let pkg = pass.pkg();
    let fset = pass.fset();
    let collect_consts = options.match_constant || options.find_duplicates;
    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or(fallback.as_str());
        if options.ignore_tests && filename.ends_with("_test.go") {
            continue;
        }

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::GenDecl(g) if g.tok == Some(Token::CONST) => {
                    if collect_consts {
                        collect_constants_from_gendecl(g, filename, options, constants);
                    }
                    false
                }
                // Upstream goconst walks into CallExpr children so nested
                // CompositeLit (e.g. f([]string{"x"})) still counts. golangci
                // `ignore-calls` only excludes *direct* BasicLit call args
                // (excludeTypes[Call]), matching jgautheron/goconst.
                NodeRef::CallExpr(c) => {
                    if !options.ignore_calls {
                        for arg in &c.args {
                            add_expr_lit(arg, options, occurrences);
                        }
                    }
                    true
                }
                NodeRef::AssignStmt(a) => {
                    for rhs in &a.rhs {
                        add_expr_lit(rhs, options, occurrences);
                    }
                    true
                }
                NodeRef::BinaryExpr(b) if b.op == Token::EQL || b.op == Token::NEQ => {
                    add_expr_lit(&b.x, options, occurrences);
                    add_expr_lit(&b.y, options, occurrences);
                    true
                }
                NodeRef::CaseClause(c) => {
                    for item in &c.list {
                        add_expr_lit(item, options, occurrences);
                    }
                    true
                }
                NodeRef::ReturnStmt(r) => {
                    for item in &r.results {
                        add_expr_lit(item, options, occurrences);
                    }
                    true
                }
                NodeRef::CompositeLit(cl) => {
                    for elt in &cl.elts {
                        match elt {
                            Expr::BasicLit(lit) => add_literal(lit, options, occurrences),
                            Expr::KeyValueExpr(kv) => {
                                add_expr_lit(&kv.key, options, occurrences);
                                add_expr_lit(&kv.value, options, occurrences);
                            }
                            _ => {}
                        }
                    }
                    true
                }
                NodeRef::ValueSpec(vs) => {
                    for val in &vs.values {
                        add_expr_lit(val, options, occurrences);
                    }
                    true
                }
                _ => true,
            }
        });
    }
}

fn format_message(key: &str, count: usize, matching_const: Option<&str>) -> String {
    if let Some(name) = matching_const {
        format!(
            "string `{key}` has {count} occurrences, but such constant `{name}` already exists"
        )
    } else {
        format!("string `{key}` has {count} occurrences, make it a constant")
    }
}

fn format_duplicate_message(name: &str, first_pos: &guff::position::Position) -> String {
    format!("This constant is a duplicate of `{name}` at {first_pos}")
}

fn report_duplicate_consts(
    pass: &mut Pass<'_>,
    constants: &HashMap<String, Vec<ConstEntry>>,
) {
    let mut keys: Vec<_> = constants.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let entries = constants.get(&key).expect("key present");
        if entries.len() < 2 {
            continue;
        }

        let mut non_test: Vec<_> = entries
            .iter()
            .filter(|e| !e.filename.ends_with("_test.go"))
            .cloned()
            .collect();
        let mut test: Vec<_> = entries
            .iter()
            .filter(|e| e.filename.ends_with("_test.go"))
            .cloned()
            .collect();

        for scope in [&mut non_test, &mut test] {
            scope.sort_by(|a, b| (a.filename.as_str(), a.pos).cmp(&(b.filename.as_str(), b.pos)));
            if scope.len() < 2 {
                continue;
            }
            let first = &scope[0];
            let first_pos = pass.fset().position(Pos(first.pos as i64));
            for dup in &scope[1..] {
                pass.reportf(
                    dup.pos,
                    &format_duplicate_message(&first.name, &first_pos),
                );
            }
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "goconst requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GoconstOptions>("goconst")
        .copied()
        .unwrap_or_default();

    let mut occurrences: HashMap<String, Vec<u32>> = HashMap::new();
    let mut constants: HashMap<String, Vec<ConstEntry>> = HashMap::new();
    collect(pass, options, &mut occurrences, &mut constants);

    let mut keys: Vec<_> = occurrences.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let positions = occurrences.get(&key).expect("key present");
        let count = positions.len();
        if count < options.min_occurrences {
            continue;
        }
        // golangci/goconst reports the first occurrence in each file that
        // contains the duplicated literal (package-level count, per-file
        // diagnostics). Match that so finding-set keys align.
        let mut first_per_file: HashMap<String, u32> = HashMap::new();
        for &pos in positions {
            let filename = pass.fset().position(Pos(pos as i64)).filename;
            first_per_file
                .entry(filename)
                .and_modify(|p| *p = (*p).min(pos))
                .or_insert(pos);
        }
        let mut report_positions: Vec<_> = first_per_file.into_values().collect();
        report_positions.sort_unstable();
        for pos in report_positions {
            let filename = pass.fset().position(Pos(pos as i64)).filename;
            let matching = if options.match_constant {
                find_matching_const(&key, &filename, &constants)
            } else {
                None
            };
            pass.reportf(pos, &format_message(&key, count, matching.as_deref()));
        }
    }

    if options.find_duplicates {
        report_duplicate_consts(pass, &constants);
    }

    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "goconst",
        doc: "Finds repeated strings that could be replaced by a constant",
        url: "https://github.com/jgautheron/goconst",
        run: run as RunFn,
        // AST-only (like upstream jgautheron/goconst). Still useful on
        // packages guff typechecks imperfectly — cobra OSS hunt regression.
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
