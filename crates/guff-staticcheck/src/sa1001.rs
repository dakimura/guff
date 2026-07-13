//! SA1001 — invalid template passed to `text/template` or `html/template`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1001`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::code::{expr_to_string, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| {
        must_parse(
            r#"(CallExpr (Symbol (Or "(*text/template.Template).Parse" "(*html/template.Template).Parse")) [s])"#,
        )
    })
}

fn validate_text_template(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            let start = i;
            i += 2;
            let mut depth = 1usize;
            while i < bytes.len() {
                if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
                    depth += 1;
                    i += 2;
                    continue;
                }
                if i + 1 < bytes.len() && bytes[i] == b'}' && bytes[i + 1] == b'}' {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                    continue;
                }
                i += 1;
            }
            if depth != 0 {
                return Some(format!(
                    "template: {}: unexpected \"}}\" in operand",
                    &s[..start.min(s.len())]
                ));
            }
            continue;
        }
        i += 1;
    }
    None
}

fn parse_from_new(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, .. }) = &*call.fun else {
        return false;
    };
    match &**x {
        Expr::CallExpr(new_call) => is_call_to_any(
            pass,
            new_call,
            &["text/template.New", "html/template.New"],
        ),
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, _m| {
        let NodeRef::CallExpr(call) = node else {
            return true;
        };
        if !parse_from_new(pass, call) {
            return true;
        }
        let Some(s_expr) = call.args.first() else {
            return true;
        };
        let Some(s) = expr_to_string(pass, s_expr) else {
            return true;
        };
        let Some(err) = validate_text_template(&s) else {
            return true;
        };
        if err.contains("unexpected") || err.contains("bad character") {
            let pos = s_expr.pos().0 as u32;
            pending.push((pos, err));
        }
        true
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa1001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1001",
        doc: "invalid template",
        url: "https://staticcheck.dev/docs/checks/#SA1001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1001 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn detects_unclosed_action() {
        assert!(validate_text_template("{{.Name}} {{.LastName").is_some());
        assert!(validate_text_template("{{.Name}}").is_none());
    }
}
