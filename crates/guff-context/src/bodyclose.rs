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
//! A package that does not **directly** import `net/http` is not checked at
//! all — see [`imports_net_http`]. That is upstream's own first act, and it is
//! not an optimisation: it decides findings.
//!
//! DEFERRED: full SSA referrer / Phi / closure-capture / FieldAddr /
//! `io.Closer` ChangeInterface parity.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, Expr, FieldList, FuncType, ReturnStmt, Stmt, ValueSpec};
use guff::walk::{inspect, preorder, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect as inspect_pass;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::operand::OperandMode;
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

/// Whether the function hands a `*net/http.Response` back to its caller, which
/// upstream takes as reason to skip it whole:
///
/// ```go
/// FuncLoop:
/// for _, f := range funcs {
///     // skip if the function is just referenced
///     for i := 0; i < f.Signature.Results().Len(); i++ {
///         if f.Signature.Results().At(i).Type().String() == r.resTyp.String() {
///             continue FuncLoop
///         }
///     }
/// ```
///
/// `resTyp` is `*net/http.Response`, and the comparison is on the **resolved
/// type**. Answering it on the syntax — "a result whose type is spelled
/// `Response`" — skipped every function returning any type of that name from
/// any package: boundary's `internal/clientcache/internal/client` returns
/// `(*api.Response, error)` from `Get` and `Post`, and both leaked responses
/// went unreported. It also skipped functions returning `http.Response` by
/// *value*, which upstream does not, because `resTyp` is the pointer.
fn func_returns_response(pass: &Pass<'_>, ty: &FuncType) -> bool {
    let Some(results) = ty.results.as_ref() else {
        return false;
    };
    results.list.iter().any(|field| {
        field
            .ty
            .as_ref()
            .and_then(|t| type_of(pass, t))
            .is_some_and(|t| is_http_response_ptr(pass, t))
    })
}

/// One `*http.Response` a variable held. A variable can hold several at once:
/// two arms of an `if` assigning the same name are one `ssa.Phi` downstream,
/// and a close on the phi closes both.
struct RespEntry {
    pos: u32,
    closed: bool,
    consumed: bool,
    /// If this value reached the current variable through a `Phi`, how many
    /// loops enclosed the branch that made it. `None` means it never merged.
    ///
    /// Upstream's `*ssa.Phi` arm looks for a `FieldAddr` among **that phi's**
    /// own referrers and stops there — it does not follow a phi into another
    /// phi. So a value merged inside a loop and closed outside it passes
    /// through the loop header's phi as well, and upstream loses it: it
    /// reports. That is what this depth records.
    merged_depth: Option<u32>,
}

struct RespUsage {
    entries: Vec<RespEntry>,
    /// Branch path of the assignment that last wrote this variable, so the
    /// next one can tell "kills it" from "merges with it".
    path: Vec<(u32, u16)>,
    /// Which variable these entries belong to.
    ///
    /// This table is keyed by *name*, because every close the walk sees is
    /// spelled `resp.Body.Close()` and an AST has only the name to match on.
    /// Two `resp`s in disjoint blocks are two different variables, though, and
    /// upstream (which works on `ssa.Value`s) never merges them: each `:=` is
    /// its own `Alloc` with its own referrers, and no `Phi` joins values that
    /// no path carries together. Without this the second `resp`'s close
    /// settled the first one too — beats writes exactly that, two `resp`s in
    /// one `t.Run` closure, and the leak went unreported.
    ///
    /// `None` when the identifier did not resolve, in which case the branch
    /// test below decides on its own, as it did before.
    obj: Option<guff_types::arena::ObjectId>,
    /// The response reached a closure upstream calls *not called*, and nothing
    /// written in this function can settle it. See [`RespUsage::mark_go_escape`].
    forced_open: bool,
    /// A func literal has already captured this response. Upstream decides at
    /// the *first* `MakeClosure` among the captured cell's referrers and never
    /// looks at a later one, so a goroutine that captures the response after
    /// a closure that closes it changes nothing.
    closure_seen: bool,
}

impl RespUsage {
    fn new(pos: u32, path: Vec<(u32, u16)>, obj: Option<guff_types::arena::ObjectId>) -> Self {
        Self {
            entries: vec![RespEntry {
                pos,
                closed: false,
                consumed: false,
                merged_depth: None,
            }],
            path,
            obj,
            forced_open: false,
            closure_seen: false,
        }
    }

    /// Every entry a close written at loop depth `depth` can reach.
    fn reachable(&mut self, depth: u32) -> impl Iterator<Item = &mut RespEntry> {
        self.entries
            .iter_mut()
            .filter(move |e| e.merged_depth.is_none_or(|d| d <= depth))
    }

    fn mark_closed(&mut self, depth: u32) {
        for e in self.reachable(depth) {
            e.closed = true;
        }
    }

    fn mark_consumed(&mut self, depth: u32) {
        for e in self.reachable(depth) {
            e.consumed = true;
        }
    }

    /// A `go` statement, or a closure this function only returns.
    ///
    /// Once the response is captured, upstream decides inside the closure and
    /// never looks at the enclosing function again:
    ///
    /// ```go
    /// called := r.isClosureCalled(c)
    /// return r.calledInFunc(f, called)
    /// ```
    ///
    /// and `isClosureCalled` counts only `*ssa.Call` and `*ssa.Defer`
    /// referrers of the `MakeClosure` — an `*ssa.Go` is neither. With
    /// `called == false` every arm of `calledInFunc` ends in `!called`, so the
    /// response is open whatever the closure does with it and whatever the
    /// caller does after. vitess's `streamQuerylog` writes
    /// `defer resp.Body.Close()` and then reads the body from a goroutine;
    /// upstream reports it and guff called the `defer` enough.
    ///
    /// Separate from `mark_settled` because the two race: the closure that
    /// captures the response is visited *after* the `go` that launches it, and
    /// settling must not undo this.
    ///
    /// Only the **first** capture decides. `for aref := range
    /// *resRef.Addr.Referrers()` returns at the first `*ssa.MakeClosure`, so
    /// dapr's
    ///
    /// ```go
    /// if resp != nil {
    ///     defer func() { _ = resp.Body.Close() }()
    /// }
    /// …
    /// go func() { reader := bufio.NewReader(resp.Body); … }()
    /// ```
    ///
    /// is settled by the deferred closure, and the goroutine after it is never
    /// asked. The caller checks [`RespUsage::closure_seen`].
    fn mark_go_escape(&mut self) {
        self.forced_open = true;
    }

    /// Ownership left this function — passed to a call, captured by a closure,
    /// returned, stored in a field. Nothing downstream is a phi question.
    fn mark_settled(&mut self) {
        for e in &mut self.entries {
            e.closed = true;
            e.consumed = true;
        }
    }

    fn report(self, check_consumption: bool, pending: &mut Vec<(u32, String)>) {
        let forced_open = self.forced_open;
        for e in self.entries {
            let ok = !forced_open
                && if check_consumption {
                    e.closed && e.consumed
                } else {
                    e.closed
                };
            if !ok {
                let msg = if check_consumption {
                    MSG_CLOSE_AND_CONSUME
                } else {
                    MSG_CLOSE
                };
                pending.push((e.pos, msg.to_string()));
            }
        }
    }
}

/// Where each position sits in the function's branch and loop structure.
///
/// Two assignments to one variable are two different things in SSA. If the
/// second *dominates* the first — same block, or one arm out — it kills it,
/// and the first value has no referrer left, which is upstream's
/// `len(*call.Referrers()) == 0 { return true }`. If instead they sit in
/// sibling arms, or the first is outside a branch the second is inside, both
/// values flow into one `Phi` and a later close covers both.
#[derive(Default)]
struct Shape {
    /// `(branch statement position, arm index, arm start, arm end)`.
    arms: Vec<(u32, u16, u32, u32)>,
    /// `(start, end)` of every `for` / `range` body.
    loops: Vec<(u32, u32)>,
}

impl Shape {
    fn collect(body: &BlockStmt) -> Self {
        let mut s = Shape::default();
        inspect(NodeRef::BlockStmt(body), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                // A closure is its own function; `check_body` stops there too.
                NodeRef::FuncLit(_) => return false,
                NodeRef::IfStmt(i) => {
                    let at = i.if_.0 as u32;
                    s.arms
                        .push((at, 0, i.body.lbrace.0 as u32, i.body.rbrace.0 as u32));
                    if let Some(e) = i.else_.as_deref() {
                        s.arms
                            .push((at, 1, e.pos().0 as u32, e.end().0 as u32));
                    }
                }
                NodeRef::SwitchStmt(sw) => Self::clauses(&mut s, sw.switch.0 as u32, &sw.body.list),
                NodeRef::TypeSwitchStmt(sw) => {
                    Self::clauses(&mut s, sw.switch.0 as u32, &sw.body.list)
                }
                NodeRef::SelectStmt(se) => Self::clauses(&mut s, se.select_.0 as u32, &se.body.list),
                NodeRef::ForStmt(f) => s
                    .loops
                    .push((f.body.lbrace.0 as u32, f.body.rbrace.0 as u32)),
                NodeRef::RangeStmt(r) => s
                    .loops
                    .push((r.body.lbrace.0 as u32, r.body.rbrace.0 as u32)),
                _ => {}
            }
            true
        });
        s
    }

    fn clauses(s: &mut Shape, at: u32, list: &[Stmt]) {
        for (i, c) in list.iter().enumerate() {
            let (start, body) = match c {
                Stmt::CaseClause(c) => (c.colon.0 as u32, &c.body),
                Stmt::CommClause(c) => (c.colon.0 as u32, &c.body),
                _ => continue,
            };
            let end = body.last().map_or(start, |st| st.end().0 as u32);
            s.arms.push((at, i as u16, start, end));
        }
    }

    /// The arms containing `pos`, outermost first.
    fn path(&self, pos: u32) -> Vec<(u32, u16)> {
        let mut hits: Vec<&(u32, u16, u32, u32)> = self
            .arms
            .iter()
            .filter(|(_, _, s, e)| pos >= *s && pos <= *e)
            .collect();
        hits.sort_by_key(|(_, _, s, _)| *s);
        hits.iter().map(|(at, arm, _, _)| (*at, *arm)).collect()
    }

    fn loop_depth(&self, pos: u32) -> u32 {
        self.loops
            .iter()
            .filter(|(s, e)| pos >= *s && pos <= *e)
            .count() as u32
    }
}

/// Does the assignment at `new` kill the one at `prev`, or merge with it?
///
/// It kills when `new`'s path is a prefix of `prev`'s — the same block, or one
/// branch further out, where the store is unconditional relative to the old
/// one. Anything else is a merge: sibling arms, or an old value that survives
/// on the branch's other edge.
fn kills(new: &[(u32, u16)], prev: &[(u32, u16)]) -> bool {
    new.len() <= prev.len() && new.iter().zip(prev).all(|(a, b)| a == b)
}

/// The branch whose phi merges the two paths: the first step `new` takes that
/// `prev` did not.
fn merge_branch(new: &[(u32, u16)], prev: &[(u32, u16)]) -> Option<u32> {
    let i = new
        .iter()
        .zip(prev)
        .position(|(a, b)| a != b)
        .unwrap_or(prev.len());
    new.get(i).map(|(at, _)| *at)
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

fn mark_consumption(
    pass: &Pass<'_>,
    call: &CallExpr,
    depth: u32,
    usages: &mut HashMap<String, RespUsage>,
) {
    if !is_consumption_call(pass, call) {
        return;
    }
    for arg in &call.args {
        if let Some(var) = body_field_var(arg) {
            if let Some(u) = usages.get_mut(var) {
                u.mark_consumed(depth);
            }
        }
    }
}

fn mark_close(call: &CallExpr, depth: u32, usages: &mut HashMap<String, RespUsage>) {
    if let Some(var) = body_close_var(call) {
        if let Some(u) = usages.get_mut(var) {
            u.mark_closed(depth);
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
fn mark_cleanup_close(call: &CallExpr, depth: u32, usages: &mut HashMap<String, RespUsage>) {
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
            mark_close(c, depth, usages);
        }
        true
    });
}

/// Handing a tracked response to another call only settles it when that call
/// **closes the body**.
///
/// Upstream's `*ssa.Call` arm walks into a static callee and answers `false`
/// — not open — only on finding an `isCloseCall` in it; a callee it cannot
/// see, or one that does not close, leaves the response open and the finding
/// stands. guff treated *any* argument position as handing over ownership,
/// which silenced `httputil.DumpResponse(response, false)`, a method that only
/// validates (connect-go's `d.validateResponse(response)`), and everything
/// else that merely reads.
fn mark_escaped_arg(
    pass: &Pass<'_>,
    call: &CallExpr,
    closers: &HashSet<guff_types::arena::ObjectId>,
    usages: &mut HashMap<String, RespUsage>,
) {
    if body_close_var(call).is_some() {
        return;
    }
    let callee_closes = code::call_target_object(pass, &call.fun)
        .is_some_and(|obj| closers.contains(&obj));
    if !callee_closes {
        return;
    }
    for arg in &call.args {
        if let Some(name) = ident_name(arg) {
            if let Some(u) = usages.get_mut(name) {
                u.mark_settled();
            }
        }
    }
}

/// Any tracked response a func literal **captures** is settled: upstream stops
/// at the `MakeClosure` and `calledInFunc` answers "not open".
///
/// Captures, not mentions. Upstream reaches the closure through an
/// `*ssa.MakeClosure` over a *free variable*, and a variable the literal
/// declares itself is not free — it is a different object that happens to share
/// a name. Matching on the name alone silenced every response whose function
/// also held a literal declaring a `resp` / `r` of its own, wherever that
/// literal sat. boundary's `controller_ratelimit_reload_test.go` writes
///
/// ```go
/// r, err := c.Do(func() *http.Request {
///     r, err := http.NewRequest(http.MethodGet, url, nil)
///     require.NoError(t, err)
///     return r
/// }())
/// ```
///
/// sixteen times, and all sixteen went unreported.
/// Tracked responses mentioned anywhere inside `node`, excluding names the
/// subtree declares itself.
fn tracked_names_in(
    pass: &Pass<'_>,
    node: NodeRef<'_>,
    span: (u32, u32),
    usages: &HashMap<String, RespUsage>,
) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    if usages.is_empty() {
        return seen;
    }
    inspect(node, |n| {
        if let Some(NodeRef::Ident(id)) = n {
            if usages.contains_key(id.name.as_str()) && !declared_inside(pass, id, span) {
                seen.push(id.name.clone());
            }
        }
        true
    });
    seen
}

fn mark_captured_by_closure(
    pass: &Pass<'_>,
    lit: &guff::ast::FuncLit,
    usages: &mut HashMap<String, RespUsage>,
) {
    if usages.is_empty() {
        return;
    }
    let span = (lit.ty.func.0 as u32, lit.body.rbrace.0 as u32);
    let mut seen: Vec<String> = Vec::new();
    inspect(NodeRef::BlockStmt(&lit.body), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            if usages.contains_key(id.name.as_str()) && !declared_inside(pass, id, span) {
                seen.push(id.name.clone());
            }
        }
        true
    });
    for name in seen {
        if let Some(u) = usages.get_mut(&name) {
            u.mark_settled();
            u.closure_seen = true;
        }
    }
}

/// Whether `id` resolves to an object the enclosing literal declares itself,
/// rather than to one it closes over.
///
/// Reads `defs` before `uses` so the declaring occurrence of a `:=` answers
/// yes on its own, without depending on a later mention.
///
/// An identifier that resolves to nothing, or to an object with no position,
/// answers **no** — settled, the behaviour before this predicate existed. That
/// is the safe side of the unknown here: settling a response that was not
/// really captured costs a missed finding, while refusing to settle one that
/// was costs a false positive on code that does close the body.
fn declared_inside(pass: &Pass<'_>, id: &guff::ast::Ident, span: (u32, u32)) -> bool {
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
    else {
        return false;
    };
    let obj = info
        .defs
        .get(&id.id)
        .copied()
        .flatten()
        .or_else(|| info.uses.get(&id.id).copied());
    let Some(obj) = obj else {
        return false;
    };
    let pos = obj.pos(&artifacts.objects) as u32;
    pos != 0 && pos >= span.0 && pos <= span.1
}

/// The functions of this package that close a `*http.Response` parameter's
/// body — the ones upstream's walk into a static callee would answer `false`
/// for.
fn response_closing_funcs(pass: &Pass<'_>) -> HashSet<guff_types::arena::ObjectId> {
    let mut out = HashSet::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(body) = &fd.body else {
                continue;
            };
            // Which parameters are `*http.Response`, by name.
            let mut params: HashSet<&str> = HashSet::new();
            if let Some(list) = fd.ty.params.as_ref() {
                for field in &list.list {
                    if !field
                        .ty
                        .as_ref()
                        .is_some_and(type_expr_looks_like_response)
                    {
                        continue;
                    }
                    for name in &field.names {
                        params.insert(name.name.as_str());
                    }
                }
            }
            if params.is_empty() {
                continue;
            }
            let mut closes = false;
            inspect(NodeRef::BlockStmt(body), |n| {
                let Some(NodeRef::CallExpr(c)) = n else {
                    return true;
                };
                if body_close_var(c).is_some_and(|v| params.contains(v)) {
                    closes = true;
                }
                true
            });
            if closes {
                if let Some(Some(obj)) = info.defs.get(&fd.name.id) {
                    out.insert(*obj);
                }
            }
        }
    }
    out
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
                for e in &mut u.entries {
                    e.closed = true;
                }
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

fn handle_defer_close(call: &CallExpr, depth: u32, usages: &mut HashMap<String, RespUsage>) {
    match call.fun.as_ref() {
        Expr::SelectorExpr(_) => mark_close(call, depth, usages),
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
                    mark_close(c, depth, usages);
                }
                true
            });
        }
        _ => {}
    }
}

/// bodyclose's `isCloseCall` `*ssa.ChangeInterface` arm, as a table of the
/// package's functions that take an `io.Closer` and close it.
///
/// `defer closing(resp.Body, log)` converts the body to `io.Closer` — an
/// `*ssa.ChangeInterface` — and upstream then walks *that* value's referrers
/// looking for a `*ssa.Defer` whose callee contains a call to
/// `(io.Closer).Close`. beats' `libbeat/esleg/eslegclient/connection.go:512`
/// writes exactly that.
///
/// A plain, non-deferred call never reaches the arm: the referrer is an
/// `*ssa.Call`, not a `*ssa.Defer`. Measured side by side — both tools report
/// `closing(resp.Body)` without the `defer` and neither reports it with one.
///
/// The value is one flag per parameter position: whether that parameter is
/// exactly `io.Closer`, which is what makes the conversion happen.
fn io_closer_closing_funcs(
    pass: &Pass<'_>,
) -> HashMap<guff_types::arena::ObjectId, Vec<bool>> {
    let mut out = HashMap::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(body) = &fd.body else {
                continue;
            };
            // One flag per parameter *position* — a field can name several.
            let mut flags: Vec<bool> = Vec::new();
            let mut closer_params: HashSet<&str> = HashSet::new();
            if let Some(list) = fd.ty.params.as_ref() {
                for field in &list.list {
                    let is_closer = field
                        .ty
                        .as_ref()
                        .is_some_and(|t| is_io_closer_type(pass, t));
                    let n = field.names.len().max(1);
                    for _ in 0..n {
                        flags.push(is_closer);
                    }
                    if is_closer {
                        for name in &field.names {
                            closer_params.insert(name.name.as_str());
                        }
                    }
                }
            }
            if closer_params.is_empty() {
                continue;
            }
            // Upstream asks whether the callee calls `(io.Closer).Close` at
            // all; asking that it be one of *these* parameters is narrower and
            // never says yes where upstream says no.
            let mut closes = false;
            inspect(NodeRef::BlockStmt(body), |n| {
                let Some(NodeRef::CallExpr(c)) = n else {
                    return true;
                };
                if let Expr::SelectorExpr(sel) = c.fun.as_ref() {
                    if sel.sel.name == "Close" {
                        if let Some(name) = ident_name(&sel.x) {
                            if closer_params.contains(name) {
                                closes = true;
                            }
                        }
                    }
                }
                true
            });
            if closes {
                if let Some(Some(obj)) = info.defs.get(&fd.name.id) {
                    out.insert(*obj, flags);
                }
            }
        }
    }
    out
}

/// Exactly `io.Closer` — the type the conversion upstream keys on produces.
fn is_io_closer_type(pass: &Pass<'_>, ty: &Expr) -> bool {
    let Some(typ) = type_of(pass, ty) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let TypeData::Named(n) = artifacts.types.get(typ) else {
        return false;
    };
    let obj = n.obj();
    obj.name(&artifacts.objects) == "Closer"
        && obj
            .pkg(&artifacts.objects)
            .is_some_and(|p| artifacts.packages.get(p).path() == "io")
}

/// `defer f(resp.Body, …)` where `f` closes the `io.Closer` it is handed.
fn mark_deferred_closer_arg(
    pass: &Pass<'_>,
    call: &CallExpr,
    io_closers: &HashMap<guff_types::arena::ObjectId, Vec<bool>>,
    usages: &mut HashMap<String, RespUsage>,
) {
    if io_closers.is_empty() {
        return;
    }
    let Some(obj) = code::call_target_object(pass, &call.fun) else {
        return;
    };
    let Some(flags) = io_closers.get(&obj) else {
        return;
    };
    for (i, arg) in call.args.iter().enumerate() {
        if !flags.get(i).copied().unwrap_or(false) {
            continue;
        }
        let Some(name) = body_field_var(arg) else {
            continue;
        };
        if let Some(u) = usages.get_mut(name) {
            for e in &mut u.entries {
                e.closed = true;
            }
        }
    }
}

fn check_body(
    pass: &Pass<'_>,
    body: &BlockStmt,
    // Start of the enclosing function, so an assignment to a variable declared
    // outside it can be told from one declared within.
    func_start: u32,
    closure_reassigned: &ClosureStores,
    closers: &HashSet<guff_types::arena::ObjectId>,
    io_closers: &HashMap<guff_types::arena::ObjectId, Vec<bool>>,
    check_consumption: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let shape = Shape::collect(body);
    let mut usages: HashMap<String, RespUsage> = HashMap::new();
    // `x.f = resp` — upstream's `*ssa.Store` into a `FieldAddr`, which it
    // settles by looking for a close on the body reached through that field.
    // Keyed by the rendered left-hand side (`d.response`).
    let mut field_aliases: HashMap<String, String> = HashMap::new();
    // Every `<chain>.Body.Close()` seen anywhere in the body.
    let mut chain_closes: HashSet<String> = HashSet::new();
    // Calls whose result has its `Body` closed in the very expression the call
    // appears in — see [`body_close_call_base`].
    let mut closed_call_results: HashSet<u32> = HashSet::new();
    let mut usages_to_close: Vec<String> = Vec::new();
    // Calls whose result the walk below already accounts for: the right-hand
    // side of an assignment or a `var` spec (tracked by name), and a call
    // statement (reported by the `ExprStmt` arm).
    let handled_calls = calls_handled_elsewhere(body);

    inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::GoStmt(go) = n {
            // Upstream's `isClosureCalled` does not count an `*ssa.Go` as a
            // call, and no arm of `isopen` matches an `*ssa.Go` referrer.
            //
            // - `go func() { … resp … }()`: the literal's `MakeClosure` is a
            //   referrer of the captured cell. If it is the *first* capture,
            //   upstream stops there with `called == false` and the response
            //   is open whatever else happens; if an earlier closure captured
            //   it, that one already decided.
            // - `go sink(resp)`: the value reaches an `*ssa.Go` that nothing
            //   matches. It is not a hand-off (a `defer resp.Body.Close()`
            //   elsewhere still settles it) and not a leak either: upstream
            //   skips it. So the walk does not descend, where the call arm
            //   would read `resp` as passed to a callee.
            if let Expr::FuncLit(lit) = code::unparen(&go.call.fun) {
                let span = (lit.ty.func.0 as u32, lit.body.rbrace.0 as u32);
                for name in tracked_names_in(pass, NodeRef::BlockStmt(&lit.body), span, &usages) {
                    if let Some(u) = usages.get_mut(&name) {
                        if !u.closure_seen {
                            u.mark_go_escape();
                        }
                    }
                }
                mark_captured_by_closure(pass, lit, &mut usages);
            }
            return false;
        }
        if let NodeRef::FuncLit(lit) = n {
            // A response captured by a func literal is upstream's `*ssa.Store`
            // -> `*ssa.MakeClosure` branch, and `calledInFunc` answers "not
            // open" for it however the closure uses the body — draining it,
            // reading a field, or touching nothing at all. Only the closure
            // that actually closes was recognised here before, so
            // `defer func() { io.Copy(io.Discard, resp.Body) }()` — connect-go's
            // `bench_test.go` — read as a leak.
            mark_captured_by_closure(pass, lit, &mut usages);
            return false;
        }

        match n {
            NodeRef::AssignStmt(assign) => {
                note_indirect_store(
                    pass,
                    assign,
                    &usages,
                    &mut field_aliases,
                    &mut usages_to_close,
                );
                handle_assign(
                    pass,
                    assign,
                    (func_start, body.rbrace.0 as u32),
                    closure_reassigned,
                    &shape,
                    check_consumption,
                    &mut usages,
                    pending,
                );
            }
            NodeRef::ValueSpec(spec) => {
                handle_value_spec(pass, spec, check_consumption, &mut usages, pending);
            }
            NodeRef::CallExpr(call) => {
                if let Some(chain) = body_close_chain(call) {
                    chain_closes.insert(chain);
                }
                if let Some(base) = body_close_call_base(call) {
                    closed_call_results.insert(base);
                }
                // `getReqCall` accepts *any* call whose result type mentions
                // `*net/http.Response` — a helper of this package as much as
                // `client.Do` — and a result nobody binds has no referrers, so
                // `isopen` reports. guff only ever tracked assignments, so
                // `getPingResponse(t, "ping").Uncompressed` (connect-go, four
                // times) said nothing.
                if !handled_calls.contains(&call.id)
                    && !closed_call_results.contains(&call.id)
                    && !is_conversion(pass, call)
                    && call_result_is_response(pass, call)
                    && !is_httptest_result_call(pass, &Expr::CallExpr(call.clone()))
                {
                    let msg = if check_consumption {
                        MSG_CLOSE_AND_CONSUME
                    } else {
                        MSG_CLOSE
                    };
                    pending.push((call.lparen.0 as u32, msg.to_string()));
                }
                // `getReqCall` accepts any call whose *type string* contains
                // `*net/http.Response`, which is true of a call returning a
                // `func(*http.Request) (*http.Response, error)` as well. No
                // referrer of such a call is a response value, so `isopen`
                // proves nothing and reports. cli's `httpmock.ScopesResponder`
                // returns exactly that.
                if !is_conversion(pass, call) && mentions_response_indirectly(pass, call) {
                    let msg = if check_consumption {
                        MSG_CLOSE_AND_CONSUME
                    } else {
                        MSG_CLOSE
                    };
                    pending.push((call.lparen.0 as u32, msg.to_string()));
                }
                let depth = shape.loop_depth(call.lparen.0 as u32);
                mark_close(call, depth, &mut usages);
                // `t.Cleanup(func() { resp.Body.Close() })` — common in tests;
                // upstream SSA sees the close; scan no-arg Cleanup closures.
                mark_cleanup_close(call, depth, &mut usages);
                // Passing `resp` to another call transfers ownership for this
                // AST approximation (e.g. `return handleResponse(res)`).
                mark_escaped_arg(pass, call, closers, &mut usages);
                if check_consumption {
                    mark_consumption(pass, call, depth, &mut usages);
                }
            }
            NodeRef::ReturnStmt(ret) => {
                mark_returned_body(ret, &mut usages);
                // A closure this function only *returns* has no `*ssa.Call`
                // or `*ssa.Defer` referrer either —
                // `return func() { resp.Body.Close() }` is reported, while the
                // same literal handed to `t.Cleanup` is not, because there the
                // argument makes the call a referrer.
                //
                // Folded into this arm rather than intercepting `ReturnStmt`
                // before the match: an early `return true` here skipped
                // `mark_returned_body`, and `return resp.Body, nil` — which
                // upstream treats as handing the close to the caller — became a
                // finding. The isolate tier caught it; nothing else did.
                for result in &ret.results {
                    let Expr::FuncLit(lit) = result else {
                        continue;
                    };
                    let span = (lit.ty.func.0 as u32, lit.body.rbrace.0 as u32);
                    for name in
                        tracked_names_in(pass, NodeRef::BlockStmt(&lit.body), span, &usages)
                    {
                        if let Some(u) = usages.get_mut(&name) {
                            u.mark_go_escape();
                        }
                    }
                }
            }
            NodeRef::DeferStmt(d) => {
                let depth = shape.loop_depth(d.defer_.0 as u32);
                handle_defer_close(&d.call, depth, &mut usages);
                mark_deferred_closer_arg(pass, &d.call, io_closers, &mut usages);
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
                                    mark_consumption(pass, c, depth, &mut usages);
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

    // A response stored into a field is settled when the body is closed
    // through that field — connect-go closes `d.response.Body` from a
    // different method, but gitea and others do it in the same one.
    for (chain, resp) in &field_aliases {
        if chain_closes.contains(chain) {
            if let Some(u) = usages.get_mut(resp) {
                u.mark_settled();
            }
        }
    }
    for name in usages_to_close {
        if let Some(u) = usages.get_mut(&name) {
            u.mark_settled();
        }
    }

    for (_, u) in usages {
        u.report(check_consumption, pending);
    }
}

/// Does this call's result include a `*http.Response`?
fn call_result_is_response(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(typ) = type_of(pass, &Expr::CallExpr(call.clone())) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    if matches!(artifacts.types.get(typ), TypeData::Tuple(_)) {
        let n = tuple_len(&artifacts.types, Some(typ));
        return (0..n).any(|i| {
            tuple_at(&artifacts.types, typ, i)
                .typ(&artifacts.objects)
                .is_some_and(|t| is_http_response_ptr(pass, t))
        });
    }
    is_http_response_ptr(pass, typ)
}

/// The calls whose result some other arm of the walk already accounts for.
fn calls_handled_elsewhere(body: &BlockStmt) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut note = |e: &Expr, out: &mut HashSet<u32>| {
        if let Expr::CallExpr(c) = e {
            out.insert(c.id);
        }
    };
    inspect(NodeRef::BlockStmt(body), |n| {
        match n {
            Some(NodeRef::AssignStmt(a)) => {
                for e in &a.rhs {
                    note(e, &mut out);
                }
            }
            Some(NodeRef::ValueSpec(v)) => {
                for e in &v.values {
                    note(e, &mut out);
                }
            }
            Some(NodeRef::ExprStmt(es)) => note(&es.x, &mut out),
            _ => {}
        }
        true
    });
    out
}

/// The rendered receiver of a `<chain>.Body.Close()` call, where `<chain>` is a
/// dotted path of identifiers (`d.response`).
fn body_close_chain(call: &CallExpr) -> Option<String> {
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
    render_chain(&body_sel.x)
}

/// The **call** at the base of `<call>.Body.Close()`, as a node id.
///
/// [`body_close_chain`] answers the same question for a dotted path of
/// identifiers, and `render_chain` stops at anything that is not one — so
/// `w.HttpResponse().Body.Close()` recorded nothing and the very call it closes
/// was reported. Upstream walks that call's result through its referrers,
/// finds the `FieldAddr` for `Body` and the `Close` on it, and answers "not
/// open".
///
/// A *second* call in the same chain is a different value and stays reportable:
/// `w.HttpResponse().StatusCode` after `w.HttpResponse().Body.Close()` is a
/// finding for both tools.
fn body_close_call_base(call: &CallExpr) -> Option<u32> {
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
    fn call_id(e: &Expr) -> Option<u32> {
        match e {
            Expr::CallExpr(inner) => Some(inner.id),
            Expr::ParenExpr(p) => call_id(&p.x),
            _ => None,
        }
    }
    call_id(&body_sel.x)
}

/// A conversion `T(x)`, which the AST spells as a `CallExpr` and go/ssa does
/// not spell as a call at all.
///
/// `getReqCall` matches `*ssa.Call`, and a conversion lowers to `ChangeType` /
/// `Convert` / `MakeInterface`, so upstream never offers one to its test. That
/// test is a substring over the printed type —
///
/// ```go
/// if !strings.Contains(callType, r.resTyp.String()) ||
///     strings.Contains(callType, "net/http.ResponseController") { return nil, false }
/// ```
///
/// — and `*net/http.ResponseWriter` *does* contain `*net/http.Response`. The
/// substring test is faithful; applying it to AST calls is what is not.
/// boundary's `require.Implements(t, (*http.ResponseWriter)(nil), wrapped)` is
/// the shape.
fn is_conversion(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    info.types
        .get(&call.fun.id())
        .is_some_and(|tv| tv.mode == OperandMode::TypeExpr)
}

/// `d.response` → `Some("d.response")`; anything that is not a dotted path of
/// identifiers → `None`.
fn render_chain(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => Some(format!("{}.{}", render_chain(&sel.x)?, sel.sel.name)),
        Expr::ParenExpr(p) => render_chain(&p.x),
        _ => None,
    }
}

/// Records `x.f = resp` (a field store) and settles `pkgVar = resp` (a store to
/// a package-level variable, which upstream answers `false` for outright:
/// "Referrers for globals are always nil, so skip").
fn note_indirect_store(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    usages: &HashMap<String, RespUsage>,
    field_aliases: &mut HashMap<String, String>,
    to_close: &mut Vec<String>,
) {
    for (i, lhs) in assign.lhs.iter().enumerate() {
        let Some(rhs) = rhs_for_index(assign, i) else {
            continue;
        };
        let Some(resp) = ident_name(rhs).filter(|n| usages.contains_key(*n)) else {
            continue;
        };
        match lhs {
            Expr::SelectorExpr(_) => {
                if let Some(chain) = render_chain(lhs) {
                    field_aliases.insert(chain, resp.to_string());
                }
            }
            Expr::Ident(id) => {
                if is_package_level_var(pass, id) {
                    to_close.push(resp.to_string());
                }
            }
            _ => {}
        }
    }
}

/// The object an assignment's left-hand identifier names.
///
/// A `:=` *defines* its names and an `=` *uses* them, so both tables are
/// consulted. `_` has no object, and neither does an identifier in a file the
/// type-checker could not finish; both answer `None`, which leaves the branch
/// test to decide alone.
fn assigned_object(pass: &Pass<'_>, expr: &Expr) -> Option<guff_types::arena::ObjectId> {
    let Expr::Ident(id) = expr else {
        return None;
    };
    let info = pass.types_info()?;
    info.defs
        .get(&id.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&id.id).copied())
}

/// Is `id` a package-level variable of the package being analysed?
fn is_package_level_var(pass: &Pass<'_>, id: &guff::ast::Ident) -> bool {
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
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(scope) = obj.parent(&artifacts.objects) else {
        return false;
    };
    // The package scope is the one whose own parent is the universe.
    artifacts.scopes.get(scope).parent().is_none_or(|p| {
        artifacts.scopes.get(p).parent().is_none()
    })
}

fn handle_assign(
    pass: &Pass<'_>,
    assign: &AssignStmt,
    func_span: (u32, u32),
    closure_reassigned: &ClosureStores,
    shape: &Shape,
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

        // Two assignments to one name are two SSA values. The second kills the
        // first only when it dominates it — the same block, or one branch
        // further out; in sibling arms, or one arm in from the other, both
        // reach a `Phi` and one close settles them together. guff used to
        // report the earlier one on sight, which is telegraf's
        // `plugins/inputs/prometheus/prometheus.go:581`: two clients, one
        // `resp`, one `defer resp.Body.Close()`.
        let path = shape.path(assign_report_pos(assign, i));
        let obj = assigned_object(pass, lhs);
        // A merge needs one variable. When the two assignments name *different*
        // objects — a `:=` that shadows, or two `:=` in blocks neither of which
        // encloses the other — there is no `Phi` joining them upstream and the
        // earlier value keeps its own fate, so settle it here and start over.
        // Only the branch geometry was asked before, and sibling arms read as
        // a merge whether or not they shared a variable.
        let same_var = match (obj, usages.get(name).and_then(|u| u.obj)) {
            (Some(a), Some(b)) => a == b,
            // Unresolved on either side: fall back to the geometry alone.
            _ => true,
        };
        let merged = match usages.get(name) {
            Some(prev) if same_var && !kills(&path, &prev.path) => {
                merge_branch(&path, &prev.path).map(|at| shape.loop_depth(at))
            }
            Some(_) => {
                if let Some(prev) = usages.remove(name) {
                    prev.report(check_consumption, pending);
                }
                None
            }
            None => None,
        };

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
            let pos = assign_report_pos(assign, i);
            match usages.get_mut(name) {
                // A merge: the earlier values are still live, and every one of
                // them now sits behind the branch's phi.
                Some(u) => {
                    let depth = merged.unwrap_or(0);
                    for e in &mut u.entries {
                        e.merged_depth = Some(e.merged_depth.unwrap_or(0).max(depth));
                    }
                    u.entries.push(RespEntry {
                        pos,
                        closed: false,
                        consumed: false,
                        merged_depth: Some(depth),
                    });
                    u.path = path.clone();
                }
                None => {
                    usages.insert(name.to_string(), RespUsage::new(pos, path.clone(), obj));
                }
            }
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
                        if captured_by_invoked_closure(pass, &fl.body, &id.name, span) {
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

/// Whether a func literal nested in `body` captures `name` and is **invoked**:
/// called on the spot, deferred, or passed as an argument.
///
/// That is `isClosureCalled`, which counts an `*ssa.Call` or `*ssa.Defer`
/// referrer of the `MakeClosure` — passing the literal to a function makes the
/// call itself that referrer — and then `calledInFunc(f, true)`, which answers
/// at the closure's first instruction that is not a load: a `Close` on an
/// `io.ReadCloser` gives `!called`, and anything else gives
/// `isopen(b, i) || !called`, both **false** for a called closure that opens
/// nothing. So the capture settles the response whether or not the closure
/// closes it — datadog-agent's `pkg/util/ecs/metadata/v3or4` defers
/// `func() { telemetry.AddQueryToTelemetry(path, resp) }` and is silent
/// upstream even in the variant where nothing closes the body.
///
/// A literal that is never invoked does not settle it (`called == false` makes
/// `calledInFunc` true), and neither does `go func(){…}()`: an `*ssa.Go` is
/// neither a `Call` nor a `Defer`.
fn captured_by_invoked_closure(
    pass: &Pass<'_>,
    body: &BlockStmt,
    name: &str,
    outer_span: (u32, u32),
) -> bool {
    let mut found = false;
    let mut stack: Vec<NodeRef<'_>> = Vec::new();
    guff::walk::preorder_stack(NodeRef::BlockStmt(body), &mut stack, |n, st| {
        if found {
            return false;
        }
        let NodeRef::FuncLit(fl) = n else {
            return true;
        };
        // Does it capture `name`? (A literal that declares its own does not.)
        let span = (fl.ty.func.0 as u32, fl.body.rbrace.0 as u32);
        let mut mentions = false;
        let mut inner_stack: Vec<NodeRef<'_>> = Vec::new();
        guff::walk::preorder_stack(
            NodeRef::BlockStmt(&fl.body),
            &mut inner_stack,
            |inner, ist| {
                if let NodeRef::Ident(id) = inner {
                    // `_ = resp` loads nothing: go/ssa drops an assignment to
                    // the blank identifier, so the literal captures no free
                    // variable and upstream never reaches `calledInFunc`.
                    let blank_rhs = matches!(ist.last(), Some(NodeRef::AssignStmt(a))
                        if a.lhs.iter().all(|l| matches!(l, Expr::Ident(i) if i.name == "_")));
                    if id.name == name && !blank_rhs && !declared_inside(pass, id, span) {
                        mentions = true;
                    }
                }
                true
            },
        );
        let _ = outer_span;
        if mentions && literal_is_invoked(fl, st) {
            found = true;
            return false;
        }
        true
    });
    found
}

/// `isClosureCalled`: the literal is the callee of a call or a `defer`, or an
/// argument of one. `go f()` is neither.
fn literal_is_invoked(fl: &guff::ast::FuncLit, stack: &[NodeRef<'_>]) -> bool {
    let me = NodeRef::FuncLit(fl).erased_ptr();
    let Some(parent) = stack.last() else {
        return false;
    };
    let NodeRef::CallExpr(call) = parent else {
        return false;
    };
    let is_callee = guff::walk::expr_ref(&call.fun).erased_ptr() == me;
    let is_arg = call
        .args
        .iter()
        .any(|a| guff::walk::expr_ref(a).erased_ptr() == me);
    if !is_callee && !is_arg {
        return false;
    }
    // `go f()` does not count; `defer f()` and a plain call do.
    let grand = stack.len().checked_sub(2).map(|i| stack[i]);
    !matches!(grand, Some(NodeRef::GoStmt(_)))
}

/// Whether some func literal nested in `body` calls `<name>.Body.Close()`.
#[allow(dead_code)]
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
            // A `var` declaration introduces the name, so there is nothing
            // for it to merge with — but if it *shadows* a response already
            // tracked under that name, that one is a separate variable whose
            // fate this declaration cannot change.
            if let Some(prev) = usages.remove(name) {
                prev.report(check_consumption, pending);
            }
            let obj = pass
                .types_info()
                .and_then(|info| info.defs.get(&name_id.id).and_then(|o| *o));
            usages.insert(name.to_string(), RespUsage::new(pos, Vec::new(), obj));
        }
    }
}

/// Whether the package being analysed lists `net/http` among its own imports.
///
/// Upstream's `run` begins with
///
/// ```go
/// r.resObj = analysisutil.LookupFromImports(pass.Pkg.Imports(), "net/http", "Response")
/// if r.resObj == nil {
///     return nil, nil // skip checking
/// }
/// ```
///
/// and `LookupFromImports` (`gostaticanalysis/analysisutil`) walks
/// `pass.Pkg.Imports()` — the package's **direct** imports, with no
/// transitivity. So a package that only reaches `*http.Response` through a
/// dependency is skipped whole, whatever its code looks like: scaleway-cli's
/// `internal/gotty` dials with `gorilla/websocket`, never names `net/http`, and
/// upstream reports nothing there. Ten call shapes agreed between the two tools
/// before this gate went in; the divergence was never the shape.
///
/// A Go package's imports are the union of its files', which is what this
/// walks. `pass.files()` includes the `_test.go` files for a test-augmented
/// package, as `pass.Pkg.Imports()` does.
fn imports_net_http(pass: &Pass<'_>) -> bool {
    pass.files().iter().any(|file| {
        file.imports.iter().any(|spec| {
            let path = spec.path.value.trim_matches('"');
            // `analysisutil.RemoveVendor`.
            let path = path.rsplit_once("/vendor/").map_or(path, |(_, rest)| rest);
            path == HTTP_PKG
        })
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect_pass::InspectResult>(inspect_pass::analyzer())
        .ok_or_else(|| "bodyclose requires inspect analyzer".to_string())?;

    if !imports_net_http(pass) {
        return Ok(None);
    }

    let options = pass
        .settings::<BodycloseOptions>("bodyclose")
        .cloned()
        .unwrap_or_default();
    let check_consumption = options.check_consumption;

    let mut pending: Vec<(u32, String)> = Vec::new();
    let closure_reassigned = collect_closure_reassigned(pass);
    let closers = response_closing_funcs(pass);
    let io_closers = io_closer_closing_funcs(pass);
    for file in pass.files() {
        // Rooted at each `FuncDecl`, not at the file: `buildssa` builds
        // `SrcFuncs` from those alone, so a literal in a package-level `var`
        // initializer — every Ginkgo suite — is invisible upstream.
        // See [`code::src_func_decls`].
        for top in code::src_func_decls(file) {
            preorder(NodeRef::FuncDecl(top), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if func_returns_response(pass, &fd.ty) {
                        return true;
                    }
                    if let Some(body) = &fd.body {
                        check_body(
                            pass,
                            body,
                            fd.ty.func.0 as u32,
                            &closure_reassigned,
                            &closers,
                            &io_closers,
                            check_consumption,
                            &mut pending,
                        );
                    }
                }
                NodeRef::FuncLit(fl) => {
                    if func_returns_response(pass, &fl.ty) {
                        return true;
                    }
                    check_body(
                        pass,
                        &fl.body,
                        fl.ty.func.0 as u32,
                        &closure_reassigned,
                        &closers,
                        &io_closers,
                        check_consumption,
                        &mut pending,
                    );
                }
                _ => {}
            }
            true
            });
        }
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
