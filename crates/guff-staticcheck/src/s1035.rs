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
use guff_analysis::code::{self, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_node;

/// The four methods upstream's pattern names, as `(Symbol …)` spells them.
///
/// Upstream matches the **callee object**, not the selector's spelling:
///
/// ```text
/// (Symbol callName@(Or
///     "(net/http.Header).Add" "(net/http.Header).Del"
///     "(net/http.Header).Get" "(net/http.Header).Set"))
/// ```
///
/// Matching on the name `Set` alone is a different check. minio's
/// `cmd/postpolicyform_test.go` has
///
/// ```ignore
/// type formValues struct{ http.Header }
/// func (f formValues) Set(key, value string) formValues { … }
/// ```
///
/// — an embedded `http.Header` whose `Set` is **shadowed** by the outer type's
/// own method. The call is `(cmd.formValues).Set`, upstream says nothing, and
/// guff reported it seven times. A type with a `Set` method and no relation to
/// `net/http` at all was reported too.
///
/// The same object is what the message names. For a **promoted** method the
/// callee really is `net/http.Header`'s, and upstream prints
/// `(net/http.Header).Del` where guff printed the receiver expression's own
/// type — the right finding under the wrong name.
const HEADER_METHODS: &[&str] = &[
    "(net/http.Header).Add",
    "(net/http.Header).Del",
    "(net/http.Header).Get",
    "(net/http.Header).Set",
];

/// `(CallExpr (Symbol "net/http.CanonicalHeaderKey") _)` — the symbol, not any
/// selector that happens to end in `CanonicalHeaderKey`.
fn is_canonical_header_key_arg(pass: &Pass<'_>, arg: &Expr) -> bool {
    let Expr::CallExpr(call) = unparen(arg) else {
        return false;
    };
    code::call_name(pass, &call.fun).as_deref() == Some("net/http.CanonicalHeaderKey")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1035 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        let Some(callee) = code::callee_full_name(pass, call) else {
            return;
        };
        if !HEADER_METHODS.contains(&callee.as_str()) {
            return;
        }
        let Some(arg) = call.args.first() else {
            return;
        };
        if !is_canonical_header_key_arg(pass, arg) {
            return;
        }
        // Upstream quotes the parameter name and names the method it belongs
        // to — `on the 'key' argument of (net/http.Header).Set` — and reports
        // the redundant argument, not the enclosing call. Verified against
        // golangci-lint 2.12.2 for Add / Get / Set, including
        // `r.Header.Get(...)`.
        // `edit.ReplaceWithNode(fset, arg, arg.Args[0])`: the canonicalizing
        // call goes and its own argument takes its place, over the same
        // unparenthesized span the report uses. Upstream's pattern spells the
        // argument list `[_]`, so it only ever matches a one-argument call;
        // anything else is reported without a fix rather than rewritten to
        // something that would not compile.
        let inner = unparen(arg);
        let edit = match inner {
            Expr::CallExpr(CallExpr { args, .. }) if args.len() == 1 => {
                render_node(pass, &args[0]).map(|text| TextEdit {
                    pos: inner.pos().0 as u32,
                    end: inner.end().0 as u32,
                    new_text: text,
                })
            }
            _ => None,
        };
        pending.push((
            // The pattern binds the *unparenthesized* node, so upstream's
            // report starts inside the parens: `h.Set((canon(k)), v)` is
            // reported at `canon`, one column in from `(`.
            inner.pos().0 as u32,
            format!(
                "calling net/http.CanonicalHeaderKey on the 'key' argument of {callee} is redundant"
            ),
            edit,
        ));
    });
    for (pos, message, edit) in pending {
        let Some(edit) = edit else {
            pass.report_unless_generated(pos, message);
            continue;
        };
        if code::is_generated_at(pass, pos) {
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Remove call to CanonicalHeaderKey".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
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
