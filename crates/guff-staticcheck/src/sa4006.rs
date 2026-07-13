//! SA4006 — assigned value never read before overwrite.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4006` (simplified; defers goyacc/generated filtering).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Ident, IncDecStmt, Stmt};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{filter_debug, referrers, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{Extract, InstrData};
use guff_ssa::value::Value;
use crate::render::render_expr;

fn has_use(func: &guff_ssa::function::Function, v: Value) -> bool {
    let refs = filter_debug(guff_analysis::referrers(func, v), func);
    for &rid in &refs {
        match func.instrs.get(rid) {
            InstrData::Phi(_) => {
                if has_use(func, Value::Instr(rid)) {
                    return true;
                }
            }
            InstrData::DebugRef(_) => {}
            InstrData::Store(_) => {}
            _ => return true,
        }
    }
    false
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    if let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) {
        inspect.preorder(pass.files(), |node| {
            let NodeRef::AssignStmt(assign) = node else {
                return;
            };
        let func = ir.src_funcs.iter().find_map(|&fid| {
            let f = ir.prog.functions.get(fid);
            assign
                .rhs
                .first()
                .and_then(|e| f.value_for_expr(e))
                .or_else(|| assign.lhs.first().and_then(|e| f.value_for_expr(e)))
                .map(|_| f)
        });
        let Some(func) = func else {
            return;
        };
        if assign.lhs.len() > 1 && assign.rhs.len() == 1 {
            if let Some((v, _)) = func.value_for_expr(&assign.rhs[0]) {
                for rid in filter_debug(referrers(func, v), func) {
                    if let InstrData::Extract(Extract { index, .. }) = func.instrs.get(rid) {
                        let lhs = &assign.lhs[*index];
                        if matches!(lhs, Expr::Ident(Ident { name, .. }) if name == "_") {
                            continue;
                        }
                        if !has_use(func, Value::Instr(rid)) {
                            pending.push((
                                assign.tok_pos.0 as u32,
                                format!("this value of {} is never used", render_expr(lhs)),
                            ));
                        }
                    }
                }
            }
            return;
        }
        if assign.lhs.len() != assign.rhs.len() {
            return;
        }
        for (lhs, rhs) in assign.lhs.iter().zip(assign.rhs.iter()) {
            if matches!(lhs, Expr::Ident(Ident { name, .. }) if name == "_") {
                continue;
            }
            let val = func
                .value_for_expr(rhs)
                .map(|(v, _)| v)
                .or_else(|| {
                    if assign.tok != Some(Token::ASSIGN) {
                        func.value_for_expr(lhs).map(|(v, _)| v)
                    } else {
                        None
                    }
                });
            let Some(v) = val else {
                continue;
            };
            if matches!(v, Value::Const(_)) {
                continue;
            }
            if !has_use(func, v) {
                pending.push((
                    assign.tok_pos.0 as u32,
                    format!("this value of {} is never used", render_expr(lhs)),
                ));
            }
        }
        });
        for (pos, msg) in &pending {
            pass.report_unless_generated(*pos, msg.clone());
        }
        pending.clear();
    }

    let mut ast_pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::IncDecStmt(inc) = node else {
            return;
        };
        let Expr::Ident(id) = &inc.x else {
            return;
        };
        if object_of(pass, id).is_none() {
            return;
        };
        if ident_used_before(pass, id, inc.tok_pos.0 as u32) {
            return;
        }
        ast_pending.push((
            inc.tok_pos.0 as u32,
            format!("this value of {} is never used", render_expr(&inc.x)),
        ));
    });
    for (pos, msg) in ast_pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn ident_used_before(pass: &Pass<'_>, id: &Ident, before: u32) -> bool {
    let Some(obj) = object_of(pass, id) else {
        return false;
    };
    let Some(inspect) = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .cloned()
    else {
        return false;
    };
    let mut used = false;
    inspect.preorder(pass.files(), |node| {
        if used {
            return;
        }
        let pos = node_pos(node);
        if pos >= before {
            return;
        }
        if node_reads_obj(pass, node, obj) {
            used = true;
        }
    });
    used
}

fn node_pos(node: NodeRef<'_>) -> u32 {
    match node {
        NodeRef::AssignStmt(s) => s.tok_pos.0 as u32,
        NodeRef::IncDecStmt(s) => s.tok_pos.0 as u32,
        NodeRef::ExprStmt(s) => s.x.id() as u32,
        _ => 0,
    }
}

fn node_reads_obj(pass: &Pass<'_>, node: NodeRef<'_>, obj: guff_types::ObjectId) -> bool {
    match node {
        NodeRef::AssignStmt(AssignStmt { rhs, .. }) => rhs.iter().any(|e| refers_to(pass, e, obj)),
        NodeRef::ExprStmt(es) => refers_to(pass, &es.x, obj),
        NodeRef::IncDecStmt(IncDecStmt { x, .. }) => refers_to(pass, x, obj),
        _ => false,
    }
}

fn sa4006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4006",
        doc: "a value assigned to a variable is never read before being overwritten",
        url: "https://staticcheck.dev/docs/checks/#SA4006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
