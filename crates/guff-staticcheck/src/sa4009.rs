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
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::{buildir, inspect};
use guff_analysis::{has_non_debug_referrer, referrers, AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::value::Value;
use guff_types::ObjectId;

/// True if `body` contains an assignment whose LHS is `obj` (upstream's
/// `ast.Inspect` over AssignStmt).
fn body_assigns_to(pass: &Pass<'_>, body: &[Stmt], obj: ObjectId) -> bool {
    body.iter().any(|s| stmt_assigns_to(pass, s, obj))
}

fn stmt_assigns_to(pass: &Pass<'_>, stmt: &Stmt, obj: ObjectId) -> bool {
    match stmt {
        Stmt::AssignStmt(a) => a.lhs.iter().any(|e| {
            matches!(e, Expr::Ident(id) if object_of(pass, id) == Some(obj))
        }),
        Stmt::BlockStmt(b) => body_assigns_to(pass, &b.list, obj),
        Stmt::IfStmt(i) => {
            i.init
                .as_ref()
                .is_some_and(|s| stmt_assigns_to(pass, s, obj))
                || body_assigns_to(pass, &i.body.list, obj)
                || i.else_
                    .as_ref()
                    .is_some_and(|e| stmt_assigns_to(pass, e, obj))
        }
        Stmt::ForStmt(f) => {
            f.init
                .as_ref()
                .is_some_and(|s| stmt_assigns_to(pass, s, obj))
                || f.post
                    .as_ref()
                    .is_some_and(|s| stmt_assigns_to(pass, s, obj))
                || body_assigns_to(pass, &f.body.list, obj)
        }
        Stmt::RangeStmt(r) => body_assigns_to(pass, &r.body.list, obj),
        Stmt::SwitchStmt(s) => s.body.list.iter().any(|c| {
            matches!(c, Stmt::CaseClause(cc) if body_assigns_to(pass, &cc.body, obj))
        }),
        Stmt::TypeSwitchStmt(s) => {
            s.init
                .as_ref()
                .is_some_and(|i| stmt_assigns_to(pass, i, obj))
                || stmt_assigns_to(pass, &s.assign, obj)
                || s.body.list.iter().any(|c| {
                    matches!(c, Stmt::CaseClause(cc) if body_assigns_to(pass, &cc.body, obj))
                })
        }
        Stmt::SelectStmt(s) => s.body.list.iter().any(|c| match c {
            Stmt::CommClause(cc) => {
                cc.comm
                    .as_ref()
                    .is_some_and(|comm| stmt_assigns_to(pass, comm, obj))
                    || body_assigns_to(pass, &cc.body, obj)
            }
            _ => false,
        }),
        Stmt::LabeledStmt(l) => stmt_assigns_to(pass, &l.stmt, obj),
        _ => false,
    }
}

/// True if `obj` appears in a value position (not solely as a plain assign LHS).
/// Covers SSA gaps for interface method receivers: `p = p.M()`.
fn body_has_value_use(pass: &Pass<'_>, body: &[Stmt], obj: ObjectId) -> bool {
    body.iter().any(|s| stmt_has_value_use(pass, s, obj))
}

fn stmt_has_value_use(pass: &Pass<'_>, stmt: &Stmt, obj: ObjectId) -> bool {
    match stmt {
        Stmt::AssignStmt(a) => a.rhs.iter().any(|e| refers_to(pass, e, obj)),
        Stmt::IncDecStmt(i) => refers_to(pass, &i.x, obj),
        Stmt::ExprStmt(e) => refers_to(pass, &e.x, obj),
        Stmt::ReturnStmt(r) => r.results.iter().any(|e| refers_to(pass, e, obj)),
        Stmt::SendStmt(s) => {
            refers_to(pass, &s.chan_, obj) || refers_to(pass, &s.value, obj)
        }
        Stmt::GoStmt(g) => {
            refers_to(pass, &g.call.fun, obj) || g.call.args.iter().any(|a| refers_to(pass, a, obj))
        }
        Stmt::DeferStmt(d) => {
            refers_to(pass, &d.call.fun, obj) || d.call.args.iter().any(|a| refers_to(pass, a, obj))
        }
        Stmt::BlockStmt(b) => body_has_value_use(pass, &b.list, obj),
        Stmt::IfStmt(i) => {
            i.init
                .as_ref()
                .is_some_and(|s| stmt_has_value_use(pass, s, obj))
                || refers_to(pass, &i.cond, obj)
                || body_has_value_use(pass, &i.body.list, obj)
                || i.else_
                    .as_ref()
                    .is_some_and(|e| stmt_has_value_use(pass, e, obj))
        }
        Stmt::ForStmt(f) => {
            f.init
                .as_ref()
                .is_some_and(|s| stmt_has_value_use(pass, s, obj))
                || f.cond.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || f.post
                    .as_ref()
                    .is_some_and(|s| stmt_has_value_use(pass, s, obj))
                || body_has_value_use(pass, &f.body.list, obj)
        }
        Stmt::RangeStmt(r) => {
            refers_to(pass, &r.x, obj) || body_has_value_use(pass, &r.body.list, obj)
        }
        Stmt::SwitchStmt(s) => {
            s.init
                .as_ref()
                .is_some_and(|st| stmt_has_value_use(pass, st, obj))
                || s.tag.as_ref().is_some_and(|e| refers_to(pass, e, obj))
                || s.body.list.iter().any(|c| {
                    matches!(c, Stmt::CaseClause(cc) if body_has_value_use(pass, &cc.body, obj)
                        || cc.list.iter().any(|e| refers_to(pass, e, obj)))
                })
        }
        Stmt::TypeSwitchStmt(s) => {
            s.init
                .as_ref()
                .is_some_and(|i| stmt_has_value_use(pass, i, obj))
                || stmt_has_value_use(pass, &s.assign, obj)
                || s.body.list.iter().any(|c| {
                    matches!(c, Stmt::CaseClause(cc) if body_has_value_use(pass, &cc.body, obj))
                })
        }
        Stmt::SelectStmt(s) => s.body.list.iter().any(|c| match c {
            Stmt::CommClause(cc) => {
                cc.comm
                    .as_ref()
                    .is_some_and(|comm| stmt_has_value_use(pass, comm, obj))
                    || body_has_value_use(pass, &cc.body, obj)
            }
            _ => false,
        }),
        Stmt::LabeledStmt(l) => stmt_has_value_use(pass, &l.stmt, obj),
        _ => false,
    }
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
                let Some((func, pid)) = ir.src_funcs.iter().find_map(|&fid| {
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
                // SSA may miss interface-method uses of the param; AST bailout.
                if body_has_value_use(pass, body, obj) {
                    continue;
                }

                if body_assigns_to(pass, body, obj) {
                    pending.push((
                        arg.name_pos.0 as u32,
                        format!("argument {} is overwritten before first use", arg.name),
                    ));
                }
            }
        }
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
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
