//! Port of [`github.com/ClickHouse/clickhouse-go-linter`](https://github.com/ClickHouse/clickhouse-go-linter)
//! (golangci-lint wrapper in `pkg/golinters/clickhouselint`).
//!
//! Detects common mistakes with the ClickHouse native Go driver API
//! (`github.com/ClickHouse/clickhouse-go/v2`, `clickhouse.Open()` / `driver.Conn`):
//!
//! 1. **chrowserr** — `Rows.Next()` without a subsequent `Rows.Err()` in the
//!    same function (reassignment flushes prior tracking).
//! 2. **chbatchclose** — `driver.Batch` assigned without a defensive
//!    `defer batch.Close()` (or returning the batch to the caller). Blank
//!    assignment of a Batch is always reported (connection leak).
//!
//! Analysis is intra-procedural: nested `FuncLit` bodies are checked as
//! separate units (not descended into from the outer function). Deferred
//! closures that take parameters, or nest further FuncLits / goroutines for
//! `Close()`, are treated as non-defensive (upstream parity / false positives
//! on rare patterns).
//!
//! No settings keys. DEFERRED: `CH_GO_LINTER_DEBUG` valid-usage reports.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, DeferStmt, Expr, FuncType, ReturnStmt};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::named::named_obj;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

const DRIVER_PKG: &str = "github.com/ClickHouse/clickhouse-go/v2/lib/driver";

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

fn is_named_pkg_type(pass: &Pass<'_>, typ: TypeId, pkg_path: &str, name: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != name {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    cut_vendor(artifacts.packages.get(pkg_id).path()) == pkg_path
}

/// Port of `util.IsChObj`.
fn is_ch_obj(pass: &Pass<'_>, expr: &Expr, name: &str) -> bool {
    let Some(typ) = type_of(pass, expr) else {
        return false;
    };
    is_named_pkg_type(pass, typ, DRIVER_PKG, name)
}

/// Whether the i-th result of a multi-value RHS (typically a call) is a
/// ClickHouse named type. Used when TypeOf on blank `_` has no Types entry.
fn rhs_result_is_ch(pass: &Pass<'_>, assign: &AssignStmt, lhs_index: usize, name: &str) -> bool {
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
            return is_named_pkg_type(pass, elem_typ, DRIVER_PKG, name);
        }
        return lhs_index == 0 && is_named_pkg_type(pass, typ, DRIVER_PKG, name);
    }
    assign
        .rhs
        .get(lhs_index)
        .is_some_and(|e| is_ch_obj(pass, e, name))
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

struct RowsUsage {
    next_pos: u32,
}

impl RowsUsage {
    fn report(&self, var_name: &str, pending: &mut Vec<(u32, String)>) {
        pending.push((
            self.next_pos,
            format!("clickhouse {var_name}.Err() must be checked after {var_name}.Next()"),
        ));
    }
}

struct BatchUsage {
    assign_pos: u32,
    deferred_close: bool,
    returned: bool,
}

impl BatchUsage {
    fn report(&self, var_name: &str, pending: &mut Vec<(u32, String)>) {
        if !self.deferred_close && !self.returned {
            pending.push((
                self.assign_pos,
                format!(
                    "clickhouse Batch {var_name} must be closed defensively with defer {var_name}.Close() after successful instantiation"
                ),
            ));
        }
    }
}

fn check_rows_err(pass: &Pass<'_>, body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    let mut usages: HashMap<String, RowsUsage> = HashMap::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        // Nested closures are separate units (handled by the file-level walk).
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                for lhs in &assign.lhs {
                    let Some(name) = ident_name(lhs) else {
                        continue;
                    };
                    if let Some(s) = usages.remove(name) {
                        s.report(name, pending);
                    }
                }
            }
            NodeRef::CallExpr(call) => {
                handle_rows_call(pass, call, &mut usages);
            }
            _ => {}
        }
        true
    });

    for (var_name, s) in usages {
        s.report(&var_name, pending);
    }
}

fn handle_rows_call(pass: &Pass<'_>, call: &CallExpr, usages: &mut HashMap<String, RowsUsage>) {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    let Some(var_name) = ident_name(&sel.x) else {
        return;
    };
    if !is_ch_obj(pass, &sel.x, "Rows") {
        return;
    }
    match sel.sel.name.as_str() {
        "Next" => {
            usages
                .entry(var_name.to_string())
                .or_insert_with(|| RowsUsage {
                    next_pos: call.pos().0 as u32,
                });
        }
        "Err" => {
            usages.remove(var_name);
        }
        _ => {}
    }
}

fn check_batch_close(pass: &Pass<'_>, body: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    let mut usages: HashMap<String, BatchUsage> = HashMap::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                handle_batch_assign(pass, assign, &mut usages, pending);
            }
            NodeRef::DeferStmt(d) => {
                handle_defer(d, &mut usages);
            }
            NodeRef::ReturnStmt(r) => {
                handle_return(r, &mut usages);
            }
            _ => {}
        }
        true
    });

    for (var_name, u) in usages {
        u.report(&var_name, pending);
    }
}

fn assign_pos(assign: &AssignStmt) -> u32 {
    assign
        .lhs
        .first()
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

fn handle_batch_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    usages: &mut HashMap<String, BatchUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            continue;
        };

        let is_batch = is_ch_obj(pass, lhs, "Batch") || rhs_result_is_ch(pass, assign, i, "Batch");

        if name == "_" && is_batch {
            pending.push((
                assign_pos(assign),
                "clickhouse Batch assigned to blank identifier. Connection leak. clickhouse Batch must be instantiated and closed defensively with defer batch.Close() after successful instantiation".to_string(),
            ));
            continue;
        }

        if let Some(u) = usages.remove(name) {
            u.report(name, pending);
        }

        if is_batch && name != "_" {
            usages.insert(
                name.to_string(),
                BatchUsage {
                    assign_pos: assign_pos(assign),
                    deferred_close: false,
                    returned: false,
                },
            );
        }
    }
}

fn handle_defer(defer_stmt: &DeferStmt, usages: &mut HashMap<String, BatchUsage>) {
    let call = &defer_stmt.call;
    match call.fun.as_ref() {
        Expr::SelectorExpr(fun) => {
            let Some(var_name) = ident_name(&fun.x) else {
                return;
            };
            let Some(u) = usages.get_mut(var_name) else {
                return;
            };
            if fun.sel.name == "Close" {
                u.deferred_close = true;
            }
        }
        Expr::FuncLit(fun) => {
            if has_params(&fun.ty) {
                return;
            }
            handle_deferred_closure(&fun.body, usages);
        }
        _ => {}
    }
}

fn has_params(ty: &FuncType) -> bool {
    ty.params.as_ref().is_some_and(|p| !p.list.is_empty())
}

fn handle_deferred_closure(body: &BlockStmt, usages: &mut HashMap<String, BatchUsage>) {
    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }
        let NodeRef::CallExpr(call) = n else {
            return true;
        };
        let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
            return true;
        };
        if sel.sel.name != "Close" {
            return true;
        }
        let Some(var_name) = ident_name(&sel.x) else {
            return true;
        };
        if let Some(u) = usages.get_mut(var_name) {
            u.deferred_close = true;
        }
        true
    });
}

fn handle_return(ret: &ReturnStmt, usages: &mut HashMap<String, BatchUsage>) {
    for result in &ret.results {
        let Some(name) = ident_name(result) else {
            continue;
        };
        if let Some(u) = usages.get_mut(name) {
            u.returned = true;
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "clickhouselint requires inspect analyzer".to_string())?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if let Some(body) = &fd.body {
                        check_rows_err(pass, body, &mut pending);
                        check_batch_close(pass, body, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    check_rows_err(pass, &fl.body, &mut pending);
                    check_batch_close(pass, &fl.body, &mut pending);
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
        name: "clickhouselint",
        doc: "Detects common mistakes with the ClickHouse native Go driver API.",
        url: "https://github.com/ClickHouse/clickhouse-go-linter",
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
        assert_eq!(a.name, "clickhouselint");
        assert!(!a.doc.is_empty());
    }
}
