//! S1005 — drop unnecessary use of the blank identifier.
//!
//! Port of `honnef.co/go/tools/simple/s1005`.

use std::sync::OnceLock;

use guff::ast::{Expr, RangeStmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_pattern::{must_parse, Pattern};
use guff_analysis::passes::inspect;
use guff::ast::Stmt;
use guff_analysis::code;
use guff_analysis::code::unparen;
use guff_analysis::{
    entry_mask, match_pattern, match_pos, AnalysisResult, Analyzer, Diagnostic, Pass, RunError,
    RunFn, SuggestedFix, TextEdit,
};

use crate::render::render_stmt;

static PAT_BLANK_RECV1: OnceLock<Pattern> = OnceLock::new();
static PAT_BLANK_RECV2: OnceLock<Pattern> = OnceLock::new();

fn pat_blank_recv1() -> &'static Pattern {
    PAT_BLANK_RECV1.get_or_init(|| {
        must_parse(r#"(AssignStmt [_ (Ident "_")] _ (UnaryExpr "<-" _))"#)
    })
}

fn pat_blank_recv2() -> &'static Pattern {
    PAT_BLANK_RECV2
        .get_or_init(|| must_parse(r#"(AssignStmt (Ident "_") _ recv@(UnaryExpr "<-" _))"#))
}

/// `astutil.IsBlank`, which is purely syntactic: an identifier spelled `_`.
///
/// This used to also require the identifier to resolve to no object, which
/// silently dropped the `for i, _ := range xs` shape — a `:=` *declares* its
/// blank, so it has one. The fixture held only `_ = <-ch` and neither tier
/// could see the gap; adding the three range shapes to it surfaced the missing
/// finding immediately (2026-08-27).
fn is_blank(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(ident) if ident.name == "_")
}

/// An `AssignStmt` has no `pos()`/`end()` of its own: it starts at its first
/// left-hand expression and ends at its last right-hand one.
fn assign_pos(a: &guff::ast::AssignStmt) -> guff::position::Pos {
    a.lhs.first().map(|e| e.pos()).unwrap_or_default()
}

fn assign_end(a: &guff::ast::AssignStmt) -> guff::position::Pos {
    a.rhs.last().map(|e| e.end()).unwrap_or_default()
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String, &'static str, Option<TextEdit>)> = Vec::new();

    inspect.preorder_typed(entry_mask(pat_blank_recv1()), pass.files(), |node| {
        if match_pattern(pass, pat_blank_recv1(), node).is_none() {
            return;
        }
        // Upstream copies the AssignStmt, truncates `Lhs` to its first element
        // and replaces the statement with the copy — so `x, _ = m[k]` becomes
        // `x = m[k]` through go/printer, not through string surgery on the
        // source.
        let edit = match node {
            NodeRef::AssignStmt(assign) if !assign.lhs.is_empty() => {
                let mut trimmed = assign.clone();
                trimmed.lhs.truncate(1);
                render_stmt(pass, &Stmt::AssignStmt(trimmed)).map(|text| TextEdit {
                    pos: assign_pos(assign).0 as u32,
                    end: assign_end(assign).0 as u32,
                    new_text: text,
                })
            }
            _ => None,
        };
        pending.push((
            match_pos(node),
            "unnecessary assignment to the blank identifier".into(),
            "Remove assignment to blank identifier",
            edit,
        ));
    });

    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        let NodeRef::AssignStmt(assign) = node else {
            return;
        };
        if match_pattern(pass, pat_blank_recv2(), node).is_none() {
            return;
        }
        // `_ = <-ch` collapses to the receive itself. The pattern binds it as
        // `recv`, but a `recv@(UnaryExpr …)` binding arrives as a `Node`, not
        // an `Expr`, so reading it back through `as_expr()` silently yields
        // nothing — the right-hand side of the statement is the same node and
        // says so directly. `unparen` because the pattern strips parens before
        // binding, so `_ = (<-ch)` binds the receive, not the parens.
        let edit = assign.rhs.first().and_then(|rhs| {
            let recv = unparen(rhs);
            crate::render::render_node(pass, recv).map(|text| TextEdit {
                pos: assign_pos(assign).0 as u32,
                end: assign_end(assign).0 as u32,
                new_text: text,
            })
        });
        pending.push((
            match_pos(node),
            "unnecessary assignment to the blank identifier".into(),
            "Simplify channel receive operation",
            edit,
        ));
    });

    inspect.preorder_typed(node_mask!(RangeStmt), pass.files(), |node| {
        let NodeRef::RangeStmt(rs) = node else {
            return;
        };
        check_range_blank(rs, &mut pending);
    });

    for (pos, message, fix_msg, edit) in pending {
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
                message: fix_msg.into(),
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The three range shapes, all fixed by `edit.Delete` rather than by printing
/// anything: `for _ :=` and `for _, _ :=` lose everything from the key up to and
/// including the `:=`, and `for x, _ :=` loses the `, _` between the key's end
/// and the value's.
fn check_range_blank(
    rs: &RangeStmt,
    pending: &mut Vec<(u32, String, &'static str, Option<TextEdit>)>,
) {
    const MSG: &str = "unnecessary assignment to the blank identifier";
    const FIX: &str = "Remove assignment to blank identifier";
    let key_blank = rs.key.as_ref().is_some_and(|k| is_blank(k));
    let value_blank = rs.value.as_ref().is_some_and(|v| is_blank(v));

    // `edit.Delete(edit.Range{rs.Key.Pos(), rs.TokPos + 1})`. The `+ 1` assumes
    // a one-byte token, which looks unsafe next to a two-byte `:=` — but a
    // range whose left side is only blanks can never use `:=`: `for _ := range`
    // and `for _, _ := range` are both rejected by the compiler with "no new
    // variables on left side of :=" (measured). So the shapes that reach this
    // deletion always spell the token `=`, and the third shape below does not
    // touch the token at all.
    //
    // The deletion leaves `for  range xs`, with the space that followed the
    // token; the gofmt pass that always follows a fix collapses it. Measured
    // against golangci-lint 2.12.2: `for _, _ = range xs` and `for _ = range
    // xs` both become `for range xs`, and `for i, _ := range xs` becomes
    // `for i := range xs`.
    let drop_key = |k: &guff::ast::Expr| TextEdit {
        pos: k.pos().0 as u32,
        end: (rs.tok_pos.0 + 1) as u32,
        new_text: String::new(),
    };

    if rs.value.is_none() && key_blank {
        let k = rs.key.as_ref().unwrap();
        pending.push((k.pos().0 as u32, MSG.into(), FIX, Some(drop_key(k))));
    }
    if key_blank && value_blank {
        let k = rs.key.as_ref().unwrap();
        pending.push((k.pos().0 as u32, MSG.into(), FIX, Some(drop_key(k))));
    }
    if !key_blank && rs.key.is_some() && value_blank {
        let (k, v) = (rs.key.as_ref().unwrap(), rs.value.as_ref().unwrap());
        pending.push((
            v.pos().0 as u32,
            MSG.into(),
            FIX,
            Some(TextEdit {
                pos: k.end().0 as u32,
                end: v.end().0 as u32,
                new_text: String::new(),
            }),
        ));
    }
}

fn s1005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1005",
        doc: "drop unnecessary use of the blank identifier",
        url: "https://staticcheck.dev/docs/checks/#S1005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1005 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
