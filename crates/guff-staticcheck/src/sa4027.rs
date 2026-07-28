//! SA4027 — (*net/url.URL).Query returns a copy
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4027`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff_analysis::code::is_of_type_with_name;


static PAT: OnceLock<Pattern> = OnceLock::new();

fn is_url_type(pass: &Pass<'_>, expr: &guff::ast::Expr) -> bool {
    [
        "net/url.URL",
        "*net/url.URL",
        "url.URL",
        "*url.URL",
    ]
    .into_iter()
    .any(|name| is_of_type_with_name(pass, expr, name))
}

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| must_parse(r#"(CallExpr (SelectorExpr (CallExpr (SelectorExpr recv (Ident "Query")) []) (Ident meth)) _)"#))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4027 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, m| {
        let Some(recv) = m.state.get("recv").and_then(|v| v.as_expr()) else { return true };
        let meth = m.state.get("meth").and_then(|v| v.as_ident()).map(|i| i.name.as_str()).unwrap_or("");
        if !matches!(meth, "Add" | "Del" | "Set") { return true; }
        let _ = recv;
        pending.push((match_pos(node), "(*net/url.URL).Query returns a copy, modifying it doesn't change the URL".into()));
        true
    });
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = call.fun.as_ref() else {
            return;
        };
        if !matches!(sel.name.as_str(), "Add" | "Del" | "Set") {
            return;
        }
        let Expr::CallExpr(qcall) = x.as_ref() else {
            return;
        };
        let Expr::SelectorExpr(SelectorExpr { sel: qsel, .. }) = qcall.fun.as_ref() else {
            return;
        };
        if qsel.name != "Query" {
            return;
        }
        pending.push((
            call.lparen.0 as u32,
            "(*net/url.URL).Query returns a copy, modifying it doesn't change the URL".into(),
        ));
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4027_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4027",
        doc: "(*net/url.URL).Query returns a copy",
        url: "https://staticcheck.dev/docs/checks/#SA4027",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4027_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4027_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
