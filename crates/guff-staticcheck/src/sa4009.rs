//! SA4009 — function argument overwritten before first use.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4009`.
//!
//! Uses SSA Parameter referrers (FilterDebug) like upstream: if the parameter
//! value has any non-DebugRef use after lifting, it was read. The previous AST
//! walk missed uses in `if`/`for` conditions and false-positived e.g.
//! `if st == nil { st = ... }` in prometheus `model/textparse`.
//!
//! When SSA misses interface-method uses of a param (e.g.
//! `logger = logger.Named(...)` after lift), fall back to an AST value-use
//! check so we do not false-positive overwrite-before-use.

use std::sync::OnceLock;

use guff::ast::{Expr, Stmt};
use guff::node_mask;
use guff::walk::{preorder, stmt_ref, NodeRef};
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{
    has_non_debug_referrer, referrers, AnalysisResult, Analyzer, Diagnostic, Pass,
    RelatedInformation, RunError, RunFn,
};
use guff_ssa::value::Value;
use guff_types::ObjectId;

/// Position of the first assignment whose LHS is `obj` (upstream's
/// `ast.Inspect` over AssignStmt).
/// Position of the first assignment to `obj`, in walk order.
///
/// Upstream keeps the node, not a yes/no: `report.Related(assignment, …)`
/// attaches "assignment to x" to the `*ast.AssignStmt` it found first
/// (`ast.Inspect`, stopping at the first hit). guff answered the question as a
/// bool and so had nothing to point at, and the related diagnostic was missing
/// entirely.
fn body_assigns_to(pass: &Pass<'_>, body: &[Stmt], obj: ObjectId) -> Option<u32> {
    body.iter().find_map(|s| stmt_assigns_to(pass, s, obj))
}

fn stmt_assigns_to(pass: &Pass<'_>, stmt: &Stmt, obj: ObjectId) -> Option<u32> {
    match stmt {
        Stmt::AssignStmt(a) => a
            .lhs
            .iter()
            .any(|e| matches!(e, Expr::Ident(id) if object_of(pass, id) == Some(obj)))
            // `AssignStmt.Pos()` is `Lhs[0].Pos()`.
            .then(|| a.lhs.first().map_or(a.tok_pos.0 as u32, |e| e.pos().0 as u32)),
        Stmt::BlockStmt(b) => body_assigns_to(pass, &b.list, obj),
        Stmt::IfStmt(i) => i
            .init
            .as_ref()
            .and_then(|s| stmt_assigns_to(pass, s, obj))
            .or_else(|| body_assigns_to(pass, &i.body.list, obj))
            .or_else(|| {
                i.else_
                    .as_ref()
                    .and_then(|e| stmt_assigns_to(pass, e, obj))
            }),
        Stmt::ForStmt(f) => f
            .init
            .as_ref()
            .and_then(|s| stmt_assigns_to(pass, s, obj))
            .or_else(|| f.post.as_ref().and_then(|s| stmt_assigns_to(pass, s, obj)))
            .or_else(|| body_assigns_to(pass, &f.body.list, obj)),
        Stmt::RangeStmt(r) => body_assigns_to(pass, &r.body.list, obj),
        Stmt::SwitchStmt(s) => s.body.list.iter().find_map(|c| match c {
            Stmt::CaseClause(cc) => body_assigns_to(pass, &cc.body, obj),
            _ => None,
        }),
        Stmt::TypeSwitchStmt(s) => s
            .init
            .as_ref()
            .and_then(|i| stmt_assigns_to(pass, i, obj))
            .or_else(|| stmt_assigns_to(pass, &s.assign, obj))
            .or_else(|| {
                s.body.list.iter().find_map(|c| match c {
                    Stmt::CaseClause(cc) => body_assigns_to(pass, &cc.body, obj),
                    _ => None,
                })
            }),
        Stmt::SelectStmt(s) => s.body.list.iter().find_map(|c| match c {
            Stmt::CommClause(cc) => cc
                .comm
                .as_ref()
                .and_then(|comm| stmt_assigns_to(pass, comm, obj))
                .or_else(|| body_assigns_to(pass, &cc.body, obj)),
            _ => None,
        }),
        Stmt::LabeledStmt(l) => stmt_assigns_to(pass, &l.stmt, obj),
        _ => None,
    }
}

/// True if `obj` is read before (or on the RHS of) its first overwrite.
///
/// Covers SSA gaps for interface method receivers: `p = p.M()`. Post-overwrite
/// uses of the same ObjectId (e.g. `ctx, _ = …; use(ctx)`) must not suppress.
fn body_has_pre_overwrite_value_use(pass: &Pass<'_>, body: &[Stmt], obj: ObjectId) -> bool {
    use guff::walk::preorder;

    // Pass 1: earliest assign to `obj`, and whether that assign's RHS reads it.
    let mut first_assign: Option<u32> = None;
    let mut rhs_of_first_uses = false;
    for stmt in body {
        preorder(stmt_ref(stmt), |n| {
            let NodeRef::AssignStmt(a) = n else {
                return true;
            };
            for e in &a.lhs {
                let Expr::Ident(id) = e else { continue };
                if object_of(pass, id) != Some(obj) {
                    continue;
                }
                let pos = id.name_pos.0 as u32;
                match first_assign {
                    None => {
                        first_assign = Some(pos);
                        rhs_of_first_uses = a.rhs.iter().any(|r| refers_to(pass, r, obj));
                    }
                    Some(prev) if pos < prev => {
                        first_assign = Some(pos);
                        rhs_of_first_uses = a.rhs.iter().any(|r| refers_to(pass, r, obj));
                    }
                    _ => {}
                }
            }
            true
        });
    }
    let Some(first_pos) = first_assign else {
        return false;
    };
    if rhs_of_first_uses {
        return true;
    }

    // Pass 2: any value-use of `obj` strictly before that assign.
    let mut use_before = false;
    for stmt in body {
        preorder(stmt_ref(stmt), |n| {
            let NodeRef::Ident(id) = n else {
                return true;
            };
            if object_of(pass, id) != Some(obj) {
                return true;
            }
            if let Some(info) = pass.types_info() {
                if !info.uses.contains_key(&id.id) {
                    return true;
                }
            }
            if (id.name_pos.0 as u32) < first_pos {
                use_before = true;
            }
            true
        });
    }
    use_before
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4009 requires inspect analyzer".to_string())?
        .clone();
    let Some(ir) = pass.result_of::<buildir::BuildIrResult>(buildir::analyzer()) else {
        return Ok(None);
    };

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |node| {
        let (params, body) = match node {
            NodeRef::FuncDecl(fd) => (
                fd.ty.params.as_ref().map(|p| &p.list),
                fd.body.as_ref().map(|b| b.list.as_slice()),
            ),
            NodeRef::FuncLit(fl) => (
                fl.ty.params.as_ref().map(|p| &p.list),
                Some(fl.body.list.as_slice()),
            ),
            _ => return,
        };
        let (Some(params), Some(body)) = (params, body) else {
            return;
        };
        for field in params {
            for arg in &field.names {
                if matches!(arg.name.as_str(), "_" | "") {
                    continue;
                }
                let Some(obj) = object_of(pass, arg) else {
                    continue;
                };

                // Locate the SSA Parameter for this type-checker Var.
                let Some((func, pid)) = ir.src_funcs_with_methods().iter().find_map(|&fid| {
                    let f = ir.prog.functions.get(fid);
                    f.params
                        .iter()
                        .find(|(_, p)| p.object == Some(obj))
                        .map(|(pid, _)| (f, pid))
                }) else {
                    continue;
                };

                // Upstream: any non-DebugRef referrer means the param value was
                // used (after lifting removes a dead spill Store).
                if has_non_debug_referrer(referrers(func, Value::Param(pid)), func) {
                    continue;
                }
                // SSA may miss interface-method uses of the param; AST bailout
                // only counts uses before / on the RHS of the first overwrite.
                if body_has_pre_overwrite_value_use(pass, body, obj) {
                    continue;
                }

                if let Some(assign_pos) = body_assigns_to(pass, body, obj) {
                    pending.push((
                        arg.name_pos.0 as u32,
                        format!("argument {} is overwritten before first use", arg.name),
                        assign_pos,
                        format!("assignment to {}", arg.name),
                    ));
                }
            }
        }
    });
    for (pos, msg, related_pos, related_msg) in pending {
        // `report.Related(assignment, "assignment to x")` — golangci renders it
        // as a second issue, `SA4009(related information): …`.
        pass.report(Diagnostic {
            pos,
            message: msg,
            related: vec![RelatedInformation {
                pos: related_pos,
                end: related_pos,
                message: related_msg,
            }],
            ..Default::default()
        });
    }
    Ok(None)
}

fn sa4009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4009",
        doc: "a function argument is overwritten before its first use",
        url: "https://staticcheck.dev/docs/checks/#SA4009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer(), buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
