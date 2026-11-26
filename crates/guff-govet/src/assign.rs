//! `assign` — check for useless assignments.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/assign`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, IndexExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    refactor, AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix,
};
use guff_types::arena::TypeData;

use crate::expreq::{expr_equal, same_node_kind, unparen};
use crate::govet_util::{format_expr, no_effects};

fn is_map_index(pass: &Pass<'_>, e: &Expr) -> bool {
    let Expr::IndexExpr(IndexExpr { x, .. }) = unparen(e) else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(t) = info.types.get(&x.id()).map(|tv| tv.typ) else {
        return false;
    };
    matches!(artifacts.types.get(t), TypeData::Map(_))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "assign requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |n| {
        let NodeRef::AssignStmt(AssignStmt { tok, lhs, rhs, .. }) = n else {
            return;
        };
        if *tok != Some(Token::ASSIGN) || lhs.len() != rhs.len() {
            return;
        }
        let mut exprs = Vec::new();
        for (l, r) in lhs.iter().zip(rhs) {
            // Upstream's guard is a conjunction of four tests, and guff had
            // only the map one. `NoEffects` keeps `a[f()] = a[f()]` out (the
            // suggested fix would delete two calls), and the node-kind test
            // keeps `x = (x)` out (the text comparison below runs through
            // go/printer, which erases the parentheses).
            if !no_effects(pass, l) || !no_effects(pass, r) {
                continue;
            }
            if is_map_index(pass, l) {
                continue;
            }
            if !same_node_kind(l, r) || !expr_equal(l, r) {
                continue;
            }
            // Upstream names the operand with `analysisutil.Format(lhs)`, not
            // with the identifier: `s.f = s.f` reports "self-assignment of s.f".
            // guff printed "_" for everything that was not a bare ident.
            exprs.push(format_expr(pass, l));
        }
        if exprs.is_empty() {
            return;
        }
        // Upstream removes the whole statement when *every* part of it is a
        // self-assignment, and edits the redundant lhs/rhs runs otherwise.
        // Only the first is done here: the run form has to splice out
        // intervening commas, and getting that half-right writes worse code
        // than leaving the finding unfixed.
        let span = (exprs.len() == lhs.len())
            .then(|| {
                Some((
                    lhs[0].pos().0 as u32,
                    rhs.last()?.end().0 as u32,
                ))
            })
            .flatten();
        pending.push((
            lhs[0].pos().0 as u32,
            format!("self-assignment of {}", exprs.join(", ")),
            span,
        ));
    });

    for (pos, message, span) in pending {
        let text_edits = span
            .and_then(|(from, to)| {
                let file = refactor::enclosing_file(pass, from)?;
                Some(refactor::delete_with_line(
                    file,
                    refactor::file_source(pass, file),
                    from,
                    to,
                ))
            })
            .unwrap_or_default();
        if text_edits.is_empty() {
            pass.reportf(pos, message);
            continue;
        }
        pass.report(Diagnostic {
            pos,
            message,
            suggested_fixes: vec![SuggestedFix {
                message: "Remove self-assignment".into(),
                text_edits,
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "assign",
        doc: "check for useless assignments",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/assign",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
