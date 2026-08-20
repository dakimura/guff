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
//! `return resp.Body` counts as closed — see [`mark_returned_body`].
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture / FieldAddr /
//! `io.Closer` ChangeInterface parity.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FieldList, FuncType, ReturnStmt, ValueSpec};
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

/// The right-hand side that feeds `lhs_index`: the single multi-value call, or
/// the expression in the same position.
fn rhs_for_index(assign: &AssignStmt, lhs_index: usize) -> Option<&Expr> {
    if assign.rhs.len() == 1 {
        assign.rhs.first()
    } else {
        assign.rhs.get(lhs_index)
    }
}

/// `make` and `new` are lowered by go/ssa to `MakeChan`/`MakeMap`/`MakeSlice`
/// and `Alloc`, so they are not `*ssa.Call` and `getReqCall` never sees them —
/// `resp := new(http.Response)` opens nothing.
fn is_make_or_new(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(call) => is_make_or_new_call(call),
        _ => false,
    }
}

fn is_make_or_new_call(call: &CallExpr) -> bool {
    matches!(call.fun.as_ref(), Expr::Ident(id) if id.name == "make" || id.name == "new")
}

fn is_call_expr(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => true,
        Expr::ParenExpr(p) => is_call_expr(&p.x),
        _ => false,
    }
}

fn assign_report_pos(assign: &AssignStmt, lhs_index: usize) -> u32 {
    if assign.rhs.len() == 1 {
        if let Expr::CallExpr(call) = &assign.rhs[0] {
            // go/ssa gives a call the position of its `(`, and upstream reports
            // the `ssa.Call` itself.
            return call.lparen.0 as u32;
        }
        return assign.rhs[0].pos().0 as u32;
    }
    if let Some(rhs) = assign.rhs.get(lhs_index) {
        if let Expr::CallExpr(call) = rhs {
            return call.lparen.0 as u32;
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

/// A no-argument func literal passed to any call — `t.Cleanup(func(){ …
/// resp.Body.Close() … })` and its like — marks `resp` closed.
///
/// Upstream reaches the same answer through the variable rather than the call:
/// the closure's `MakeClosure` is a referrer of the alloc (or free variable),
/// and `calledInFunc` walks into it and finds the `Close` on the
/// `io.ReadCloser`. The callee's name plays no part, so neither does it here.
fn mark_cleanup_close(call: &CallExpr, usages: &mut HashMap<String, RespUsage>) {
    let Some(Expr::FuncLit(fun)) = call.args.first() else {
        return;
    };
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

/// If a tracked response ident is passed as a call argument, treat it as
/// closed/escaped (callee owns the body). Skips `.Body.Close()` itself.
fn mark_escaped_arg(call: &CallExpr, usages: &mut HashMap<String, RespUsage>) {
    if body_close_var(call).is_some() {
        return;
    }
    for arg in &call.args {
        if let Some(name) = ident_name(arg) {
            if let Some(u) = usages.get_mut(name) {
                u.closed = true;
                u.consumed = true;
            }
        }
    }
}

/// `return resp.Body, nil` counts as closed.
///
/// bodyclose's `isCloseCall` has an `*ssa.Return` arm that answers yes when any
/// result of the return has the static type `io.ReadCloser` — and a `Return`
/// only reaches that arm as a referrer of the body load, i.e. when the body
/// itself is one of the returned values. Handing the body to the caller is
/// bodyclose's idea of handing over the close, which is what gitea does in five
/// different download helpers.
///
/// `resp.Body` is an `io.ReadCloser` by construction, so returning it satisfies
/// the type test on its own and there is nothing else to check. `consumed` is
/// deliberately left alone: with `check-consumption` on, upstream still goes
/// looking for a consumption call on the same body.
fn mark_returned_body(ret: &ReturnStmt, usages: &mut HashMap<String, RespUsage>) {
    for result in &ret.results {
        if let Some(name) = body_field_var(result) {
            if let Some(u) = usages.get_mut(name) {
                u.closed = true;
            }
        }
    }
}

fn is_response_composite(expr: &Expr) -> bool {
    match expr {
        Expr::UnaryExpr(u) if u.op == guff::token::Token::AND => {
            matches!(u.x.as_ref(), Expr::CompositeLit(_))
        }
        Expr::CompositeLit(_) => true,
        Expr::ParenExpr(p) => is_response_composite(&p.x),
        _ => false,
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
    // Start of the enclosing function, so an assignment to a variable declared
    // outside it can be told from one declared within.
    func_start: u32,
    closure_reassigned: &ClosureStores,
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
                handle_assign(
                    pass,
                    assign,
                    (func_start, body.rbrace.0 as u32),
                    closure_reassigned,
                    check_consumption,
                    &mut usages,
                    pending,
                );
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, check_consumption, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                // `getReqCall` accepts any call whose *type string* contains
                // `*net/http.Response`, which is true of a call returning a
                // `func(*http.Request) (*http.Response, error)` as well. No
                // referrer of such a call is a response value, so `isopen`
                // proves nothing and reports. cli's `httpmock.ScopesResponder`
                // returns exactly that.
                if mentions_response_indirectly(pass, call) {
                    let msg = if check_consumption {
                        MSG_CLOSE_AND_CONSUME
                    } else {
                        MSG_CLOSE
                    };
                    pending.push((call.lparen.0 as u32, msg.to_string()));
                }
                mark_close(call, &mut usages);
                // `t.Cleanup(func() { resp.Body.Close() })` — common in tests;
                // upstream SSA sees the close; scan no-arg Cleanup closures.
                mark_cleanup_close(call, &mut usages);
                // Passing `resp` to another call transfers ownership for this
                // AST approximation (e.g. `return handleResponse(res)`).
                mark_escaped_arg(call, &mut usages);
                if check_consumption {
                    mark_consumption(pass, call, &mut usages);
                }
            }
            NodeRef::ReturnStmt(ret) => {
                mark_returned_body(ret, &mut usages);
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
                            // go/ssa gives a call the position of its `(`.
                            pending.push((call.lparen.0 as u32, msg.to_string()));
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
    func_span: (u32, u32),
    closure_reassigned: &ClosureStores,
    check_consumption: bool,
    usages: &mut HashMap<String, RespUsage>,
    pending: &mut Vec<(u32, String)>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(name) = ident_name(lhs) else {
            continue;
        };
        if name == "_" {
            // A response assigned to the blank identifier has no `ssa.Extract`
            // for `isopen` to follow, so upstream falls through to its default
            // and reports. dapr writes `_, err = client.Do(req)` where only the
            // error is wanted.
            if discarded_response(pass, assign, i) {
                let msg = if check_consumption {
                    MSG_CLOSE_AND_CONSUME
                } else {
                    MSG_CLOSE
                };
                pending.push((assign_report_pos(assign, i), msg.to_string()));
            }
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

        // Synthetic `&http.Response{…}` / `http.Response{…}` have no live body
        // from an HTTP round-trip (upstream SSA does not flag these).
        let skip_composite = assign
            .rhs
            .first()
            .is_some_and(is_response_composite)
            || assign.rhs.get(i).is_some_and(is_response_composite);

        // Upstream works on `ssa.Call` instructions whose result carries an
        // `*http.Response`, so a response that arrives any other way is not one
        // this package opened: `resp := m[k]`, `resp := rs[0]`, `resp := in`,
        // `resp := s.R`, and `case resp := <-respCh` are all silent for it and
        // were all findings for guff, which asked only what the *type* was.
        // dapr's `tests/integration/suite/daprd/shutdown/graceful` receives its
        // responses over a channel.
        let from_call = rhs_for_index(assign, i)
            .is_some_and(|e| is_call_expr(e) && !is_make_or_new(e));
        let is_resp = !skip_httptest
            && !skip_composite
            && from_call
            && (expr_is_response(pass, lhs) || rhs_result_is_response(pass, assign, i));

        if let Some(prev) = usages.remove(name) {
            prev.report(check_consumption, pending);
        }

        // Assigning into a variable the enclosing function owns stores through
        // an `ssa.FreeVar`, and a free variable's referrers live inside the
        // closure: there is no `MakeClosure` for `isopen` to follow and no
        // field store either, so nothing proves the body is closed and the walk
        // reports. A later `resp.Body.Close()` reads the variable rather than
        // the call's result, so upstream never sees it. dapr silences four of
        // these in `tests/integration/suite/actors/http/ttl.go`.
        if is_resp && target_is_closure_reassigned(pass, lhs, func_span, closure_reassigned) {
            let msg = if check_consumption {
                MSG_CLOSE_AND_CONSUME
            } else {
                MSG_CLOSE
            };
            pending.push((assign_report_pos(assign, i), msg.to_string()));
            continue;
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

/// The call's type mentions `*net/http.Response` somewhere other than as a
/// result of its own — a function type, a slice, a channel. Upstream's
/// `getReqCall` is a substring test over the printed type, and its `getResVal`
/// then needs the exact type, so nothing matches and the walk reports.
fn mentions_response_indirectly(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(typ) = info.types.get(&call.id).map(|tv| tv.typ) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // `make` and `new` are not calls in go/ssa — they lower to `MakeChan`,
    // `MakeMap`, `MakeSlice` and `Alloc` — so `getReqCall`, which only looks at
    // `*ssa.Call`, never sees them. dapr passes responses over a
    // `make(chan *http.Response)`.
    if is_make_or_new_call(call) {
        return false;
    }

    // A result that *is* a response is the ordinary case, handled by the
    // assignment and expression-statement paths.
    let unaliased = unalias_readonly(&artifacts.types, typ);
    if is_http_response_ptr(pass, unaliased) {
        return false;
    }
    if matches!(artifacts.types.get(unaliased), TypeData::Tuple(_)) {
        let n = tuple_len(&artifacts.types, Some(unaliased));
        for i in 0..n {
            let elem = tuple_at(&artifacts.types, unaliased, i);
            if elem
                .typ(&artifacts.objects)
                .is_some_and(|t| is_http_response_ptr(pass, t))
            {
                return false;
            }
        }
    }
    let printed = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    printed.contains("*net/http.Response") && !printed.contains("net/http.ResponseController")
}

/// Variables that a closure re-assigns a response into, and that the closure
/// does not own.
///
/// Such a store goes through an `ssa.FreeVar`, whose referrers live inside the
/// closure: `isopen` finds no `MakeClosure` to follow and no field store, so
/// nothing proves the body is closed and it reports. The *outer* assignment to
/// the same variable is reported too, because the variable's alloc there does
/// have a `MakeClosure` referrer, and `calledInFunc` walks into the closure and
/// finds that unprovable store — `isopen(b, i) || !called` is then true
/// whatever the outer code does with the body. dapr's
/// `tests/integration/suite/actors/http/ttl.go` silences four of these.
fn collect_closure_reassigned(pass: &Pass<'_>) -> ClosureStores {
    let mut out = ClosureStores::default();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::FuncLit(fl) = n else {
                return true;
            };
            let span = (fl.ty.func.0 as u32, fl.body.rbrace.0 as u32);
            // `calledInFunc` walks the closure's instructions and answers with
            // `isopen(b, i) || !called` at the first one that is not a load, so
            // the outer assignment is only reported when the closure *opens*
            // there — its first statement being this very assignment.
            let first_stmt_pos = fl
                .body
                .list
                .first()
                .map(|st| st.pos().0 as u32)
                .unwrap_or_default();
            preorder(NodeRef::BlockStmt(&fl.body), |inner| {
                let NodeRef::AssignStmt(assign) = inner else {
                    return true;
                };
                let is_first_stmt = assign
                    .lhs
                    .first()
                    .map(|e| e.pos().0 as u32)
                    .unwrap_or_default()
                    == first_stmt_pos;
                for (i, lhs) in assign.lhs.iter().enumerate() {
                    let Expr::Ident(id) = lhs else {
                        continue;
                    };
                    if id.name == "_" {
                        continue;
                    }
                    let Some(obj) = info.uses.get(&id.id).copied() else {
                        continue; // `:=` here declares its own
                    };
                    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                        continue;
                    };
                    let pos = obj.pos(&artifacts.objects) as u32;
                    if pos == 0 || (pos >= span.0 && pos <= span.1) {
                        continue; // the closure owns it
                    }
                    let from_call = rhs_for_index(assign, i)
                        .is_some_and(|e| is_call_expr(e) && !is_make_or_new(e));
                    if from_call && rhs_result_is_response(pass, assign, i) {
                        // A *nested* closure that closes the body is what
                        // upstream follows out of the free variable: the
                        // `MakeClosure` among its referrers leads to
                        // `calledInFunc`, which finds the `Close` on an
                        // `io.ReadCloser` and answers "not open". dapr's
                        // `outbox` tests close inside `t.Cleanup(func(){…})`.
                        if closed_by_nested_closure(&fl.body, &id.name) {
                            continue;
                        }
                        out.inner.insert(obj);
                        if is_first_stmt {
                            out.outer.insert(obj);
                        }
                    }
                }
                true
            });
            true
        });
    }
    out
}

/// Whether some func literal nested in `body` calls `<name>.Body.Close()`.
fn closed_by_nested_closure(body: &BlockStmt, name: &str) -> bool {
    let mut found = false;
    preorder(NodeRef::BlockStmt(body), |n| {
        if found {
            return false;
        }
        let NodeRef::FuncLit(fl) = n else {
            return true;
        };
        preorder(NodeRef::BlockStmt(&fl.body), |inner| {
            if let NodeRef::CallExpr(call) = inner {
                if body_close_var(call) == Some(name) {
                    found = true;
                    return false;
                }
            }
            true
        });
        true
    });
    found
}

/// Where a closure re-assigns a response into a variable it does not own.
#[derive(Default)]
struct ClosureStores {
    /// The variables themselves: the store inside the closure is reported.
    inner: HashSet<guff_types::ObjectId>,
    /// Those whose closure opens the response as its *first* statement, where
    /// `calledInFunc` also condemns the assignment outside.
    outer: HashSet<guff_types::ObjectId>,
}

/// The identifier names one of the variables a closure re-assigns a response
/// into — see [`collect_closure_reassigned`]. Both the store inside the closure
/// and the assignment outside it are reported.
fn target_is_closure_reassigned(
    pass: &Pass<'_>,
    lhs: &Expr,
    func_span: (u32, u32),
    stores: &ClosureStores,
) -> bool {
    if stores.inner.is_empty() {
        return false;
    }
    let Expr::Ident(id) = lhs else {
        return false;
    };
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(obj) = info
        .uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).and_then(|o| *o))
    else {
        return false;
    };
    if !stores.inner.contains(&obj) {
        return false;
    }
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    // The store inside the closure — the one that reaches a free variable — is
    // always reported; the assignment where the variable lives only when the
    // closure opens as its first statement.
    let declared_at = obj.pos(&artifacts.objects) as u32;
    let captured_here = declared_at != 0 && (declared_at < func_span.0 || declared_at > func_span.1);
    captured_here || stores.outer.contains(&obj)
}

/// The value at `lhs_index` is an `*http.Response` this call opened, and it is
/// being thrown away.
fn discarded_response(pass: &Pass<'_>, assign: &AssignStmt, lhs_index: usize) -> bool {
    let Some(rhs) = rhs_for_index(assign, lhs_index) else {
        return false;
    };
    if !is_call_expr(rhs)
        || is_make_or_new(rhs)
        || is_httptest_result_call(pass, rhs)
        || is_response_composite(rhs)
    {
        return false;
    }
    rhs_result_is_response(pass, assign, lhs_index)
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
    let closure_reassigned = collect_closure_reassigned(pass);
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if func_returns_response(&fd.ty) {
                        return true;
                    }
                    if let Some(body) = &fd.body {
                        check_body(
                            pass,
                            body,
                            fd.ty.func.0 as u32,
                            &closure_reassigned,
                            check_consumption,
                            &mut pending,
                        );
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_response(&fl.ty) {
                        return true;
                    }
                    check_body(
                        pass,
                        &fl.body,
                        fl.ty.func.0 as u32,
                        &closure_reassigned,
                        check_consumption,
                        &mut pending,
                    );
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
