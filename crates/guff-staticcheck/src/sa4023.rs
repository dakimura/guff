//! SA4023 — impossible comparison of interface value with untyped nil.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4023` (simplified; defers
//! nilness/typedness analysis of call results).
//!
//! The IR path mirrors honnef's `MakeInterface` case. An AST fallback covers
//! the same pattern while SSA `MakeInterface` emission is incomplete: an
//! interface variable assigned from a concrete pointer, then compared to nil.
//! The fallback only considers assignments that appear *before* the comparison
//! so later concrete writes (e.g. go-redis `Manager.Listener`) are not FPs.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Ident};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::callcheck;
use guff_analysis::code::is_nil;
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::is_nil_const;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{BinOp, InstrData};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;

fn is_interface_type(prog: &guff_ssa::program::Program, typ: guff_types::TypeId) -> bool {
    matches!(
        prog.type_arena.get(typ.underlying(&prog.type_arena)),
        TypeData::Interface(_)
    )
}

fn is_concrete_pointer(pass: &Pass<'_>, id: &Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&id.id) else {
        return false;
    };
    matches!(
        artifacts.types.get(tav.typ.underlying(&artifacts.types)),
        TypeData::Pointer(_)
    )
}

/// True if `id` was assigned a concrete pointer value before `before_pos`.
///
/// Matches by types object identity (not bare name) so a later write to a
/// different `listener` in another function cannot poison the comparison.
fn interface_from_concrete_pointer_before(
    pass: &Pass<'_>,
    id: &Ident,
    before_pos: u32,
) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(target) = info
        .uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).copied().flatten())
    else {
        return false;
    };
    let Some(inspect) = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .cloned()
    else {
        return false;
    };
    let mut found = false;
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |node| {
        let NodeRef::AssignStmt(AssignStmt { lhs, rhs, .. }) = node else {
            return;
        };
        let Some(Expr::Ident(lhs_id)) = lhs.first() else {
            return;
        };
        let Some(lhs_obj) = info
            .defs
            .get(&lhs_id.id)
            .copied()
            .flatten()
            .or_else(|| info.uses.get(&lhs_id.id).copied())
        else {
            return;
        };
        if lhs_obj != target {
            return;
        }
        // Only assignments that textually precede the comparison can feed it.
        if lhs_id.name_pos.0 as u32 >= before_pos {
            return;
        }
        let Some(rhs) = rhs.first() else {
            return;
        };
        if let Expr::Ident(rhs_id) = rhs {
            if is_concrete_pointer(pass, rhs_id) {
                found = true;
            }
        }
    });
    found
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4023 requires buildir analyzer".to_string())?;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4023 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::BinOp(BinOp {
                    op, x, y, typ, ..
                }) = func.instrs.get(iid)
                else {
                    continue;
                };
                if !matches!(*op, Token::EQL | Token::NEQ) {
                    continue;
                }
                if !is_interface_type(&ir.prog, *typ) {
                    continue;
                }
                if !is_nil_const(&ir.prog, func, *y) {
                    continue;
                }
                let qualifier = if *op == Token::EQL { "never" } else { "always" };
                let x = callcheck::flatten_ssa_value(func, *x);
                if let Value::Instr(xid) = x {
                    if matches!(func.instrs.get(xid), InstrData::MakeInterface(_)) {
                        // Match honnef's short diagnostic (related info omitted).
                        pending.push((
                            func.pos(iid).0 as u32,
                            format!("this comparison is {qualifier} true"),
                        ));
                    }
                }
            }
        }
    }

    inspect.preorder_typed(node_mask!(BinaryExpr), pass.files(), |node| {
        let NodeRef::BinaryExpr(bin) = node else {
            return;
        };
        if !matches!(bin.op, Token::EQL | Token::NEQ) {
            return;
        }
        let nil_on_rhs = is_nil(pass, &bin.y);
        let nil_on_lhs = is_nil(pass, &bin.x);
        if nil_on_rhs == nil_on_lhs {
            return;
        }
        let lhs = if nil_on_rhs { bin.x.as_ref() } else { bin.y.as_ref() };
        let Expr::Ident(id) = lhs else {
            return;
        };
        let Some(info) = pass.types_info() else {
            return;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return;
        };
        let Some(tav) = info.types.get(&id.id) else {
            return;
        };
        if !matches!(
            artifacts.types.get(tav.typ.underlying(&artifacts.types)),
            TypeData::Interface(_)
        ) {
            return;
        }
        if interface_from_concrete_pointer_before(pass, id, bin.op_pos.0 as u32) {
            let qualifier = if bin.op == Token::EQL { "never" } else { "always" };
            // Upstream reports the BinOp node; its position is the start of
            // the left operand, not the operator.
            pending.push((
                match_pos(node),
                format!("this comparison is {qualifier} true"),
            ));
        }
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4023",
        doc: "impossible comparison of interface value with untyped nil",
        url: "https://staticcheck.dev/docs/checks/#SA4023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
