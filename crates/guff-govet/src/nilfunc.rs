//! `nilfunc` — check for useless comparisons of functions against nil.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/nilfunc`.

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr, Ident};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_nil;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;

use crate::expreq::unparen;

fn func_from_expr(pass: &Pass<'_>, expr: &Expr) -> Option<guff_types::ObjectId> {
    let Expr::Ident(Ident { id, .. }) = unparen(expr) else {
        return None;
    };
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = info.uses.get(id).copied()?;
    match artifacts.objects.get(obj) {
        ObjectData::Func(_) => Some(obj),
        _ => None,
    }
}

fn check_comparison(pass: &Pass<'_>, e: &BinaryExpr) -> Option<String> {
    let op = e.op;
    if op != Token::EQL && op != Token::NEQ {
        return None;
    }
    let other = if is_nil(pass, &e.x) {
        &e.y
    } else if is_nil(pass, &e.y) {
        &e.x
    } else {
        return None;
    };
    let func = func_from_expr(pass, other)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let name = func.name(&artifacts.objects);
    let op_str = if op == Token::EQL { "==" } else { "!=" };
    let always = op == Token::NEQ;
    Some(format!(
        "comparison of function {name} {op_str} nil is always {always}"
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nilfunc requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |n| {
        let NodeRef::BinaryExpr(e) = n else {
            return;
        };
        if let Some(message) = check_comparison(pass, e) {
            pending.push((e.op_pos.0 as u32, message));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nilfunc",
        doc: "check for useless comparisons between functions and nil",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/nilfunc",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
