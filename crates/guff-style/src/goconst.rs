//! Port of [`github.com/jgautheron/goconst`](https://github.com/jgautheron/goconst)
//! (golangci-lint defaults).
//!
//! Defaults match golangci-lint: `min-len=3`, `min-occurrences=3`, and
//! exclude string literals in function call arguments (`ignore-calls: true`).
//!
//! DEFERRED: `match-constant`, `numbers`, `find-duplicates`, `ignore-strings` /
//! `ignore-functions`, and remaining settings keys.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GoconstOptions;

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

fn add_string(lit: &BasicLit, min_len: usize, occurrences: &mut HashMap<String, Vec<u32>>) {
    if lit.kind != Some(Token::STRING) {
        return;
    }
    let Some(s) = unquote_lit(lit) else {
        return;
    };
    if s.chars().count() < min_len {
        return;
    }
    occurrences
        .entry(s)
        .or_default()
        .push(lit.value_pos.0 as u32);
}

fn add_expr_lit(
    expr: &Expr,
    min_len: usize,
    occurrences: &mut HashMap<String, Vec<u32>>,
) {
    if let Expr::BasicLit(lit) = expr {
        add_string(lit, min_len, occurrences);
    }
}

fn collect(pass: &Pass<'_>, options: GoconstOptions, occurrences: &mut HashMap<String, Vec<u32>>) {
    let pkg = pass.pkg();
    let fset = pass.fset();
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
                // Skip const declarations entirely (match-constant is DEFERRED).
                NodeRef::GenDecl(g) if g.tok == Some(Token::CONST) => false,
                NodeRef::CallExpr(_) if options.ignore_calls => false,
                NodeRef::AssignStmt(a) => {
                    for rhs in &a.rhs {
                        add_expr_lit(rhs, options.min_len, occurrences);
                    }
                    true
                }
                NodeRef::BinaryExpr(b) if b.op == Token::EQL || b.op == Token::NEQ => {
                    add_expr_lit(&b.x, options.min_len, occurrences);
                    add_expr_lit(&b.y, options.min_len, occurrences);
                    true
                }
                NodeRef::CaseClause(c) => {
                    for item in &c.list {
                        add_expr_lit(item, options.min_len, occurrences);
                    }
                    true
                }
                NodeRef::ReturnStmt(r) => {
                    for item in &r.results {
                        add_expr_lit(item, options.min_len, occurrences);
                    }
                    true
                }
                NodeRef::CompositeLit(cl) => {
                    for elt in &cl.elts {
                        match elt {
                            Expr::BasicLit(lit) => add_string(lit, options.min_len, occurrences),
                            Expr::KeyValueExpr(kv) => {
                                add_expr_lit(&kv.key, options.min_len, occurrences);
                                add_expr_lit(&kv.value, options.min_len, occurrences);
                            }
                            _ => {}
                        }
                    }
                    true
                }
                NodeRef::ValueSpec(vs) => {
                    for val in &vs.values {
                        add_expr_lit(val, options.min_len, occurrences);
                    }
                    true
                }
                _ => true,
            }
        });
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
    collect(pass, options, &mut occurrences);

    let mut keys: Vec<_> = occurrences.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let positions = occurrences.get(&key).expect("key present");
        let count = positions.len();
        if count < options.min_occurrences {
            continue;
        }
        let pos = *positions.iter().min().unwrap_or(&0);
        // golangci FormatCode wraps identifiers with backticks for display.
        pass.reportf(
            pos,
            &format!("string `{key}` has {count} occurrences, make it a constant"),
        );
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
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
