//! S1035 — redundant call to net/http.CanonicalHeaderKey in Header method.
//!
//! Port of `honnef.co/go/tools/simple/s1035`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::unparen;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

const HEADER_METHODS: &[&str] = &["Add", "Del", "Get", "Set"];

fn is_canonical_header_key_arg(arg: &Expr) -> bool {
    let Expr::CallExpr(call) = unparen(arg) else {
        return false;
    };
    matches!(
        unparen(&call.fun),
        Expr::SelectorExpr(SelectorExpr { sel, .. }) if sel.name == "CanonicalHeaderKey"
    )
}

/// Fully-qualified type of a method call's receiver expression
/// (`net/http.Header`), as upstream prints it inside `(…)`.
fn recv_type_string(pass: &Pass<'_>, x: &Expr) -> Option<String> {
    let typ = pass.types_info()?.types.get(&x.id())?.typ;
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types, &a.objects, &a.packages, typ, None,
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1035 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(&call.fun) else {
            return;
        };
        if !HEADER_METHODS.contains(&sel.name.as_str()) {
            return;
        }
        let Some(arg) = call.args.first() else {
            return;
        };
        if !is_canonical_header_key_arg(arg) {
            return;
        }
        // Upstream quotes the parameter name and names the method it belongs
        // to — `on the 'key' argument of (net/http.Header).Set` — and reports
        // the redundant argument, not the enclosing call. Verified against
        // golangci-lint 2.12.2 for Add / Get / Set, including
        // `r.Header.Get(...)`.
        let Some(recv) = recv_type_string(pass, x) else {
            return;
        };
        pending.push((
            // The pattern binds the *unparenthesized* node, so upstream's
            // report starts inside the parens: `h.Set((canon(k)), v)` is
            // reported at `canon`, one column in from `(`.
            unparen(arg).pos().0 as u32,
            format!(
                "calling net/http.CanonicalHeaderKey on the 'key' argument of ({recv}).{} is redundant",
                sel.name
            ),
        ));
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
