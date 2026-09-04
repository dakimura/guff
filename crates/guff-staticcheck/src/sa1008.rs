//! SA1008 — non-canonical key in `http.Header` map.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1008`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, IndexExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, is_of_type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::code;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass, Diagnostic, SuggestedFix, TextEdit};

/// Port of Go's `net/textproto.CanonicalMIMEHeaderKey`.
fn canonical_header_key(s: &str) -> String {
    let mut upper = true;
    let mut out = Vec::with_capacity(s.len());
    for &c in s.as_bytes() {
        if !valid_header_field_byte(c) {
            return s.to_string();
        }
        let mut c = c;
        if upper && c.is_ascii_lowercase() {
            c -= b'a' - b'A';
        } else if !upper && c.is_ascii_uppercase() {
            c += b'a' - b'A';
        }
        upper = c == b'-';
        out.push(c);
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

fn valid_header_field_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'!' || c == b'#' || c == b'$' || c == b'%' || c == b'&'
        || c == b'\'' || c == b'*' || c == b'+' || c == b'-' || c == b'.' || c == b'^'
        || c == b'_' || c == b'`' || c == b'|' || c == b'~'
}

fn is_header_map(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(_) | Expr::SelectorExpr(_))
}

fn index_on_header(pass: &Pass<'_>, ix: &IndexExpr) -> bool {
    is_header_map(&ix.x) && is_of_type_with_name(pass, &ix.x, "net/http.Header")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1008 requires inspect analyzer".to_string())?
        .clone();

    // An assignment that writes an `http.Header` key takes its **whole subtree**
    // out of the walk, not just the index on its left:
    //
    //     if code.IsOfTypeWithName(pass, op.X, "net/http.Header") {
    //         return false      // `inspector.Nodes`: do not descend
    //     }
    //
    // and upstream's TODO says so in as many words — "this risks missing some
    // Header reads, for example in `h1["foo"] = h2["foo"]`". guff only skipped
    // the left-hand index, so `h[k] = append(h[k], v)` reported the read on the
    // right. k6 `internal/output/prometheusrw/sigv4/sigv4.go` writes two of
    // those.
    let mut skip: HashSet<u32> = HashSet::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        let NodeRef::AssignStmt(assign) = node else {
            return;
        };
        let writes_header = assign.lhs.iter().any(|lhs| match lhs {
            Expr::IndexExpr(ix) => index_on_header(pass, ix),
            _ => false,
        });
        if !writes_header {
            return;
        }
        guff::walk::preorder(node, |n| {
            if let NodeRef::IndexExpr(ix) = n {
                skip.insert(ix.id);
            }
            true
        });
    });

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(IndexExpr), pass.files(), |node| {
        let NodeRef::IndexExpr(ix) = node else {
            return;
        };
        if skip.contains(&ix.id) || !index_on_header(pass, ix) {
            return;
        }
        let Some(key) = expr_to_string(pass, &ix.index) else {
            return;
        };
        let canonical = canonical_header_key(&key);
        if key == canonical {
            return;
        }
        // Two shapes, two fixes. A literal key is rewritten to the canonical
        // spelling — quoted the way Go quotes it, which is why this reaches for
        // `gostd::strconv` rather than formatting the string itself. A named
        // key cannot be rewritten, so it is wrapped instead. Anything else (an
        // arbitrary expression) is reported without a fix, as upstream does.
        let edit = match ix.index.as_ref() {
            Expr::BasicLit(_) => Some(TextEdit {
                pos: ix.index.pos().0 as u32,
                end: ix.index.end().0 as u32,
                new_text: crate::gostd::strconv::quote(&canonical),
            }),
            Expr::Ident(id) => Some(TextEdit {
                pos: ix.index.pos().0 as u32,
                end: ix.index.end().0 as u32,
                new_text: format!("http.CanonicalHeaderKey({})", id.name),
            }),
            _ => None,
        };
        pending.push((
            match_pos(node),
            format!(
                "keys in http.Header are canonicalized, {key:?} is not canonical; fix the constant or use http.CanonicalHeaderKey"
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
                message: "Canonicalize header key".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn sa1008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1008",
        doc: "non-canonical key in http.Header map",
        url: "https://staticcheck.dev/docs/checks/#SA1008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1008 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn canonical_header_key_matches_go() {
        assert_eq!(canonical_header_key("foo"), "Foo");
        assert_eq!(canonical_header_key("etag"), "Etag");
        assert_eq!(canonical_header_key("content-type"), "Content-Type");
        assert_eq!(canonical_header_key("Foo"), "Foo");
    }
}
