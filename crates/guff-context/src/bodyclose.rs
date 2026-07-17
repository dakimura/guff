//! Port of [`github.com/timakin/bodyclose`](https://github.com/timakin/bodyclose)
//! (golangci-lint wrapper in `pkg/golinters/bodyclose`).
//!
//! Checks that `*net/http.Response` bodies are closed (`resp.Body.Close()`).
//!
//! Upstream uses `buildssa`. This port is an **AST / intra-procedural
//! approximation**: track named `*http.Response` assignments in a function
//! body and require a subsequent `.Body.Close()` (including in deferred
//! no-arg closures). Functions that return `*http.Response` are skipped
//! (upstream parity). `httptest.ResponseRecorder.Result` is skipped.
//!
//! Settings: `linters.settings.bodyclose.check-consumption` (default false).
//! When true, also require a known consumption call (`io.Copy` / `ReadAll` /
//! `json.NewDecoder` / `bufio.NewScanner`/`NewReader`) on the same body.
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture / FieldAddr /
//! `io.Closer` ChangeInterface / Return-as-ReadCloser parity.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FieldList, FuncType, ValueSpec};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::TypeData;
use guff_types::named::named_obj;
use guff_types::pointer::pointer_elem;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

const HTTP_PKG: &str = "net/http";
const HTTPTST_PKG: &str = "net/http/httptest";
const RESPONSE_NAME: &str = "Response";
const BODY_FIELD: &str = "Body";
const CLOSE_METHOD: &str = "Close";
const MSG_CLOSE: &str = "response body must be closed";
const MSG_CLOSE_AND_CONSUME: &str = "response body must be closed and consumed";

/// Pass-time options from `linters.settings.bodyclose`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BodycloseOptions {
    /// When true, require both Close and a known consumption call.
    pub check_consumption: bool,
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

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn is_http_response_ptr(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Pointer(_) = artifacts.types.get(typ) else {
        return false;
    };
    let elem = unalias_readonly(&artifacts.types, pointer_elem(&artifacts.types, typ));
    let TypeData::Named(_) = artifacts.types.get(elem) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, elem);
    if obj.name(&artifacts.objects) != RESPONSE_NAME {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    cut_vendor(artifacts.packages.get(pkg_id).path()) == HTTP_PKG
}

fn expr_is_response(pass: &Pass<'_>, expr: &Expr) -> bool {
    type_of(pass, expr).is_some_and(|t| is_http_response_ptr(pass, t))
}

fn rhs_result_is_response(pass: &Pass<'_>, assign: &AssignStmt, lhs_index: usize) -> bool {
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
            return is_http_response_ptr(pass, elem_typ);
        }
        return lhs_index == 0 && is_http_response_ptr(pass, typ);
    }
    assign
        .rhs
        .get(lhs_index)
        .is_some_and(|e| expr_is_response(pass, e))
}

fn is_httptest_result_call(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Expr::CallExpr(call) = expr else {
        return false;
    };
    let Expr::SelectorExpr(sel) = call.fun.as_ref() else {
        return false;
    };
    if sel.sel.name != "Result" {
        return false;
    }
    let Some(recv_typ) = type_of(pass, &sel.x) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut typ = unalias_readonly(&artifacts.types, recv_typ);
    if matches!(artifacts.types.get(typ), TypeData::Pointer(_)) {
        typ = unalias_readonly(&artifacts.types, pointer_elem(&artifacts.types, typ));
    }
    let TypeData::Named(_) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = named_obj(&artifacts.types, typ);
    if obj.name(&artifacts.objects) != "ResponseRecorder" {
        return false;
    }
    let Some(pkg_id) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    cut_vendor(artifacts.packages.get(pkg_id).path()) == HTTPTST_PKG
}

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn type_expr_looks_like_response(expr: &Expr) -> bool {
    match expr {
        Expr::StarExpr(s) => match s.x.as_ref() {
            Expr::SelectorExpr(sel) => sel.sel.name == RESPONSE_NAME,
            Expr::Ident(id) => id.name == RESPONSE_NAME,
            Expr::ParenExpr(p) => type_expr_looks_like_response(&p.x),
            _ => false,
        },
        Expr::ParenExpr(p) => type_expr_looks_like_response(&p.x),
        Expr::SelectorExpr(sel) => sel.sel.name == RESPONSE_NAME,
        Expr::Ident(id) => id.name == RESPONSE_NAME,
        _ => false,
    }
}

fn field_list_has_response(fields: Option<&FieldList>) -> bool {
    let Some(fl) = fields else {
        return false;
    };
    for field in &fl.list {
        let Some(ty) = &field.ty else {
            continue;
        };
        if type_expr_looks_like_response(ty) {
            return true;
        }
    }
    false
}

fn func_returns_response(ty: &FuncType) -> bool {
    field_list_has_response(ty.results.as_ref())
}

struct RespUsage {
    pos: u32,
    closed: bool,
    consumed: bool,
}

impl RespUsage {
    fn report(self, check_consumption: bool, pending: &mut Vec<(u32, String)>) {
        let ok = if check_consumption {
            self.closed && self.consumed
        } else {
            self.closed
        };
        if !ok {
            let msg = if check_consumption {
                MSG_CLOSE_AND_CONSUME
            } else {
                MSG_CLOSE
            };
            pending.push((self.pos, msg.to_string()));
        }
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

/// `x.Body.Close()` → Some("x")
fn body_close_var(call: &CallExpr) -> Option<&str> {
    let Expr::SelectorExpr(close_sel) = call.fun.as_ref() else {
        return None;
    };
    if close_sel.sel.name != CLOSE_METHOD {
        return None;
    }
    let Expr::SelectorExpr(body_sel) = close_sel.x.as_ref() else {
        return None;
    };
    if body_sel.sel.name != BODY_FIELD {
        return None;
    }
    ident_name(&body_sel.x)
}

/// `resp.Body` → Some("resp")
fn body_field_var(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::SelectorExpr(sel) if sel.sel.name == BODY_FIELD => ident_name(&sel.x),
        Expr::ParenExpr(p) => body_field_var(&p.x),
        _ => None,
    }
}

fn is_consumption_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(fq) = code::call_name(pass, &call.fun) else {
        return false;
    };
    matches!(
        fq.as_str(),
        "io.Copy"
            | "io.ReadAll"
            | "io/ioutil.ReadAll"
            | "encoding/json.NewDecoder"
            | "bufio.NewScanner"
            | "bufio.NewReader"
    )
}

fn mark_consumption(pass: &Pass<'_>, call: &CallExpr, usages: &mut HashMap<String, RespUsage>) {
    if !is_consumption_call(pass, call) {
        return;
    }
    for arg in &call.args {
        if let Some(var) = body_field_var(arg) {
            if let Some(u) = usages.get_mut(var) {
                u.consumed = true;
            }
        }
    }
}

fn mark_close(call: &CallExpr, usages: &mut HashMap<String, RespUsage>) {
    if let Some(var) = body_close_var(call) {
        if let Some(u) = usages.get_mut(var) {
            u.closed = true;
        }
    }
}

fn handle_defer_close(call: &CallExpr, usages: &mut HashMap<String, RespUsage>) {
    match call.fun.as_ref() {
        Expr::SelectorExpr(_) => mark_close(call, usages),
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
                    mark_close(c, usages);
                }
                true
            });
        }
        _ => {}
    }
}

fn check_body(
    pass: &Pass<'_>,
    body: &BlockStmt,
    check_consumption: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let mut usages: HashMap<String, RespUsage> = HashMap::new();

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if matches!(n, NodeRef::FuncLit(_)) {
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                handle_assign(pass, assign, check_consumption, &mut usages, pending);
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, check_consumption, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                mark_close(call, &mut usages);
                if check_consumption {
                    mark_consumption(pass, call, &mut usages);
                }
            }
            NodeRef::DeferStmt(d) => {
                handle_defer_close(&d.call, &mut usages);
                if check_consumption {
                    // defer io.Copy(...) is unusual; still scan nested calls.
                    if let Expr::FuncLit(fun) = d.call.fun.as_ref() {
                        if !fun.ty.params.as_ref().is_some_and(|p| !p.list.is_empty()) {
                            inspect(NodeRef::BlockStmt(&fun.body), |n| {
                                let Some(n) = n else {
                                    return true;
                                };
                                if matches!(n, NodeRef::FuncLit(_)) {
                                    return false;
                                }
                                if let NodeRef::CallExpr(c) = n {
                                    mark_consumption(pass, c, &mut usages);
                                }
                                true
                            });
                        }
                    }
                }
            }
            NodeRef::ExprStmt(es) => {
                // Discarded *http.Response from a bare call: report immediately.
                if let Expr::CallExpr(call) = &es.x {
                    if let Some(typ) = type_of(pass, &es.x) {
                        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                            return true;
                        };
                        let typ = unalias_readonly(&artifacts.types, typ);
                        let is_resp = if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
                            (0..tuple_len(&artifacts.types, Some(typ))).any(|i| {
                                let elem = tuple_at(&artifacts.types, typ, i);
                                elem.typ(&artifacts.objects)
                                    .is_some_and(|t| is_http_response_ptr(pass, t))
                            })
                        } else {
                            is_http_response_ptr(pass, typ)
                        };
                        if is_resp && !is_httptest_result_call(pass, &es.x) {
                            let msg = if check_consumption {
                                MSG_CLOSE_AND_CONSUME
                            } else {
                                MSG_CLOSE
                            };
                            pending.push((call.pos().0 as u32, msg.to_string()));
                        }
                    }
                }
            }
            _ => {}
        }
        true
    });

    for (_, u) in usages {
        u.report(check_consumption, pending);
    }
}

fn handle_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    check_consumption: bool,
    usages: &mut HashMap<String, RespUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            continue;
        };
        if name == "_" {
            continue;
        }

        let skip_httptest = assign
            .rhs
            .first()
            .is_some_and(|e| is_httptest_result_call(pass, e))
            || assign
                .rhs
                .get(i)
                .is_some_and(|e| is_httptest_result_call(pass, e));

        let is_resp = !skip_httptest
            && (expr_is_response(pass, lhs) || rhs_result_is_response(pass, assign, i));

        if let Some(prev) = usages.remove(name) {
            prev.report(check_consumption, pending);
        }

        if is_resp {
            usages.insert(
                name.to_string(),
                RespUsage {
                    pos: assign_report_pos(assign, i),
                    closed: false,
                    consumed: false,
                },
            );
        }
    }
}

fn handle_value_spec(
    pass: &Pass<'_>,
    spec: &ValueSpec,
    check_consumption: bool,
    usages: &mut HashMap<String, RespUsage>,
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

        let skip_httptest = spec
            .values
            .first()
            .is_some_and(|e| is_httptest_result_call(pass, e))
            || spec
                .values
                .get(i)
                .is_some_and(|e| is_httptest_result_call(pass, e));

        let is_resp = if skip_httptest {
            false
        } else if spec.values.len() == 1 {
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
                is_http_response_ptr(pass, elem_typ)
            } else {
                i == 0 && is_http_response_ptr(pass, typ)
            }
        } else {
            spec.values
                .get(i)
                .is_some_and(|e| expr_is_response(pass, e))
        };

        if let Some(prev) = usages.remove(name) {
            prev.report(check_consumption, pending);
        }

        if is_resp {
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
                RespUsage {
                    pos,
                    closed: false,
                    consumed: false,
                },
            );
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "bodyclose requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<BodycloseOptions>("bodyclose")
        .cloned()
        .unwrap_or_default();
    let check_consumption = options.check_consumption;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if func_returns_response(&fd.ty) {
                        return true;
                    }
                    if let Some(body) = &fd.body {
                        check_body(pass, body, check_consumption, &mut pending);
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_response(&fl.ty) {
                        return true;
                    }
                    check_body(pass, &fl.body, check_consumption, &mut pending);
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
        name: "bodyclose",
        doc: "checks whether HTTP response body is closed successfully",
        url: "https://github.com/timakin/bodyclose",
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
        assert_eq!(a.name, "bodyclose");
        assert!(!a.doc.is_empty());
    }

    #[test]
    fn default_options_off() {
        assert!(!BodycloseOptions::default().check_consumption);
    }
}
