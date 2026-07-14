//! Port of [`github.com/jgautheron/goconst`](https://github.com/jgautheron/goconst)
//! (golangci-lint defaults).
//!
//! Defaults match golangci-lint: `min-len=3`, `min-occurrences=3`, and
//! exclude string literals in function call arguments (`exclude-types: [call]`).
//!
//! DEFERRED: `match-constant`, `numbers`, `find-duplicates`, `ignore-tests`,
//! `ignore-strings` / `ignore-functions`, and per-linter settings wiring.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

const MIN_LEN: usize = 3;
const MIN_OCCURRENCES: usize = 3;

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

fn add_string(lit: &BasicLit, occurrences: &mut HashMap<String, Vec<u32>>) {
    if lit.kind != Some(Token::STRING) {
        return;
    }
    let Some(s) = unquote_lit(lit) else {
        return;
    };
    if s.chars().count() < MIN_LEN {
        return;
    }
    occurrences
        .entry(s)
        .or_default()
        .push(lit.value_pos.0 as u32);
}

fn add_expr_lit(expr: &Expr, occurrences: &mut HashMap<String, Vec<u32>>) {
    if let Expr::BasicLit(lit) = expr {
        add_string(lit, occurrences);
    }
}

fn collect(pass: &Pass<'_>, occurrences: &mut HashMap<String, Vec<u32>>) {
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                // Skip const declarations entirely (match-constant is DEFERRED).
                NodeRef::GenDecl(g) if g.tok == Some(Token::CONST) => false,
                // golangci default exclude-types: [call] — skip call arguments.
                NodeRef::CallExpr(_) => false,
                NodeRef::AssignStmt(a) => {
                    for rhs in &a.rhs {
                        add_expr_lit(rhs, occurrences);
                    }
                    true
                }
                NodeRef::BinaryExpr(b) if b.op == Token::EQL || b.op == Token::NEQ => {
                    add_expr_lit(&b.x, occurrences);
                    add_expr_lit(&b.y, occurrences);
                    true
                }
                NodeRef::CaseClause(c) => {
                    for item in &c.list {
                        add_expr_lit(item, occurrences);
                    }
                    true
                }
                NodeRef::ReturnStmt(r) => {
                    for item in &r.results {
                        add_expr_lit(item, occurrences);
                    }
                    true
                }
                NodeRef::CompositeLit(cl) => {
                    for elt in &cl.elts {
                        match elt {
                            Expr::BasicLit(lit) => add_string(lit, occurrences),
                            Expr::KeyValueExpr(kv) => {
                                add_expr_lit(&kv.key, occurrences);
                                add_expr_lit(&kv.value, occurrences);
                            }
                            _ => {}
                        }
                    }
                    true
                }
                // VAR specs (CONST GenDecl short-circuits above).
                NodeRef::ValueSpec(vs) => {
                    for val in &vs.values {
                        add_expr_lit(val, occurrences);
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

    let mut occurrences: HashMap<String, Vec<u32>> = HashMap::new();
    collect(pass, &mut occurrences);

    let mut keys: Vec<_> = occurrences.keys().cloned().collect();
    keys.sort();
    for key in keys {
        let positions = occurrences.get(&key).expect("key present");
        let count = positions.len();
        if count < MIN_OCCURRENCES {
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
