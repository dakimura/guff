//! Port of [`github.com/xen0n/gosmopolitan`](https://github.com/xen0n/gosmopolitan)
//! (golangci-lint wrapper in `pkg/golinters/gosmopolitan`).
//!
//! Reports certain i18n/l10n anti-patterns:
//! - string literals containing runes of a watched Unicode script
//!   (default `Han`), and
//! - usages of `time.Local` (unless `allow-time-local`).
//!
//! Test files (`*_test.go`) are skipped, matching upstream's default
//! `LookAtTests: false` (golangci-lint does not expose a knob for it).
//!
//! DEFERRED (see DEVELOPMENT.md R13/R14): dot-imported `Local` (the
//! `time.Local` check only matches the `time.Local` selector form).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::{preorder, preorder_stack, NodeRef};
use guff_analysis::code::{object_of, selector_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::GosmopolitanOptions;

/// Strip the legacy `(pkg).name` parenthesised FQN form to `pkg.name`.
fn unquote_fqn(x: &str) -> String {
    if !x.starts_with('(') {
        return x.to_string();
    }
    let rest = &x[1..];
    match rest.split_once(')') {
        Some((before, after)) => format!("{before}{after}"),
        None => rest.to_string(),
    }
}

/// Compiles one regexp per watched script, skipping invalid script names
/// (upstream errors out; we ignore unknown scripts to stay non-fatal).
fn compile_scripts(scripts: &[String]) -> Vec<(String, Regex)> {
    scripts
        .iter()
        .filter_map(|s| Regex::new(&format!(r"\p{{{s}}}")).ok().map(|re| (s.clone(), re)))
        .collect()
}

/// Returns the watched-script name and byte offset of the earliest matching
/// rune in `value` (the raw literal text, including quotes).
fn first_script_match(scripts: &[(String, Regex)], value: &str) -> Option<(String, usize)> {
    let mut best: Option<(String, usize)> = None;
    for (name, re) in scripts {
        if let Some(m) = re.find(value) {
            let start = m.start();
            if best.as_ref().map(|(_, b)| start < *b).unwrap_or(true) {
                best = Some((name.clone(), start));
            }
        }
    }
    best
}

/// Fully-qualified name (`pkg/path.Name`) of the referent of a call/composite
/// type expression, used for escape-hatch matching.
fn referent_fqn(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let ident = match e {
        Expr::Ident(id) => id,
        Expr::SelectorExpr(sel) => &sel.sel,
        _ => return None,
    };
    let obj = object_of(pass, ident)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match obj.pkg(&artifacts.objects) {
        Some(p) => Some(format!("{}.{}", artifacts.packages.get(p).path(), ident.name)),
        None => Some(ident.name.clone()),
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gosmopolitan requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GosmopolitanOptions>("gosmopolitan")
        .cloned()
        .unwrap_or_default();

    let scripts = if opts.watch_for_scripts.is_empty() {
        vec!["Han".to_string()]
    } else {
        opts.watch_for_scripts.clone()
    };
    let script_res = compile_scripts(&scripts);
    if script_res.is_empty() {
        return Ok(None);
    }

    let escape: HashSet<String> = opts
        .escape_hatches
        .iter()
        .map(|s| unquote_fqn(s))
        .filter(|s| !s.is_empty())
        .collect();

    let pkg = pass.pkg();
    let fset = pass.fset();
    let mut pending: Vec<(u32, String)> = Vec::new();

    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or(fallback.as_str());
        if filename.ends_with("_test.go") {
            continue;
        }

        // Pass 1: string literals containing watched-script runes.
        let mut stack = Vec::new();
        preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
            match n {
                // Import blocks and type declarations can't hold interesting
                // runtime strings — skip their subtrees (upstream parity).
                NodeRef::ImportSpec(_) | NodeRef::TypeSpec(_) => return false,
                NodeRef::CallExpr(c) if !escape.is_empty() => {
                    if let Some(fqn) = referent_fqn(pass, &c.fun) {
                        if escape.contains(&fqn) {
                            return false;
                        }
                    }
                }
                NodeRef::CompositeLit(c) if !escape.is_empty() => {
                    if let Some(ty) = &c.ty {
                        if let Some(fqn) = referent_fqn(pass, ty) {
                            if escape.contains(&fqn) {
                                return false;
                            }
                        }
                    }
                }
                NodeRef::BasicLit(lit) if lit.kind == Some(Token::STRING) => {
                    if let Some((script, offset)) = first_script_match(&script_res, &lit.value) {
                        pending.push((
                            (lit.value_pos.0 + offset as i64) as u32,
                            format!("string literal contains rune in {script} script"),
                        ));
                    }
                }
                _ => {}
            }
            true
        });

        // Pass 2: time.Local usages.
        if !opts.allow_time_local {
            preorder(NodeRef::File(file), |n| {
                if let NodeRef::SelectorExpr(sel) = n {
                    if sel.sel.name == "Local"
                        && selector_name(pass, sel).as_deref() == Some("time.Local")
                    {
                        pending.push((sel.x.pos().0 as u32, "usage of time.Local".to_string()));
                    }
                }
                true
            });
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gosmopolitan",
        doc: "Report certain i18n/l10n anti-patterns in your Go codebase.",
        url: "https://github.com/xen0n/gosmopolitan",
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
    fn han_regexp_matches_chinese_runes() {
        let scripts = compile_scripts(&["Han".to_string()]);
        assert!(first_script_match(&scripts, "\"你好\"").is_some());
        assert!(first_script_match(&scripts, "\"hello\"").is_none());
    }

    #[test]
    fn unquote_fqn_strips_parens() {
        assert_eq!(unquote_fqn("(foo/bar).Baz"), "foo/bar.Baz");
        assert_eq!(unquote_fqn("foo/bar.Baz"), "foo/bar.Baz");
    }
}
