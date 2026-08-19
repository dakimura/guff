//! SA4000 — binary operator has identical expressions on both sides.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4000`.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_generated_at};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

use crate::render::render_node;

fn is_float_type(pass: &Pass<'_>, expr: &Expr) -> bool {
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
    matches!(
        artifacts.types.get(u),
        TypeData::Basic(b) if matches!(b.kind(), BasicKind::Float32 | BasicKind::Float64)
    )
}

fn is_rand_call(name: &str) -> bool {
    name.starts_with("math/rand.") || name.contains("(*math/rand.Rand).")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4000 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(op) = node else {
            return;
        };
        let flagged = match op.op {
            Token::EQL | Token::NEQ => true,
            Token::SUB
            | Token::QUO
            | Token::AND
            | Token::REM
            | Token::OR
            | Token::XOR
            | Token::LAND
            | Token::LOR
            | Token::LSS
            | Token::GTR
            | Token::LEQ
            | Token::GEQ => true,
            _ => false,
        };
        if !flagged {
            return;
        }
        if is_float_type(pass, &op.x) {
            return;
        }
        if std::mem::discriminant(&*op.x) != std::mem::discriminant(&*op.y) {
            return;
        }
        // Upstream compares `report.Render` of the two operands, i.e. the
        // printed source. A node the printer cannot render is not a node we
        // know to be identical to anything.
        match (render_node(pass, &op.x), render_node(pass, &op.y)) {
            (Some(x), Some(y)) if x == y => {}
            _ => return,
        }
        if let Expr::CallExpr(c) = &*op.x {
            if let Some(n) = call_name(pass, &c.fun) {
                if is_rand_call(&n) {
                    return;
                }
            }
        }
        if let (Expr::BasicLit(l1), Expr::BasicLit(l2)) = (&*op.x, &*op.y) {
            if l1.value == "0" && l2.value == "0" && is_generated_at(pass, l1.value_pos.0 as u32) {
                return;
            }
        }
        let op_str = match op.op {
            Token::EQL => "==",
            Token::NEQ => "!=",
            Token::SUB => "-",
            Token::QUO => "/",
            Token::AND => "&",
            Token::REM => "%",
            Token::OR => "|",
            Token::XOR => "^",
            Token::LAND => "&&",
            Token::LOR => "||",
            Token::LSS => "<",
            Token::GTR => ">",
            Token::LEQ => "<=",
            Token::GEQ => ">=",
            _ => "?",
        };
        // Upstream reports the BinaryExpr node, whose Pos() is the left
        // operand's start — not the operator.
        pending.push((
            op.x.pos().0 as u32,
            format!("identical expressions on the left and right side of the '{op_str}' operator"),
        ));
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa4000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4000",
        doc: "binary operator has identical expressions on both sides",
        url: "https://staticcheck.dev/docs/checks/#SA4000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
