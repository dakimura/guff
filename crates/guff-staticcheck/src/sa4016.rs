//! SA4016 — certain bitwise operations with zero do nothing useful
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4016`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::is_integer_literal;

use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use crate::render::render_expr;

fn is_integer(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let u = tav.typ.underlying(&artifacts.types);
    matches!(artifacts.types.get(u), TypeData::Basic(b) if matches!(b.kind(), BasicKind::Int | BasicKind::Int8 | BasicKind::Int16 | BasicKind::Int32 | BasicKind::Int64 | BasicKind::Uint | BasicKind::Uint8 | BasicKind::Uint16 | BasicKind::Uint32 | BasicKind::Uint64 | BasicKind::Uintptr))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4016 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else { return };
        if !matches!(bin.op, Token::AND | Token::OR | Token::XOR) { return; }
        if !is_integer(pass, &bin.x) { return; }
        if !is_integer_literal(pass, &bin.y, 0) { return; }
        let rendered = render_expr(&Expr::BinaryExpr(bin.clone()));
        let msg = match bin.op {
            Token::AND => format!("{rendered} always equals 0"),
            Token::OR | Token::XOR => format!("{rendered} always equals {}", render_expr(&bin.x)),
            _ => return,
        };
        // Upstream reports the BinaryExpr node; its position is the start of
        // the left operand, not the operator.
        pending.push((match_pos(node), msg));
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4016_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4016",
        doc: "certain bitwise operations with zero do nothing useful",
        url: "https://staticcheck.dev/docs/checks/#SA4016",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4016_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4016_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
