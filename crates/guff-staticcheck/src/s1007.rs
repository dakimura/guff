//! S1007 — simplify regular expression by using raw string literal.
//!
//! Port of `honnef.co/go/tools/simple/s1007`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

/// Upstream inspects the quoted source literal (`lit.Value`), not the
/// decoded string: require a `\\` pair and reject any other escape
/// (`\` followed by a non-`\`), so e.g. `"...\n..."` is not flagged.
fn should_use_raw_string_src(quoted: &str) -> bool {
    if !quoted.starts_with('"') || quoted.contains('`') {
        return false;
    }
    if !quoted.contains(r"\\") {
        return false;
    }
    let mut bs = false;
    for c in quoted.chars() {
        if !bs && c == '\\' {
            bs = true;
            continue;
        }
        if bs && c == '\\' {
            bs = false;
            continue;
        }
        if bs {
            // backslash followed by non-backslash → escape sequence
            return false;
        }
    }
    true
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1007 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if !is_call_to_any(pass, call, &["regexp.Compile", "regexp.MustCompile"])
            || call.args.len() != 1
        {
            return;
        }
        // Upstream interpolates the symbol it matched, so the message names
        // whichever of the two was called.
        let Some(callee) = call_name(pass, &call.fun) else {
            return;
        };
        let Expr::BasicLit(lit) = &call.args[0] else {
            return;
        };
        if lit.kind != Some(Token::STRING) || lit.value.starts_with('`') {
            return;
        }
        if !should_use_raw_string_src(&lit.value) {
            return;
        }
        pending.push((lit.value_pos.0 as u32, callee));
    });
    for (pos, callee) in pending {
        pass.report_unless_generated(
            pos,
            &format!(
                "should use raw string (`...`) with {callee} to avoid having to escape twice"
            ),
        );
    }
    Ok(None)
}

fn s1007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1007",
        doc: "simplify regular expression by using raw string literal",
        url: "https://staticcheck.dev/docs/checks/#S1007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn raw_string_heuristic_matches_upstream() {
        assert!(should_use_raw_string_src(r#""\\A\\w+""#));
        assert!(should_use_raw_string_src(r#""\\d+""#));
        // `\n` is a non-`\\` escape → do not suggest raw string
        assert!(!should_use_raw_string_src(
            r#""   ([a-zA-Z0-9\\-]{36}) - ([^\n]+)""#
        ));
        assert!(!should_use_raw_string_src(r#""foo\nbar""#));
        assert!(!should_use_raw_string_src(r#""no doubles""#));
        assert!(!should_use_raw_string_src(r#"`already raw\\d`"#));
    }
}
