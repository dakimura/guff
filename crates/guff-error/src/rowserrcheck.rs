//! Port of [`github.com/jingyugao/rowserrcheck`](https://github.com/jingyugao/rowserrcheck)
//! (golangci fork: `github.com/golangci/rowserrcheck`).
//!
//! Checks that `Rows.Err()` is called after obtaining `*database/sql.Rows`
//! (and optionally other packages via `linters.settings.rowserrcheck.packages`).
//!
//! Upstream uses `buildssa`. This port is an **AST / intra-procedural
//! approximation**: track named `Rows` assignments in a function body and
//! require a subsequent `.Err()` (including in deferred no-arg closures).
//! Functions that return `*…Rows` are skipped (upstream parity).
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture / FieldAddr parity.

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

use crate::util::type_of;

const DEFAULT_SQL_PKG: &str = "database/sql";
const ROWS_NAME: &str = "Rows";
const ERR_METHOD: &str = "Err";
const MSG: &str = "rows.Err must be checked";

/// Pass-time options from `linters.settings.rowserrcheck`.
///
/// `database/sql` is always checked; `packages` lists additional import paths
/// (e.g. `github.com/jmoiron/sqlx`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowserrcheckOptions {
    pub packages: Vec<String>,
}

fn effective_packages(opts: &RowserrcheckOptions) -> Vec<String> {
    let mut pkgs = vec![DEFAULT_SQL_PKG.to_string()];
    for p in &opts.packages {
        if !pkgs.iter().any(|x| x == p) {
            pkgs.push(p.clone());
        }
    }
    pkgs
}

fn cut_vendor(path: &str) -> &str {
    if let Some(idx) = path.rfind("/vendor/") {
        &path[idx + "/vendor/".len()..]
    } else if let Some(rest) = path.strip_prefix("vendor/") {
        rest
    } else {
        path
    }
}

fn is_rows_named(pass: &Pass<'_>, typ: TypeId, packages: &[String]) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    // Prefer *Rows (database/sql); also accept bare Rows / interface Rows.
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
    if obj.name(&artifacts.objects) != ROWS_NAME {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let path = cut_vendor(artifacts.packages.get(pkg_id).path());
    packages.iter().any(|p| p == path)
}

fn type_is_rows(pass: &Pass<'_>, typ: TypeId, packages: &[String]) -> bool {
    is_rows_named(pass, typ, packages)
}

fn expr_is_rows(pass: &Pass<'_>, expr: &Expr, packages: &[String]) -> bool {
    type_of(pass, expr).is_some_and(|t| type_is_rows(pass, t, packages))
}

/// Whether the i-th LHS of a multi-value RHS is a Rows result.
fn rhs_result_is_rows(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    lhs_index: usize,
    packages: &[String],
) -> bool {
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
            return type_is_rows(pass, elem_typ, packages);
        }
        return lhs_index == 0 && type_is_rows(pass, typ, packages);
    }
    assign
        .rhs
        .get(lhs_index)
        .is_some_and(|e| expr_is_rows(pass, e, packages))
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn type_expr_looks_like_rows(expr: &Expr) -> bool {
    match expr {
        Expr::StarExpr(s) => type_expr_looks_like_rows(&s.x),
        Expr::ParenExpr(p) => type_expr_looks_like_rows(&p.x),
        Expr::SelectorExpr(sel) => sel.sel.name == ROWS_NAME,
        Expr::Ident(id) => id.name == ROWS_NAME,
        _ => false,
    }
}

fn field_list_has_rows(_pass: &Pass<'_>, fields: Option<&FieldList>, _packages: &[String]) -> bool {
    let Some(fl) = fields else {
        return false;
    };
    for field in &fl.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        // Signature type exprs often lack Types map entries; match AST shape.
        if type_expr_looks_like_rows(ty) {
            return true;
        }
    }
    false
}

fn func_returns_rows(pass: &Pass<'_>, ty: &FuncType, packages: &[String]) -> bool {
    field_list_has_rows(pass, ty.results.as_ref(), packages)
}

struct RowsUsage {
    pos: u32,
}

impl RowsUsage {
    fn report(self, pending: &mut Vec<(u32, String)>) {
        pending.push((self.pos, MSG.to_string()));
    }
}

fn assign_report_pos(assign: &AssignStmt, lhs_index: usize) -> u32 {
    // Prefer the RHS call that produced Rows (upstream reports at the call).
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

fn check_body(pass: &Pass<'_>, body: &BlockStmt, packages: &[String], pending: &mut Vec<(u32, String)>) {
    let mut usages: HashMap<String, RowsUsage> = HashMap::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        // Nested FuncLit bodies are separate units (handled by file-level walk).
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                handle_assign(pass, assign, packages, &mut usages, pending);
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, packages, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                handle_err_call(call, &mut usages);
            }
            NodeRef::DeferStmt(d) => {
                handle_defer_err(&d.call, &mut usages);
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
    packages: &[String],
    usages: &mut HashMap<String, RowsUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            continue;
        };
        if name == "_" {
            continue;
        }

        let is_rows =
            expr_is_rows(pass, lhs, packages) || rhs_result_is_rows(pass, assign, i, packages);

        if let Some(prev) = usages.remove(name) {
            prev.report(pending);
        }

        if is_rows {
            usages.insert(
                name.to_string(),
                RowsUsage {
                    pos: assign_report_pos(assign, i),
                },
            );
        }
    }
}

fn handle_value_spec(
    pass: &Pass<'_>,
    spec: &ValueSpec,
    packages: &[String],
    usages: &mut HashMap<String, RowsUsage>,
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

        let is_rows = if spec.values.len() == 1 {
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
                type_is_rows(pass, elem_typ, packages)
            } else {
                i == 0 && type_is_rows(pass, typ, packages)
            }
        } else {
            spec.values
                .get(i)
                .is_some_and(|e| expr_is_rows(pass, e, packages))
        };

        if let Some(prev) = usages.remove(name) {
            prev.report(pending);
        }

        if is_rows {
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
            usages.insert(name.to_string(), RowsUsage { pos });
        }
    }
}

fn handle_err_call(call: &CallExpr, usages: &mut HashMap<String, RowsUsage>) {
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return;
    };
    if sel.sel.name != ERR_METHOD {
        return;
    }
    let Some(var_name) = ident_name(&sel.x) else {
        return;
    };
    usages.remove(var_name);
}

fn handle_defer_err(call: &CallExpr, usages: &mut HashMap<String, RowsUsage>) {
    match call.fun.as_ref() {
        Expr::SelectorExpr(sel) => {
            if sel.sel.name != ERR_METHOD {
                return;
            }
            let Some(var_name) = ident_name(&sel.x) else {
                return;
            };
            usages.remove(var_name);
        }
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
                    handle_err_call(c, usages);
                }
                true
            });
        }
        _ => {}
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "rowserrcheck requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<RowserrcheckOptions>("rowserrcheck")
        .cloned()
        .unwrap_or_default();
    let packages = effective_packages(&options);

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if func_returns_rows(pass, &fd.ty, &packages) {
                        return true;
                    }
                    if let Some(body) = &fd.body {
                        check_body(pass, body, &packages, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_rows(pass, &fl.ty, &packages) {
                        return true;
                    }
                    check_body(pass, &fl.body, &packages, &mut pending);
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
        name: "rowserrcheck",
        doc: "Checks whether Rows.Err is checked",
        url: "https://github.com/jingyugao/rowserrcheck",
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
        assert_eq!(a.name, "rowserrcheck");
        assert!(!a.doc.is_empty());
    }

    #[test]
    fn effective_packages_always_includes_database_sql() {
        let opts = RowserrcheckOptions {
            packages: vec!["github.com/jmoiron/sqlx".into()],
        };
        let pkgs = effective_packages(&opts);
        assert_eq!(pkgs[0], DEFAULT_SQL_PKG);
        assert!(pkgs.iter().any(|p| p == "github.com/jmoiron/sqlx"));
    }
}
