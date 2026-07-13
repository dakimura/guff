//! SA9006 — dubious bit shifting of a fixed size integer value.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9006`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, Expr};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::expr_to_int;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn fixed_type_bits(pass: &Pass<'_>, expr: &Expr) -> Option<i64> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = info.types.get(&expr.id())?.typ;
    let TypeData::Basic(b) = artifacts.types.get(typ.underlying(&artifacts.types)) else {
        return None;
    };
    let bits = match b.kind() {
        BasicKind::Int8 | BasicKind::Uint8 => 8,
        BasicKind::Int16 | BasicKind::Uint16 => 16,
        BasicKind::Int32 | BasicKind::Uint32 => 32,
        BasicKind::Int64 | BasicKind::Uint64 => 64,
        _ => return None,
    };
    Some(bits)
}

fn check_shift(pass: &Pass<'_>, value: &Expr, shift: &Expr, pending: &mut Vec<(u32, String)>, pos: u32) {
    let (Some(size), Some(shift_amt)) = (fixed_type_bits(pass, value), expr_to_int(pass, shift)) else {
        return;
    };
    if shift_amt >= size {
        pending.push((
            pos,
            format!("shifting {size}-bit value by {shift_amt} bits will always clear it"),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        match node {
            NodeRef::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) => {
                if !matches!(tok, Some(Token::ShrAssign) | Some(Token::ShlAssign)) {
                    return;
                }
                if let (Some(lhs), Some(rhs)) = (lhs.first(), rhs.first()) {
                    check_shift(pass, lhs, rhs, &mut pending, match_pos(node));
                }
            }
            NodeRef::BinaryExpr(BinaryExpr { x, y, op, .. }) => {
                if !matches!(op, Token::SHR | Token::SHL) {
                    return;
                }
                check_shift(pass, x, y, &mut pending, match_pos(node));
            }
            _ => {}
        }
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa9006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9006",
        doc: "dubious bit shifting of a fixed size integer value",
        url: "https://staticcheck.dev/docs/checks/#SA9006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
