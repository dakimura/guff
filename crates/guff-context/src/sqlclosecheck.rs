//! Port of [`github.com/ryanrolds/sqlclosecheck`](https://github.com/ryanrolds/sqlclosecheck)
//! (golangci-lint uses the `defer-only` analyzer).
//!
//! Checks that `sql.Rows` / `sql.Stmt` / `sqlx.NamedStmt` / pgx Rows are closed,
//! and that `Close` uses `defer`.
//!
//! Upstream uses `buildssa`. This port is an **AST / intra-procedural
//! approximation**: track named target assignments in a function body and
//! require a subsequent `.Close()` (including in deferred no-arg closures).
//! Non-deferred `Close` reports `"Close should use defer"`. Functions that
//! return a target type are skipped. Passing the value as a call argument
//! counts as handled (upstream `actionPassed` when last use).
//!
//! Built-in packages: `database/sql`, `github.com/jmoiron/sqlx`,
//! `github.com/jackc/pgx/v5`, `github.com/jackc/pgx/v5/pgxpool`.
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture / FieldAddr /
//! MakeInterface / Store-into-struct parity.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FieldList, FuncType, ValueSpec};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::named::named_obj;
use guff_types::pointer::pointer_elem;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

const SQL_PACKAGES: &[&str] = &[
    "database/sql",
    "github.com/jmoiron/sqlx",
    "github.com/jackc/pgx/v5",
    "github.com/jackc/pgx/v5/pgxpool",
];

const TARGET_TYPE_NAMES: &[&str] = &["Rows", "Stmt", "NamedStmt"];
const CLOSE_METHOD: &str = "Close";
const MSG_NOT_CLOSED: &str = "Rows/Stmt/NamedStmt was not closed";
const MSG_USE_DEFER: &str = "Close should use defer";

fn cut_vendor(path: &str) -> &str {
    if let Some(idx) = path.rfind("/vendor/") {
        &path[idx + "/vendor/".len()..]
    } else if let Some(rest) = path.strip_prefix("vendor/") {
        rest
    } else {
        path
    }
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_target_type(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let named_typ = match artifacts.types.get(typ) {
        TypeData::Pointer(_) => {
            let elem = pointer_elem(&artifacts.types, typ);
            unalias_readonly(&artifacts.types, elem)
        }
        _ => typ,
    };
    let TypeData::Named(_) = artifacts.types.get(named_typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, named_typ);
    let name = obj.name(&artifacts.objects);
    if !TARGET_TYPE_NAMES.contains(&name) {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let path = cut_vendor(artifacts.packages.get(pkg_id).path());
    SQL_PACKAGES.contains(&path)
}

fn expr_is_target(pass: &Pass<'_>, expr: &Expr) -> bool {
    type_of(pass, expr).is_some_and(|t| is_target_type(pass, t))
}

fn rhs_result_is_target(pass: &Pass<'_>, assign: &AssignStmt, lhs_index: usize) -> bool {
    if assign.rhs.len() == 1 {
        let Some(typ) = type_of(pass, &assign.rhs[0]) else {
            return false;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        let typ = unalias_readonly(&artifacts.types, typ);
        if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
            if lhs_index >= tuple_len(&artifacts.types, Some(typ)) {
                return false;
            }
            let elem = tuple_at(&artifacts.types, typ, lhs_index);
            let Some(elem_typ) = elem.typ(&artifacts.objects) else {
                return false;
            };
            return is_target_type(pass, elem_typ);
        }
        return lhs_index == 0 && is_target_type(pass, typ);
    }
    assign
        .rhs
        .get(lhs_index)
        .is_some_and(|e| expr_is_target(pass, e))
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn type_expr_looks_like_target(expr: &Expr) -> bool {
    match expr {
        Expr::StarExpr(s) => type_expr_looks_like_target(&s.x),
        Expr::ParenExpr(p) => type_expr_looks_like_target(&p.x),
        Expr::SelectorExpr(sel) => TARGET_TYPE_NAMES.contains(&sel.sel.name.as_str()),
        Expr::Ident(id) => TARGET_TYPE_NAMES.contains(&id.name.as_str()),
        _ => false,
    }
}

fn field_list_has_target(fields: Option<&FieldList>) -> bool {
    let Some(fl) = fields else {
        return false;
    };
    for field in &fl.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        if type_expr_looks_like_target(ty) {
            return true;
        }
    }
    false
}

fn func_returns_target(ty: &FuncType) -> bool {
    field_list_has_target(ty.results.as_ref())
}

struct SqlUsage {
    pos: u32,
    closed: bool,
    deferred: bool,
    passed: bool,
    close_pos: Option<u32>,
}

impl SqlUsage {
    fn report(self, pending: &mut Vec<(u32, String)>) {
        if self.passed {
            return;
        }
        if self.closed {
            if !self.deferred {
                let pos = self.close_pos.unwrap_or(self.pos);
                pending.push((pos, MSG_USE_DEFER.to_string()));
            }
            return;
        }
        pending.push((self.pos, MSG_NOT_CLOSED.to_string()));
    }
}

fn assign_report_pos(assign: &AssignStmt, lhs_index: usize) -> u32 {
    if assign.rhs.len() == 1 {
        if let Expr::CallExpr(call) = &assign.rhs[0] {
            return call.pos().0 as u32;
        }
        return assign.rhs[0].pos().0 as u32;
    }
    if let Some(rhs) = assign.rhs.get(lhs_index) {
        if let Expr::CallExpr(call) = rhs {
            return call.pos().0 as u32;
        }
        return rhs.pos().0 as u32;
    }
    assign
        .lhs
        .get(lhs_index)
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

/// `x.Close()` → Some("x")
fn close_var(call: &CallExpr) -> Option<&str> {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return None;
    };
    if sel.sel.name != CLOSE_METHOD {
        return None;
    }
    ident_name(&sel.x)
}

fn mark_close(call: &CallExpr, deferred: bool, usages: &mut HashMap<String, SqlUsage>) {
    if let Some(var) = close_var(call) {
        if let Some(u) = usages.get_mut(var) {
            u.closed = true;
            if deferred {
                u.deferred = true;
            }
            u.close_pos = Some(call.pos().0 as u32);
        }
    }
}

fn mark_passed_args(call: &CallExpr, usages: &mut HashMap<String, SqlUsage>) {
    // Skip Close itself — handled by mark_close.
    if close_var(call).is_some() {
        return;
    }
    for arg in &call.args {
        if let Some(name) = ident_name(arg) {
            if let Some(u) = usages.get_mut(name) {
                u.passed = true;
            }
        }
    }
}

fn handle_defer_close(call: &CallExpr, usages: &mut HashMap<String, SqlUsage>) {
    match call.fun.as_ref() {
        Expr::SelectorExpr(_) => mark_close(call, true, usages),
        Expr::FuncLit(fun) => {
            if fun.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
                return;
            }
            inspect(NodeRef::BlockStmt(&fun.body), |n| {
                let Some(n) = n else {
                    return true;
                };
                if matches!(n, NodeRef::FuncLit(_)) {
                    return false;
                }
                if let NodeRef::CallExpr(c) = n {
                    mark_close(c, true, usages);
                }
                true
            });
        }
        _ => {}
    }
}

fn check_body(pass: &Pass<'_>, body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    let mut usages: HashMap<String, SqlUsage> = HashMap::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                handle_assign(pass, assign, &mut usages, pending);
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                mark_close(call, false, &mut usages);
                mark_passed_args(call, &mut usages);
            }
            NodeRef::DeferStmt(d) => {
                handle_defer_close(&d.call, &mut usages);
            }
            NodeRef::ReturnStmt(ret) => {
                // Returning a tracked value clears it (ownership transferred).
                for result in &ret.results {
                    if let Some(name) = ident_name(result) {
                        usages.remove(name);
                    }
                }
            }
            NodeRef::ExprStmt(es) => {
                // Discarded target from a bare call: report immediately.
                if let Expr::CallExpr(call) = &es.x {
                    if let Some(typ) = type_of(pass, &es.x) {
                        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                            return true;
                        };
                        let typ = unalias_readonly(&artifacts.types, typ);
                        let is_tgt = if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
                            (0..tuple_len(&artifacts.types, Some(typ))).any(|i| {
                                let elem = tuple_at(&artifacts.types, typ, i);
                                elem.typ(&artifacts.objects)
                                    .is_some_and(|t| is_target_type(pass, t))
                            })
                        } else {
                            is_target_type(pass, typ)
                        };
                        if is_tgt {
                            pending.push((call.pos().0 as u32, MSG_NOT_CLOSED.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
        true
    });

    for (_, u) in usages {
        u.report(pending);
    }
}

fn handle_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    usages: &mut HashMap<String, SqlUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            continue;
        };
        if name == "_" {
            continue;
        }

        let is_tgt = expr_is_target(pass, lhs) || rhs_result_is_target(pass, assign, i);

        if let Some(prev) = usages.remove(name) {
            prev.report(pending);
        }

        if is_tgt {
            usages.insert(
                name.to_string(),
                SqlUsage {
                    pos: assign_report_pos(assign, i),
                    closed: false,
                    deferred: false,
                    passed: false,
                    close_pos: None,
                },
            );
        }
    }
}

fn handle_value_spec(
    pass: &Pass<'_>,
    spec: &ValueSpec,
    usages: &mut HashMap<String, SqlUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    if spec.values.is_empty() {
        return;
    }
    for (i, name_id) in spec.names.iter().enumerate() {
        let name = name_id.name.as_str();
        if name == "_" {
            continue;
        }

        let is_tgt = if spec.values.len() == 1 {
            let Some(typ) = type_of(pass, &spec.values[0]) else {
                continue;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                continue;
            };
            let typ = unalias_readonly(&artifacts.types, typ);
            if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
                if i >= tuple_len(&artifacts.types, Some(typ)) {
                    continue;
                }
                let elem = tuple_at(&artifacts.types, typ, i);
                let Some(elem_typ) = elem.typ(&artifacts.objects) else {
                    continue;
                };
                is_target_type(pass, elem_typ)
            } else {
                i == 0 && is_target_type(pass, typ)
            }
        } else {
            spec.values
                .get(i)
                .is_some_and(|e| expr_is_target(pass, e))
        };

        if let Some(prev) = usages.remove(name) {
            prev.report(pending);
        }

        if is_tgt {
            let pos = if spec.values.len() == 1 {
                if let Expr::CallExpr(call) = &spec.values[0] {
                    call.pos().0 as u32
                } else {
                    spec.values[0].pos().0 as u32
                }
            } else {
                spec.values
                    .get(i)
                    .map(|e| e.pos().0 as u32)
                    .unwrap_or(name_id.pos().0 as u32)
            };
            usages.insert(
                name.to_string(),
                SqlUsage {
                    pos,
                    closed: false,
                    deferred: false,
                    passed: false,
                    close_pos: None,
                },
            );
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "sqlclosecheck requires inspect analyzer".to_string())?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if func_returns_target(&fd.ty) {
                        return true;
                    }
                    if let Some(body) = &fd.body {
                        check_body(pass, body, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_target(&fl.ty) {
                        return true;
                    }
                    check_body(pass, &fl.body, &mut pending);
                }
                _ => {}
            }
            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "sqlclosecheck",
        doc: "Checks that sql.Rows, sql.Stmt, sqlx.NamedStmt, pgx.Query are closed.",
        url: "https://github.com/ryanrolds/sqlclosecheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect_pass::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyzer_metadata() {
        let a = analyzer();
        assert_eq!(a.name, "sqlclosecheck");
        assert!(!a.doc.is_empty());
    }
}
