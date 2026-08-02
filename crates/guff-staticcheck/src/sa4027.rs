//! SA4027 — (*net/url.URL).Query returns a copy
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4027`.

use std::sync::OnceLock;

use guff::walk::NodeRef;
use guff_analysis::code::is_of_pointer_to_type_with_name_id;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, matches, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_pattern::{must_parse, MatchValue, Pattern};

static PAT: OnceLock<Pattern> = OnceLock::new();

fn pat() -> &'static Pattern {
    PAT.get_or_init(|| {
        must_parse(
            r#"(CallExpr (SelectorExpr (CallExpr (SelectorExpr recv (Ident "Query")) []) (Ident meth)) args)"#,
        )
    })
}

fn match_node_id(v: &MatchValue<'_>) -> Option<u32> {
    match v {
        MatchValue::Expr(e) => Some(e.id()),
        MatchValue::Ident(i) => Some(i.id),
        MatchValue::Node(NodeRef::Ident(i)) => Some(i.id),
        MatchValue::Node(NodeRef::SelectorExpr(s)) => Some(s.id),
        MatchValue::Node(NodeRef::CallExpr(c)) => Some(c.id),
        MatchValue::Node(NodeRef::ParenExpr(p)) => Some(p.id),
        MatchValue::Node(NodeRef::StarExpr(s)) => Some(s.id),
        _ => None,
    }
}

fn meth_name<'a>(v: &'a MatchValue<'_>) -> Option<&'a str> {
    match v {
        MatchValue::Ident(i) => Some(i.name.as_str()),
        MatchValue::Node(NodeRef::Ident(i)) => Some(i.name.as_str()),
        MatchValue::String(s) => Some(s.as_str()),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4027 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    matches(pass, &inspect, pat(), |node, m| {
        let Some(recv) = m.state.get("recv") else {
            return true;
        };
        let Some(recv_id) = match_node_id(recv) else {
            return true;
        };
        // Upstream: IsOfPointerToTypeWithName(..., "net/url.URL") — value-typed
        // `url.URL` variables (vault mssqlhelper) are intentionally not flagged.
        if !is_of_pointer_to_type_with_name_id(pass, recv_id, "net/url.URL") {
            return true;
        }
        let meth = m
            .state
            .get("meth")
            .and_then(meth_name)
            .unwrap_or("");
        if !matches!(meth, "Add" | "Del" | "Set") {
            return true;
        }
        pending.push((
            match_pos(node),
            "(*net/url.URL).Query returns a copy, modifying it doesn't change the URL".into(),
        ));
        true
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
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
