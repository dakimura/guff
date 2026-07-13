//! S1035 — redundant call to net/http.CanonicalHeaderKey in Header method.
//!
//! Port of `honnef.co/go/tools/simple/s1035`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

const HEADER_METHODS: &[&str] = &["Add", "Del", "Get", "Set"];

fn is_canonical_header_key_arg(arg: &Expr) -> bool {
    let Expr::CallExpr(call) = arg else {
        return false;
    };
    matches!(
        &*call.fun,
        Expr::SelectorExpr(SelectorExpr { sel, .. }) if sel.name == "CanonicalHeaderKey"
    )
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1035 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Expr::SelectorExpr(SelectorExpr { sel, .. }) = &*call.fun else {
            return;
        };
        if !HEADER_METHODS.contains(&sel.name.as_str()) {
            return;
        }
        if call.args.first().is_some_and(is_canonical_header_key_arg) {
            pending.push((
                match_pos(node),
                "calling net/http.CanonicalHeaderKey on the key argument is redundant".into(),
            ));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1035_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1035",
        doc: "redundant call to net/http.CanonicalHeaderKey in Header method",
        url: "https://staticcheck.dev/docs/checks/#S1035",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1035 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1035_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1035_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
