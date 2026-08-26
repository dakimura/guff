//! S1033 — unnecessary guard around call to delete.
//!
//! Port of `honnef.co/go/tools/simple/s1033`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, IfStmt, IndexExpr, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{self, is_call_to, object_of, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{
    match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_node;

fn same_expr(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(ia), Expr::Ident(ib)) => {
            object_of(pass, ia) == object_of(pass, ib) || ia.name == ib.name
        }
        _ => a.id() == b.id(),
    }
}

fn map_index_init(init: &Stmt) -> Option<(&Expr, &Expr)> {
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = init else {
        return None;
    };
    if tok != &Some(Token::DEFINE) || lhs.len() != 2 || rhs.len() != 1 {
        return None;
    };
    let Expr::Ident(blank) = unparen(&lhs[0]) else {
        return None;
    };
    if blank.name != "_" {
        return None;
    };
    let Expr::IndexExpr(IndexExpr { x: m, index: key, .. }) = unparen(&rhs[0]) else {
        return None;
    };
    Some((m, key))
}

/// The guarded `delete(m, key)` call, when `ifs` is the shape upstream matches.
///
/// Returns the call itself rather than a bool: it is both the thing that makes
/// the guard unnecessary and, unparenthesized, the text that replaces it.
fn check_if<'a>(pass: &Pass<'_>, ifs: &'a IfStmt) -> Option<&'a Expr> {
    let (m, key) = ifs.init.as_deref().and_then(map_index_init)?;
    let Expr::Ident(ok) = unparen(&ifs.cond) else {
        return None;
    };
    if ok.name == "_" || ifs.else_.is_some() || ifs.body.list.len() != 1 {
        return None;
    }
    let Stmt::ExprStmt(es) = &ifs.body.list[0] else {
        return None;
    };
    let inner = unparen(&es.x);
    let Expr::CallExpr(call) = inner else {
        return None;
    };
    let ok = call.args.len() == 2
        && same_expr(pass, &call.args[0], m)
        && same_expr(pass, &call.args[1], key)
        && is_call_to(pass, call, "delete");
    ok.then_some(inner)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1033 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, Option<TextEdit>)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        let Some(call) = check_if(pass, ifs) else {
            return;
        };
        // `edit.ReplaceWithNode(fset, node, call)`: the whole `if` — init,
        // condition, braces and all — collapses to the call it was guarding.
        // `report.ShortRange()` narrows where the diagnostic is *reported*, not
        // what the fix rewrites, so the edit spans the statement.
        let edit = render_node(pass, call).map(|text| TextEdit {
            pos: ifs.if_.0 as u32,
            // `check_if` has already established there is no `else`, so the
            // statement ends at the closing brace of the body.
            end: ifs.body.end().0 as u32,
            new_text: text,
        });
        pending.push((
            match_pos(node),
            "unnecessary guard around call to delete".into(),
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
                message: "Remove guard".into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn s1033_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1033",
        doc: "unnecessary guard around call to delete",
        url: "https://staticcheck.dev/docs/checks/#S1033",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1033 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1033_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1033_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
