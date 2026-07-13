//! SA4005 — field assignment that will never be observed
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4005`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, SelectorExpr, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{filter_debug, referrers, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{Alloc, FieldAddr, InstrData, Store};
use guff_ssa::value::Value;

fn is_value_receiver(recv: &guff::ast::FieldList) -> bool {
    recv.list.first().is_some_and(|f| {
        match f.ty.as_ref() {
            Some(Expr::StarExpr(_)) => false,
            Some(Expr::UnaryExpr(u)) if u.op == guff::token::Token::MUL => false,
            _ => true,
        }
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4005 requires buildir analyzer".to_string())?;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4005 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        let recv = func.params.iter().next().map(|(pid, _)| Value::Param(pid));
        let Some(recv) = recv else { continue };
        let refs = filter_debug(referrers(func, recv), func);
        if refs.len() != 1 {
            continue;
        }
        let InstrData::Store(Store { addr, .. }) = func.instrs.get(refs[0]) else { continue };
        let Value::Instr(addr_id) = *addr else { continue };
        let InstrData::Alloc(Alloc { heap: false, .. }) = func.instrs.get(addr_id) else { continue };
        for &ref_id in referrers(func, *addr) {
            let InstrData::FieldAddr(FieldAddr { field, .. }) = func.instrs.get(ref_id) else { continue };
            let field_refs = filter_debug(referrers(func, Value::Instr(ref_id)), func);
            let has_read = field_refs.iter().any(|&rid| !matches!(func.instrs.get(rid), InstrData::Store(_)));
            if !has_read {
                for &w in &field_refs {
                    if matches!(func.instrs.get(w), InstrData::Store(_)) {
                        pending.push((func.pos(w).0 as u32, format!("ineffective assignment to field .{field}")));
                        break;
                    }
                }
            }
        }
    }

    inspect.preorder(pass.files(), |node| {
        let NodeRef::FuncDecl(fd) = node else {
            return;
        };
        let Some(recv) = fd.recv.as_ref() else {
            return;
        };
        if !is_value_receiver(recv) {
            return;
        }
        let Some(recv_name) = recv.list.first().and_then(|f| f.names.first()) else {
            return;
        };
        let Some(recv_obj) = object_of(pass, recv_name) else {
            return;
        };
        let Some(body) = fd.body.as_ref() else {
            return;
        };
        let mut stores: Vec<(u32, String)> = Vec::new();
        let mut reads: Vec<String> = Vec::new();
        for stmt in &body.list {
            walk_assign_stmt(pass, stmt, recv_obj, &mut stores, &mut reads);
        }
        for (pos, field) in stores {
            if !reads.iter().any(|f| f == &field) {
                pending.push((pos, format!("ineffective assignment to field .{field}")));
            }
        }
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn walk_assign_stmt(
    pass: &Pass<'_>,
    stmt: &Stmt,
    recv_obj: guff_types::ObjectId,
    stores: &mut Vec<(u32, String)>,
    reads: &mut Vec<String>,
) {
    match stmt {
        Stmt::AssignStmt(AssignStmt { lhs, .. }) => {
            for lhs in lhs {
                if let Some((pos, field)) = selector_field_on(pass, lhs, recv_obj) {
                    stores.push((pos, field));
                }
            }
        }
        Stmt::ExprStmt(es) => {
            if let Some((_, field)) = selector_field_on(pass, &es.x, recv_obj) {
                reads.push(field);
            }
        }
        Stmt::IfStmt(i) => {
            for s in &i.body.list {
                walk_assign_stmt(pass, s, recv_obj, stores, reads);
            }
            if let Some(else_) = &i.else_ {
                if let Stmt::BlockStmt(b) = &**else_ {
                    for s in &b.list {
                        walk_assign_stmt(pass, s, recv_obj, stores, reads);
                    }
                } else {
                    walk_assign_stmt(pass, else_, recv_obj, stores, reads);
                }
            }
        }
        Stmt::ForStmt(f) => {
            for s in &f.body.list {
                walk_assign_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        Stmt::RangeStmt(r) => {
            for s in &r.body.list {
                walk_assign_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        Stmt::BlockStmt(b) => {
            for s in &b.list {
                walk_assign_stmt(pass, s, recv_obj, stores, reads);
            }
        }
        _ => {}
    }
}

fn selector_field_on(pass: &Pass<'_>, expr: &Expr, recv_obj: guff_types::ObjectId) -> Option<(u32, String)> {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = expr else {
        return None;
    };
    if !refers_to(pass, x, recv_obj) {
        return None;
    }
    Some((sel.name_pos.0 as u32, sel.name.clone()))
}

fn sa4005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4005",
        doc: "field assignment that will never be observed",
        url: "https://staticcheck.dev/docs/checks/#SA4005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer(), inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
