//! `assign` — check for useless assignments.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/assign`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, IndexExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;

use crate::expreq::{expr_equal, unparen};

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
            if is_map_index(pass, l) {
                continue;
            }
            if !expr_equal(l, r) {
                continue;
            }
            if let Expr::Ident(id) = unparen(l) {
                exprs.push(id.name.clone());
            } else {
                exprs.push("_".to_string());
            }
        }
        if exprs.is_empty() {
            return;
        }
        pending.push((
            lhs[0].pos().0 as u32,
            format!("self-assignment of {}", exprs.join(", ")),
        ));
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
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
