//! Port of [`github.com/MirrexOne/unqueryvet`](https://github.com/MirrexOne/unqueryvet)
//! (golangci-lint wrapper in `pkg/golinters/unqueryvet`).
//!
//! Detects `SELECT *` in SQL string literals (assignments, const/var decls, call
//! arguments). Default `allowed-patterns` skip `COUNT(*)` / system catalogs.
//!
//! DEFERRED: SQL builders, format strings, string concat / `strings.Builder`,
//! N+1 / SQL-injection / tx-leak detectors, custom DSL rules, ignored-functions /
//! ignored-files.

use std::sync::OnceLock;

use guff::ast::{Expr, GenDecl, Spec};
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::UnqueryvetOptions;

const MESSAGE: &str = "avoid SELECT * - explicitly specify needed columns for better performance, maintainability and stability";

const ALIASED_MESSAGE: &str =
    "avoid SELECT alias.* - explicitly specify columns like alias.id, alias.name for better maintainability";

fn default_allowed_patterns() -> Vec<&'static str> {
    vec![
        r"(?i)COUNT\(\s*\*\s*\)",
        r"(?i)MAX\(\s*\*\s*\)",
        r"(?i)MIN\(\s*\*\s*\)",
        r"(?i)SELECT \* FROM information_schema\..*",
        r"(?i)SELECT \* FROM pg_catalog\..*",
        r"(?i)SELECT \* FROM sys\..*",
    ]
}

fn aliased_wildcard_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)SELECT\s+(?:[A-Za-z_][A-Za-z0-9_]*\s*\.\s*\*\s*,?\s*)+")
            .expect("aliased wildcard")
    })
}

fn subquery_select_star_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\(\s*SELECT\s+\*").expect("subquery select star"))
}

fn whitespace_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\s+").expect("whitespace"))
}

fn unquote_string_lit(value: &str) -> Option<String> {
    let v = value.trim();
    if v.len() < 2 {
        return None;
    }
    let first = v.as_bytes()[0];
    let last = v.as_bytes()[v.len() - 1];
    if (first == b'"' && last == b'"') || (first == b'`' && last == b'`') {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

/// Upstream `normalizeSQLQuery`: strip quotes, drop `--` comments, collapse
/// whitespace, uppercase.
fn normalize_sql_query(raw: &str) -> String {
    let Some(query) = unquote_string_lit(raw) else {
        return String::new();
    };

    let mut parts: Vec<&str> = Vec::new();
    for line in query.lines() {
        let line = match line.find("--") {
            Some(idx) => &line[..idx],
            None => line,
        };
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            parts.push(trimmed);
        }
    }
    let joined = parts.join(" ");
    let upper = joined.to_uppercase().replace('\t', " ");
    whitespace_re().replace_all(&upper, " ").trim().to_string()
}

fn matches_allowed(query: &str, patterns: &[String]) -> bool {
    for p in patterns {
        if let Ok(re) = Regex::new(p) {
            if re.is_match(query) {
                return true;
            }
        }
    }
    false
}

fn effective_allowed(opts: &UnqueryvetOptions) -> Vec<String> {
    if opts.allowed_patterns.is_empty() {
        default_allowed_patterns()
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        opts.allowed_patterns.clone()
    }
}

enum Hit {
    SelectStar,
    Aliased,
}

fn classify_select_star(query: &str, opts: &UnqueryvetOptions) -> Option<Hit> {
    let allowed = effective_allowed(opts);
    if matches_allowed(query, &allowed) {
        return None;
    }

    let upper = query.to_uppercase();
    if upper.contains("SELECT *") {
        const KEYWORDS: &[&str] = &[
            "FROM", "WHERE", "JOIN", "GROUP", "ORDER", "HAVING", "UNION", "LIMIT",
        ];
        if KEYWORDS.iter().any(|k| upper.contains(k)) {
            return Some(Hit::SelectStar);
        }
        if upper.trim() == "SELECT *" {
            return Some(Hit::SelectStar);
        }
    }

    if opts.check_aliased_wildcard && aliased_wildcard_re().is_match(query) {
        return Some(Hit::Aliased);
    }

    if opts.check_subqueries && subquery_select_star_re().is_match(query) {
        return Some(Hit::SelectStar);
    }

    None
}

fn check_string_lit(lit_value: &str, lit_pos: u32, opts: &UnqueryvetOptions, pending: &mut Vec<(u32, String)>) {
    let normalized = normalize_sql_query(lit_value);
    if normalized.is_empty() {
        return;
    }
    match classify_select_star(&normalized, opts) {
        Some(Hit::SelectStar) => pending.push((lit_pos, MESSAGE.to_string())),
        Some(Hit::Aliased) => pending.push((lit_pos, ALIASED_MESSAGE.to_string())),
        None => {}
    }
}

fn check_expr(expr: &Expr, opts: &UnqueryvetOptions, pending: &mut Vec<(u32, String)>) {
    if let Expr::BasicLit(lit) = expr {
        if lit.kind == Some(Token::STRING) {
            check_string_lit(&lit.value, lit.value_pos.0 as u32, opts, pending);
        }
    }
}

fn check_gen_decl(gd: &GenDecl, opts: &UnqueryvetOptions, pending: &mut Vec<(u32, String)>) {
    if gd.tok != Some(Token::CONST) && gd.tok != Some(Token::VAR) {
        return;
    }
    for spec in &gd.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        for value in &vs.values {
            check_expr(value, opts, pending);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "unqueryvet requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<UnqueryvetOptions>("unqueryvet")
        .cloned()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::AssignStmt(assign) => {
                    for rhs in &assign.rhs {
                        check_expr(rhs, &opts, &mut pending);
                    }
                }
                NodeRef::GenDecl(gd) => check_gen_decl(gd, &opts, &mut pending),
                NodeRef::CallExpr(call) => {
                    for arg in &call.args {
                        check_expr(arg, &opts, &mut pending);
                    }
                }
                _ => {}
            }
            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unqueryvet",
        doc: "Detects SELECT * in SQL queries and encourages explicit column selection.",
        url: "https://github.com/MirrexOne/unqueryvet",
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
    fn count_star_is_allowed() {
        let opts = UnqueryvetOptions::default();
        let q = normalize_sql_query(r#""SELECT COUNT(*) FROM users""#);
        assert!(classify_select_star(&q, &opts).is_none());
    }

    #[test]
    fn select_star_from_is_flagged() {
        let opts = UnqueryvetOptions::default();
        let q = normalize_sql_query(r#""SELECT * FROM users""#);
        assert!(matches!(classify_select_star(&q, &opts), Some(Hit::SelectStar)));
    }

    #[test]
    fn information_schema_is_allowed() {
        let opts = UnqueryvetOptions::default();
        let q = normalize_sql_query(r#""SELECT * FROM information_schema.tables""#);
        assert!(classify_select_star(&q, &opts).is_none());
    }

    #[test]
    fn aliased_wildcard_is_flagged() {
        let opts = UnqueryvetOptions::default();
        let q = normalize_sql_query(r#""SELECT t.* FROM users t""#);
        assert!(matches!(classify_select_star(&q, &opts), Some(Hit::Aliased)));
    }

    #[test]
    fn aliased_can_be_disabled() {
        let opts = UnqueryvetOptions {
            check_aliased_wildcard: false,
            ..UnqueryvetOptions::default()
        };
        let q = normalize_sql_query(r#""SELECT t.* FROM users t""#);
        assert!(classify_select_star(&q, &opts).is_none());
    }
}
