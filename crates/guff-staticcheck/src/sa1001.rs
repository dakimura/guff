//! SA1001 — invalid template passed to `text/template` or `html/template`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1001`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::code::{expr_to_string, is_call_to_any};
use guff_analysis::passes::{inspect, typeindex};
use guff_analysis::{matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};

use crate::gostd;

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| {
        must_parse(
            r#"(CallExpr (Symbol (Or "(*text/template.Template).Parse" "(*html/template.Template).Parse")) [s])"#,
        )
    })
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

    let _ = pass
        .result_of::<typeindex::Index>(typeindex::analyzer())
        .ok_or_else(|| "SA1001 requires typeindex analyzer".to_string())?;

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
        // Upstream calls text/template (or html/template, which returns the
        // same errors) and prints err.Error() verbatim, whitelisting only these
        // two classes; gostd::template is the port of that parser.
        let Err(err) = gostd::template::parse(&s) else {
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
        requires: vec![inspect::analyzer(), typeindex::analyzer()],
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

    /// The parser itself is gated by `tests/gostd_template.rs` against Go;
    /// this only pins the wiring — which errors reach a report.
    #[test]
    fn reports_only_the_whitelisted_error_classes() {
        let err = gostd::template::parse("{{.Name}} {{.LastName}").unwrap_err();
        assert_eq!(err, "template: :1: bad character U+007D '}'");
        assert!(gostd::template::parse("{{.Name}}").is_ok());
        // An error outside the two classes: upstream stays silent on it.
        let quiet = gostd::template::parse("{{undefined}}").unwrap_err();
        assert!(!quiet.contains("unexpected") && !quiet.contains("bad character"));
    }
}
