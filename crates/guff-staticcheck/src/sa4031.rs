//! SA4031 — checking never-nil value against nil.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4031` (simplified).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{match_pattern, match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_pattern::{must_parse, Pattern};
use guff_ssa::instr::InstrData;
use guff_ssa::value::Value;

static PAT_NIL_CMP: OnceLock<Pattern> = OnceLock::new();
static PAT4022: OnceLock<Pattern> = OnceLock::new();

fn pat_nil_cmp() -> &'static Pattern {
    PAT_NIL_CMP.get_or_init(|| {
        must_parse(r#"(BinaryExpr lhs op@(Or "==" "!=") (Or nil (Ident "nil")))"#)
    })
}

fn pat4022() -> &'static Pattern {
    PAT4022.get_or_init(|| {
        must_parse(r#"(BinaryExpr (UnaryExpr "&" _) (Or "==" "!=") (Or nil (Ident "nil")))"#)
    })
}

fn never_nil(func: &guff_ssa::function::Function, v: Value) -> bool {
    match v {
        Value::Function(_) => true,
        Value::Instr(iid) => matches!(
            func.instrs.get(iid),
            InstrData::MakeChan(_) | InstrData::MakeMap(_) | InstrData::MakeSlice(_) | InstrData::Alloc(_)
        ),
        _ => false,
    }
}

fn ast_never_nil_lhs(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(CallExpr { fun, .. }) => matches!(
            fun.as_ref(),
            Expr::Ident(id) if id.name == "make"
        ),
        _ => false,
    }
}

fn is_nil_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "nil")
}

fn lhs_is_never_nil(
    pass: &Pass<'_>,
    ir: &guff_analysis::passes::buildir::BuildIrResult,
    lhs: &Expr,
) -> bool {
    if ast_never_nil_lhs(lhs) {
        return true;
    }
    let func = ir.src_funcs.iter().find_map(|&fid| {
        let f = ir.prog.functions.get(fid);
        f.value_for_expr(lhs).map(|_| f)
    });
    let Some(func) = func else {
        return false;
    };
    let Some((v, is_addr)) = func.value_for_expr(lhs) else {
        return false;
    };
    !is_addr && never_nil(func, v)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4031 requires buildir analyzer".to_string())?;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4031 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else {
            return;
        };
        if !matches!(bin.op, Token::EQL | Token::NEQ) {
            return;
        }
        let nil_on_rhs = is_nil_expr(bin.y.as_ref());
        let nil_on_lhs = is_nil_expr(bin.x.as_ref());
        if nil_on_rhs == nil_on_lhs {
            return;
        }
        let lhs = if nil_on_rhs { bin.x.as_ref() } else { bin.y.as_ref() };
        if !ast_never_nil_lhs(lhs) {
            return;
        }
        let qualifier = if bin.op == Token::EQL { "never" } else { "always" };
        pending.push((
            bin.op_pos.0 as u32,
            format!("this nil check is {qualifier} true"),
        ));
    });
    inspect.preorder_typed(node_mask!(BinaryExpr, IfStmt), pass.files(), |node| {
        let cmp_node = match node {
            NodeRef::IfStmt(if_) => {
                if let Expr::BinaryExpr(cond) = &if_.cond {
                    NodeRef::BinaryExpr(cond)
                } else {
                    return;
                }
            }
            NodeRef::BinaryExpr(_) => node,
            _ => return,
        };
        if match_pattern(pass, pat4022(), cmp_node).is_some() {
            return;
        }
        let Some(m) = match_pattern(pass, pat_nil_cmp(), cmp_node) else {
            return;
        };
        let Some(lhs) = m.state.get("lhs").and_then(|v| v.as_expr()) else {
            return;
        };
        if !lhs_is_never_nil(pass, ir, lhs) {
            return;
        }
        let op = m
            .state
            .get("op")
            .and_then(|v| v.as_token())
            .unwrap_or(Token::EQL);
        let qualifier = if op == Token::EQL { "never" } else { "always" };
        pending.push((
            match_pos(node),
            format!("this nil check is {qualifier} true"),
        ));
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4031_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4031",
        doc: "checking never-nil value against nil",
        url: "https://staticcheck.dev/docs/checks/#SA4031",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4031_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4031_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
