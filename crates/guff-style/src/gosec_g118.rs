//! Gosec **G118** — context propagation failure (SSA).
//!
//! Port of securego/gosec v2.26.1 `analyzers/context_propagation.go` (plus the
//! `BaseAnalyzerState.ResolveFuncs` walk it borrows from `analyzers/util.go`).
//!
//! One analyzer id, three unrelated checks, each with its own severity /
//! confidence pair — so [`crate::gosec::issue_scores`] has to read the message
//! to grade a G118 finding, the way it already does for G402:
//!
//! - **lost cancel** (Medium/High) — the `cancel` returned by
//!   `context.With{Cancel,Timeout,Deadline}` is never called. This is the one
//!   that fires in practice; the whole `is_cancel_called` walk below exists to
//!   *not* report the many ways a cancel legitimately escapes (returned,
//!   captured by a closure, stored in a struct field another method calls,
//!   parked in a package-level var).
//! - **goroutine on a detached context** (High/Medium) — a `go` statement whose
//!   callee reaches `context.Background`/`TODO` inside a function that was
//!   handed a request-scoped context.
//! - **loop without a `Done` guard** (High/Low) — a strongly-connected region of
//!   the CFG with *no edge leaving it* that makes a blocking call and never
//!   selects on `ctx.Done()`. The no-exit condition makes this much narrower
//!   than it sounds: a `for { … return … }` has an edge to the return block, so
//!   only a genuinely non-terminating loop qualifies. Nothing in the 14-repo
//!   corpus reaches it.
//!
//! Two upstream shapes are worth naming because they decide findings and are
//! easy to "fix" into a diff:
//!
//! - `types.Identical` is what matches a `cancel` stored into a struct field
//!   against the methods that might call it. A **generic** type defeats it: in
//!   `func New[T any]() *Conn[T]` the composite literal's type is the
//!   *instance* `Conn[T]`, while `(*Conn[T]).Close`'s receiver is the generic
//!   *origin* with no type arguments, and the two are not identical. dapr's
//!   `pluggable.GRPCConnector[TClient]` is reported for exactly this reason
//!   even though `Close` does call `g.Cancel()`.
//! - a `cancel` stored into a **map** (`s.retryCancel[name] = cancel`) is not
//!   tracked at all — `MapUpdate` is not in the walk's instruction set — so
//!   dapr's `subscriber.retrySubscription` is reported too.
//!
//! The SSA program and the `SrcFuncs` list come from [`crate::gosec_ssa`],
//! which builds them once for every SSA-based gosec analyzer.

use std::collections::{HashMap, HashSet, VecDeque};

use guff::token::Token;
use guff::Pos;
use guff_analysis::callcheck::{render_type, static_callee};
use guff_analysis::referrers;
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, FuncId, GlobalId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;

const CONTEXT_PKG: &str = "context";
const HTTP_PKG: &str = "net/http";

pub(crate) const MSG_BACKGROUND: &str =
    "G118: Goroutine uses context.Background/TODO while request-scoped context is available";
pub(crate) const MSG_LOST_CANCEL: &str = "G118: context cancellation function returned by \
     WithCancel/WithTimeout/WithDeadline is not called";
pub(crate) const MSG_LOOP_WITHOUT_DONE: &str =
    "G118: Long-running loop performs calls without a ctx.Done() cancellation guard";

/// gosec `MaxDepth`, the bound on `ResolveFuncs`.
const MAX_DEPTH: u32 = 20;

// ---------------------------------------------------------------------------
// Type identity
// ---------------------------------------------------------------------------

/// `types.Identical` over the small set of types G118 actually compares.
///
/// The rule needs identity only between *struct-pointer* types: the base of a
/// `FieldAddr` against another `FieldAddr`'s base, or against a method's
/// receiver. [`guff_types::predicates::identical`] wants `&mut TypeArena`
/// (interface identity computes a type set on first use) and the analysis
/// wants `&Program`, so the comparison is precomputed here: the candidate
/// types are partitioned into equivalence classes once, and the walk below
/// compares class numbers.
struct TypeClasses {
    class: HashMap<TypeId, u32>,
}

impl TypeClasses {
    fn build(prog: &mut Program, src_funcs: &[FuncId]) -> Self {
        let mut candidates: HashSet<TypeId> = HashSet::new();
        for &fid in src_funcs {
            let func = prog.functions.get(fid);
            if let Some(recv) = func_recv_type(prog, func) {
                candidates.insert(recv);
            }
            for (_, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    if let InstrData::FieldAddr(fa) = func.instrs.get(iid) {
                        candidates.insert(value_type_of(prog, func, fa.x));
                    }
                }
            }
        }

        // Identical types always render identically, so the rendering is a
        // sound pre-partition; only within a group does the O(n²) comparison
        // run, and a group is normally a single type.
        let mut by_render: HashMap<String, Vec<TypeId>> = HashMap::new();
        let mut candidates: Vec<TypeId> = candidates.into_iter().collect();
        candidates.sort_by_key(|t| format!("{t:?}"));
        for &t in &candidates {
            let key = render_type(&prog.type_arena, &prog.object_arena, &prog.package_arena, t);
            by_render.entry(key).or_default().push(t);
        }

        let mut class: HashMap<TypeId, u32> = HashMap::new();
        let mut next: u32 = 0;
        for (_, group) in by_render {
            let mut reps: Vec<(TypeId, u32)> = Vec::new();
            for t in group {
                let found = reps.iter().find(|&&(rep, _)| {
                    guff_types::predicates::identical(
                        &mut prog.type_arena,
                        &prog.object_arena,
                        &prog.package_arena,
                        rep,
                        t,
                    )
                });
                match found {
                    Some(&(_, c)) => {
                        class.insert(t, c);
                    }
                    None => {
                        class.insert(t, next);
                        reps.push((t, next));
                        next += 1;
                    }
                }
            }
        }

        TypeClasses { class }
    }

    fn same(&self, a: TypeId, b: TypeId) -> bool {
        if a == b {
            return true;
        }
        match (self.class.get(&a), self.class.get(&b)) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Analysis state shared by the three detectors, plus the two package-wide
/// indexes that replace upstream's repeated `for _, fn := range allFuncs`
/// scans. Building them once is the only deliberate difference from upstream:
/// the answers are the same, the cost is not quadratic in the number of
/// `context.WithCancel` calls.
struct Ctx<'a> {
    prog: &'a Program,
    classes: TypeClasses,
    /// Every `FieldAddr` in the package: `(func, instr, field index, type of X)`.
    field_addrs: Vec<(FuncId, InstrId, usize, TypeId)>,
    /// Every `*global` load in the package: `(func, instr, global)`.
    global_loads: Vec<(FuncId, InstrId, GlobalId)>,
}

/// Collects G118 out of the SSA build [`crate::gosec_ssa`] shares between the
/// gosec analyzers, appending `(pos, message)` into `pending`.
pub(crate) fn collect_g118(
    prog: &mut Program,
    src_funcs: &[FuncId],
    pending: &mut Vec<(u32, u32, String)>,
) {
    // The type partition and the two indexes exist only for the lost-cancel
    // walk, so a package that never calls `context.With…` pays for none of
    // them — which is most packages.
    let tracks_cancels = src_funcs.iter().any(|&fid| {
        let func = prog.functions.get(fid);
        func.live_blocks().any(|(_, block)| {
            block.instrs.iter().any(|&iid| {
                call_common(func.instrs.get(iid))
                    .is_some_and(|common| is_context_with_family(prog, common))
            })
        })
    });

    let classes = if tracks_cancels {
        TypeClasses::build(prog, src_funcs)
    } else {
        TypeClasses {
            class: HashMap::new(),
        }
    };
    let prog: &Program = prog;

    let mut field_addrs = Vec::new();
    let mut global_loads = Vec::new();
    if tracks_cancels {
        for &fid in src_funcs {
            let func = prog.functions.get(fid);
            for (_, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    match func.instrs.get(iid) {
                        InstrData::FieldAddr(fa) => {
                            field_addrs.push((fid, iid, fa.field, value_type_of(prog, func, fa.x)));
                        }
                        InstrData::UnOp(u) if u.op == Token::MUL => {
                            if let Value::Global(g) = u.x {
                                global_loads.push((fid, iid, g));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let ctx = Ctx {
        prog,
        classes,
        field_addrs,
        global_loads,
    };

    // gosec keys its issues by position and keeps the first, so a `for` loop
    // whose header is also a `context.WithCancel` line reports once.
    let mut issues: HashMap<Pos, &'static str> = HashMap::new();

    for &fid in src_funcs {
        let func = prog.functions.get(fid);
        if func.blocks.is_empty() {
            continue;
        }

        if function_has_request_context(prog, func) {
            let ctx_values = collect_context_values(prog, func);
            detect_unsafe_goroutines(&ctx, fid, func, &ctx_values, &mut issues);
            detect_loops_without_cancellation_guard(&ctx, func, &ctx_values, &mut issues);
        }

        detect_lost_cancel(&ctx, fid, func, &mut issues);
    }

    for (pos, msg) in issues {
        pending.push((pos.0 as u32, pos.0 as u32, msg.to_string()));
    }
}

fn add_issue(issues: &mut HashMap<Pos, &'static str>, pos: Pos, what: &'static str) {
    if !pos.is_valid() {
        return;
    }
    issues.entry(pos).or_insert(what);
}

// ---------------------------------------------------------------------------
// SSA/type helpers
// ---------------------------------------------------------------------------

/// `instr.(ssa.CallInstruction)`: `Call`, `Defer` and `Go` all satisfy it.
pub(crate) fn call_common(instr: &InstrData) -> Option<&CallCommon> {
    match instr {
        InstrData::Call(c) => Some(&c.call),
        InstrData::Defer(d) => Some(&d.call),
        InstrData::Go(g) => Some(&g.call),
        _ => None,
    }
}

/// `callInstr.Value()`: the call's result register, which is `nil` for `go` and
/// `defer` (they produce no value).
fn call_value(instr: &InstrData, iid: InstrId) -> Option<Value> {
    matches!(instr, InstrData::Call(_)).then_some(Value::Instr(iid))
}

/// go/ssa's `Value.Referrers()`, which is `nil` for the program-level values
/// (`Const`, `Global`, `Builtin`) and for package-level `Function`s. guff's
/// `compute_referrers` indexes every operand a function mentions, so the
/// distinction has to be reapplied here — it is what makes upstream need the
/// separate `isGlobalCalledInAnyFunc` scan.
fn go_referrers<'a>(prog: &Program, func: &'a Function, v: Value) -> &'a [InstrId] {
    match v {
        Value::Const(_) | Value::Global(_) | Value::Builtin(_) => &[],
        Value::Function(f) if prog.functions.get(f).parent.is_none() => &[],
        _ => referrers(func, v),
    }
}

/// `(callee.Pkg.Pkg.Path(), callee.Name())` for a static call.
fn callee_pkg_and_name<'a>(prog: &'a Program, common: &CallCommon) -> Option<(&'a str, &'a str)> {
    let fid = static_callee(common)?;
    Some((func_pkg_path(prog, fid)?, prog.functions.get(fid).name.as_str()))
}

/// `callee.Pkg.Pkg.Path()`, with the fallback guff needs.
///
/// go/ssa's `CreatePackage` materialises an imported package's **methods** as
/// members straight out of export data, so `(*http.Request).Context` carries a
/// `Pkg`. guff creates import members lazily and only for package-*level*
/// objects, so a method reached through `object_method` is a synthetic shell
/// with `pkg: None` — its declaring package is on the type-checker object the
/// shell was built from. Without this, `req.Context()` is not recognised as a
/// request-scoped context and the goroutine check goes quiet on every
/// `http.Handler`.
fn func_pkg_path(prog: &Program, fid: FuncId) -> Option<&str> {
    let f = prog.functions.get(fid);
    if let Some(pkg) = f.pkg {
        return Some(prog.package_arena.get(prog.packages.get(pkg).type_pkg()).path());
    }
    let p = f.object?.pkg(&prog.object_arena)?;
    Some(prog.package_arena.get(p).path())
}

/// The receiver type of `func`'s signature, or `None` when it is not a method.
fn func_recv_type(prog: &Program, func: &Function) -> Option<TypeId> {
    let sig = func.signature?;
    let TypeData::Signature(s) = prog.type_arena.get(sig) else {
        return None;
    };
    let recv = s.recv()?;
    recv.typ(&prog.object_arena)
}

/// `isContextType`: `context.Context` itself, or any interface with at least
/// four methods that has all of `Done` / `Err` / `Value` / `Deadline`.
fn is_context_type(prog: &Program, t: TypeId) -> bool {
    if let TypeData::Named(n) = prog.type_arena.get(t) {
        let obj = n.obj();
        if obj.name(&prog.object_arena) == "Context" {
            if let Some(p) = obj.pkg(&prog.object_arena) {
                if prog.package_arena.get(p).path() == CONTEXT_PKG {
                    return true;
                }
            }
        }
    }

    let u = t.underlying(&prog.type_arena);
    if !matches!(prog.type_arena.get(u), TypeData::Interface(_)) {
        return false;
    }
    let names = interface_method_names(prog, u);
    if names.len() < 4 {
        return false;
    }
    names.contains("Done") && names.contains("Err") && names.contains("Value")
        && names.contains("Deadline")
}

/// The complete method-set names of an interface, read-only.
///
/// `Interface.NumMethods` / `LookupFieldOrMethod` would compute (and cache) the
/// type set, which needs `&mut TypeArena`; walking the explicit methods and the
/// embedded elements gives the same names for every interface that has any.
fn interface_method_names(prog: &Program, iface: TypeId) -> HashSet<&str> {
    let mut names: HashSet<&str> = HashSet::new();
    let mut seen: HashSet<TypeId> = HashSet::new();
    let mut stack = vec![iface];
    while let Some(t) = stack.pop() {
        let u = t.underlying(&prog.type_arena);
        if !seen.insert(u) {
            continue;
        }
        let TypeData::Interface(i) = prog.type_arena.get(u) else {
            continue;
        };
        for k in 0..i.num_explicit_methods() {
            names.insert(i.explicit_method(k).name(&prog.object_arena));
        }
        for k in 0..i.num_embeddeds() {
            stack.push(i.embedded_type(k));
        }
    }
    names
}

/// `isHTTPRequestPointerType`: exactly `*net/http.Request`.
fn is_http_request_pointer_type(prog: &Program, t: TypeId) -> bool {
    let TypeData::Pointer(ptr) = prog.type_arena.get(t) else {
        return false;
    };
    let TypeData::Named(n) = prog.type_arena.get(ptr.elem()) else {
        return false;
    };
    let obj = n.obj();
    if obj.name(&prog.object_arena) != "Request" {
        return false;
    }
    obj.pkg(&prog.object_arena)
        .is_some_and(|pkg| prog.package_arena.get(pkg).path() == HTTP_PKG)
}

fn is_background_or_todo_call(prog: &Program, common: &CallCommon) -> bool {
    matches!(
        callee_pkg_and_name(prog, common),
        Some((CONTEXT_PKG, "Background" | "TODO"))
    )
}

fn is_background_or_todo_value(prog: &Program, func: &Function, v: Value) -> bool {
    let Value::Instr(i) = v else {
        return false;
    };
    let InstrData::Call(c) = func.instrs.get(i) else {
        return false;
    };
    is_background_or_todo_call(prog, &c.call)
}

fn is_context_with_family(prog: &Program, common: &CallCommon) -> bool {
    matches!(
        callee_pkg_and_name(prog, common),
        Some((CONTEXT_PKG, "WithCancel" | "WithTimeout" | "WithDeadline"))
    )
}

/// `isHTTPRequestContextCall`: `(*http.Request).Context()`.
fn is_http_request_context_call(prog: &Program, common: &CallCommon) -> bool {
    if common.method.is_some() {
        return false;
    }
    let Some(fid) = static_callee(common) else {
        return false;
    };
    let f = prog.functions.get(fid);
    if f.name != "Context" {
        return false;
    }
    if func_pkg_path(prog, fid) != Some(HTTP_PKG) {
        return false;
    }
    func_recv_type(prog, f).is_some_and(|t| is_http_request_pointer_type(prog, t))
}

/// `isContextDoneCall`: `ctx.Done()`, through an interface or a concrete type.
fn is_context_done_call(prog: &Program, func: &Function, common: &CallCommon) -> bool {
    if let Some(m) = common.method {
        if m.name(&prog.object_arena) != "Done" {
            return false;
        }
        return is_context_type(prog, value_type_of(prog, func, common.value));
    }

    let Some(fid) = static_callee(common) else {
        return false;
    };
    let callee = prog.functions.get(fid);
    if callee.name != "Done" {
        return false;
    }
    func_recv_type(prog, callee).is_some_and(|t| is_context_type(prog, t))
}

/// `looksLikeBlockingCall`: a hand-written list, not an effect analysis.
fn looks_like_blocking_call(prog: &Program, common: &CallCommon) -> bool {
    if let Some(m) = common.method {
        return matches!(
            m.name(&prog.object_arena),
            "Do" | "RoundTrip" | "QueryContext" | "ExecContext" | "Read" | "Write" | "Recv" | "Send"
        );
    }

    let Some((pkg, name)) = callee_pkg_and_name(prog, common) else {
        return false;
    };
    match pkg {
        "time" => name == "Sleep",
        "net/http" => matches!(name, "Get" | "Head" | "Post" | "PostForm"),
        "database/sql" => matches!(
            name,
            "Query" | "QueryContext" | "Exec" | "ExecContext" | "Begin" | "BeginTx"
        ),
        "os" => matches!(name, "ReadFile" | "WriteFile" | "Open" | "OpenFile"),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Per-function context inventory
// ---------------------------------------------------------------------------

/// `functionHasRequestContext`: reads the *signature*'s parameters, so a
/// method's receiver does not count.
fn function_has_request_context(prog: &Program, func: &Function) -> bool {
    let Some(sig) = func.signature else {
        return false;
    };
    let TypeData::Signature(s) = prog.type_arena.get(sig) else {
        return false;
    };
    let params = s.params();
    let n = tuple_len(&prog.type_arena, params);
    let Some(params) = params else {
        return false;
    };
    for i in 0..n {
        let Some(t) = tuple_at(&prog.type_arena, params, i).typ(&prog.object_arena) else {
            continue;
        };
        if is_context_type(prog, t) || is_http_request_pointer_type(prog, t) {
            return true;
        }
    }
    false
}

/// `collectContextValues`: parameters of context type (here the *SSA* params,
/// which do include a method's receiver), the result of `req.Context()`, and
/// the first component of every `context.With…` call.
fn collect_context_values(prog: &Program, func: &Function) -> HashSet<Value> {
    let mut vals: HashSet<Value> = HashSet::new();

    for (pid, p) in func.params.iter() {
        if is_context_type(prog, p.typ) {
            vals.insert(Value::Param(pid));
        }
    }

    for (_, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let instr = func.instrs.get(iid);
            let Some(common) = call_common(instr) else {
                continue;
            };

            if is_http_request_context_call(prog, common) {
                if let Some(v) = call_value(instr, iid) {
                    vals.insert(v);
                }
                continue;
            }

            if !is_context_with_family(prog, common) {
                continue;
            }
            let Some(tuple) = call_value(instr, iid) else {
                continue;
            };
            for &rid in go_referrers(prog, func, tuple) {
                if let InstrData::Extract(e) = func.instrs.get(rid) {
                    if e.index == 0 {
                        vals.insert(Value::Instr(rid));
                    }
                }
            }
        }
    }

    vals
}

// ---------------------------------------------------------------------------
// Detector 1: goroutines on a detached context
// ---------------------------------------------------------------------------

fn detect_unsafe_goroutines(
    ctx: &Ctx<'_>,
    fid: FuncId,
    func: &Function,
    context_values: &HashSet<Value>,
    issues: &mut HashMap<Pos, &'static str>,
) {
    let prog = ctx.prog;
    for (_, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let InstrData::Go(go) = func.instrs.get(iid) else {
                continue;
            };

            let mut has_background = go
                .call
                .args
                .iter()
                .any(|&a| is_background_or_todo_value(prog, func, a));

            if !has_background {
                for callee in resolve_go_call_targets(prog, fid, func, &go.call) {
                    if function_calls_background(prog, callee) {
                        has_background = true;
                        break;
                    }
                }
            }

            if has_background && !context_values.is_empty() {
                add_issue(issues, func.pos(iid), MSG_BACKGROUND);
            }
        }
    }
}

/// `resolveGoCallTargets` → `BaseAnalyzerState.ResolveFuncs`.
fn resolve_go_call_targets(
    prog: &Program,
    fid: FuncId,
    func: &Function,
    common: &CallCommon,
) -> Vec<FuncId> {
    resolve_value_funcs(prog, fid, func, common.value)
}

/// `BaseAnalyzerState.ResolveFuncs` on one value: the functions a callee
/// expression can denote, following closures, phis, `ChangeType`s and loads.
/// Shared with [`crate::gosec_g123`], which needs the same walk for
/// `tls.Config.GetConfigForClient`.
pub(crate) fn resolve_value_funcs(
    prog: &Program,
    fid: FuncId,
    func: &Function,
    v: Value,
) -> Vec<FuncId> {
    let mut out = Vec::new();
    let mut cache: HashSet<(FuncId, Value)> = HashSet::new();
    resolve_funcs(prog, fid, func, v, &mut out, &mut cache, 0);
    out
}

fn resolve_funcs(
    prog: &Program,
    fid: FuncId,
    func: &Function,
    v: Value,
    out: &mut Vec<FuncId>,
    cache: &mut HashSet<(FuncId, Value)>,
    depth: u32,
) {
    if depth > MAX_DEPTH || !cache.insert((fid, v)) {
        return;
    }
    match v {
        Value::Function(f) => out.push(f),
        Value::Instr(i) => match func.instrs.get(i) {
            InstrData::MakeClosure(mc) => out.push(mc.fn_),
            InstrData::Phi(p) => {
                for edge in p.edges.clone().into_iter().flatten() {
                    resolve_funcs(prog, fid, func, edge, out, cache, depth + 1);
                }
            }
            InstrData::ChangeType(c) => {
                let x = c.x;
                resolve_funcs(prog, fid, func, x, out, cache, depth + 1);
            }
            InstrData::UnOp(u) if u.op == Token::MUL => {
                let x = u.x;
                resolve_funcs(prog, fid, func, x, out, cache, depth + 1);
            }
            _ => {}
        },
        _ => {}
    }
}

fn function_calls_background(prog: &Program, fid: FuncId) -> bool {
    let func = prog.functions.get(fid);
    for (_, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let Some(common) = call_common(func.instrs.get(iid)) else {
                continue;
            };
            if is_background_or_todo_call(prog, common) {
                return true;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Detector 2: the cancel that is never called
// ---------------------------------------------------------------------------

fn detect_lost_cancel(
    ctx: &Ctx<'_>,
    fid: FuncId,
    func: &Function,
    issues: &mut HashMap<Pos, &'static str>,
) {
    for (_, block) in func.live_blocks() {
        for &iid in &block.instrs {
            let instr = func.instrs.get(iid);
            let Some(common) = call_common(instr) else {
                continue;
            };
            if !is_context_with_family(ctx.prog, common) {
                continue;
            }
            let Some(tuple) = call_value(instr, iid) else {
                continue;
            };
            let Some(cancel) = find_cancel_result(ctx.prog, func, tuple) else {
                continue;
            };
            if !is_cancel_called(ctx, fid, cancel) {
                add_issue(issues, func.pos(iid), MSG_LOST_CANCEL);
            }
        }
    }
}

/// The `cancel` half of `ctx, cancel := context.WithCancel(…)`.
fn find_cancel_result(prog: &Program, func: &Function, tuple: Value) -> Option<Value> {
    for &rid in go_referrers(prog, func, tuple) {
        let InstrData::Extract(e) = func.instrs.get(rid) else {
            continue;
        };
        if e.index != 1 {
            continue;
        }
        if is_cancel_func_type(prog, value_type_of(prog, func, Value::Instr(rid))) {
            return Some(Value::Instr(rid));
        }
    }
    None
}

/// `isCancelFuncType`: any `func()`, not `context.CancelFunc` by name.
fn is_cancel_func_type(prog: &Program, t: TypeId) -> bool {
    let u = t.underlying(&prog.type_arena);
    let TypeData::Signature(s) = prog.type_arena.get(u) else {
        return false;
    };
    tuple_len(&prog.type_arena, s.params()) == 0 && tuple_len(&prog.type_arena, s.results()) == 0
}

fn is_used_in_call(common: &CallCommon, target: Value) -> bool {
    common.value == target || common.args.contains(&target)
}

/// `isCancelCalled`: a breadth-first walk of everywhere the cancel value flows,
/// answering "does anything ever *call* it, or hand responsibility away?".
fn is_cancel_called(ctx: &Ctx<'_>, start_fn: FuncId, start: Value) -> bool {
    let prog = ctx.prog;
    let mut queue: VecDeque<(FuncId, Value)> = VecDeque::from([(start_fn, start)]);
    let mut visited: HashSet<(FuncId, Value)> = HashSet::new();

    while let Some((fid, current)) = queue.pop_front() {
        if !visited.insert((fid, current)) {
            continue;
        }
        let func = prog.functions.get(fid);

        for &rid in go_referrers(prog, func, current) {
            let instr = func.instrs.get(rid);
            if let Some(common) = call_common(instr) {
                if is_used_in_call(common, current) {
                    return true;
                }
                continue;
            }
            match instr {
                InstrData::Store(st) => {
                    if st.val != current {
                        continue;
                    }
                    if let Value::Instr(ai) = st.addr {
                        if let InstrData::FieldAddr(fa) = func.instrs.get(ai) {
                            let field = fa.field;
                            let x = fa.x;
                            let xty = value_type_of(prog, func, x);
                            if is_cancel_called_via_struct_field(ctx, field, xty)
                                || is_struct_field_returned_from_func(prog, func, x)
                                || is_field_called_in_any_func(ctx, field, xty)
                            {
                                return true;
                            }
                        }
                    }
                    if let Value::Global(g) = st.addr {
                        if is_global_called_in_any_func(ctx, g) {
                            return true;
                        }
                    }
                    queue.push_back((fid, st.addr));
                }
                InstrData::UnOp(u) if u.op == Token::MUL && u.x == current => {
                    queue.push_back((fid, Value::Instr(rid)));
                }
                InstrData::Phi(_) => queue.push_back((fid, Value::Instr(rid))),
                InstrData::ChangeType(c) if c.x == current => {
                    queue.push_back((fid, Value::Instr(rid)));
                }
                InstrData::Convert(c) if c.x == current => {
                    queue.push_back((fid, Value::Instr(rid)));
                }
                InstrData::MakeInterface(m) if m.x == current => {
                    queue.push_back((fid, Value::Instr(rid)));
                }
                InstrData::MakeClosure(mc) => {
                    // The cancel is captured: follow the matching free
                    // variable into the closure body.
                    let inner = mc.fn_;
                    for (i, &binding) in mc.bindings.iter().enumerate() {
                        if binding != current {
                            continue;
                        }
                        if let Some((fvid, _)) = prog.functions.get(inner).freevars.iter().nth(i) {
                            queue.push_back((inner, Value::FreeVar(fvid)));
                        }
                    }
                }
                // Returned to the caller: responsibility transferred.
                InstrData::Return(r) if r.results.contains(&current) => return true,
                _ => {}
            }
        }
    }

    false
}

/// `isStructFieldReturnedFromFunc`: the struct that owns the field is loaded
/// and returned. Note this needs a *load* — `return &T{cancel: cancel}` returns
/// the `Alloc` itself and so does not match, which is why every constructor
/// that parks a cancel in a fresh struct depends on one of the field walks.
fn is_struct_field_returned_from_func(prog: &Program, func: &Function, base: Value) -> bool {
    for &rid in go_referrers(prog, func, base) {
        let InstrData::UnOp(u) = func.instrs.get(rid) else {
            continue;
        };
        if u.op != Token::MUL {
            continue;
        }
        for &r2 in go_referrers(prog, func, Value::Instr(rid)) {
            if matches!(func.instrs.get(r2), InstrData::Return(_)) {
                return true;
            }
        }
    }
    false
}

/// `isCancelCalledViaStructField`: another method on the same receiver type
/// loads the same field off its receiver and calls it.
fn is_cancel_called_via_struct_field(ctx: &Ctx<'_>, field: usize, struct_ptr: TypeId) -> bool {
    let prog = ctx.prog;
    for &(fid, iid, f, _) in &ctx.field_addrs {
        if f != field {
            continue;
        }
        let func = prog.functions.get(fid);
        let Some(recv) = func_recv_type(prog, func) else {
            continue;
        };
        if !ctx.classes.same(recv, struct_ptr) {
            continue;
        }
        let Some((p0, _)) = func.params.iter().next() else {
            continue;
        };
        let InstrData::FieldAddr(fa) = func.instrs.get(iid) else {
            continue;
        };
        if !reaches_param(func, fa.x, Value::Param(p0), &mut HashSet::new()) {
            continue;
        }
        if is_field_value_called(prog, func, iid) {
            return true;
        }
    }
    false
}

/// `isFieldCalledInAnyFunc`: any function at all — closure included — loads the
/// same field off an identically-typed struct pointer and calls it. Covers
/// `s.cancel = cancel; defer s.cancel()`.
fn is_field_called_in_any_func(ctx: &Ctx<'_>, field: usize, struct_ptr: TypeId) -> bool {
    let prog = ctx.prog;
    for &(fid, iid, f, xty) in &ctx.field_addrs {
        if f != field || !ctx.classes.same(xty, struct_ptr) {
            continue;
        }
        if is_field_value_called(prog, prog.functions.get(fid), iid) {
            return true;
        }
    }
    false
}

/// `isGlobalCalledInAnyFunc`: a cancel parked in a package-level variable and
/// called from `init` / a shutdown hook / a signal handler.
fn is_global_called_in_any_func(ctx: &Ctx<'_>, global: GlobalId) -> bool {
    let prog = ctx.prog;
    for &(fid, iid, g) in &ctx.global_loads {
        if g != global {
            continue;
        }
        if is_value_called(prog, fid, Value::Instr(iid)) {
            return true;
        }
    }
    false
}

/// `reachesParam`: does `v` trace back to the receiver parameter?
fn reaches_param(
    func: &Function,
    v: Value,
    param: Value,
    seen: &mut HashSet<Value>,
) -> bool {
    if !seen.insert(v) {
        return false;
    }
    if v == param {
        return true;
    }
    let Value::Instr(i) = v else {
        return false;
    };
    match func.instrs.get(i) {
        InstrData::UnOp(u) => {
            let x = u.x;
            reaches_param(func, x, param, seen)
        }
        InstrData::Phi(p) => {
            let edges = p.edges.clone();
            edges
                .into_iter()
                .flatten()
                .any(|e| reaches_param(func, e, param, seen))
        }
        InstrData::FieldAddr(fa) => {
            let x = fa.x;
            reaches_param(func, x, param, seen)
        }
        _ => false,
    }
}

/// `isFieldValueCalled`: the value loaded out of a `FieldAddr` reaches a call.
fn is_field_value_called(prog: &Program, func: &Function, fa: InstrId) -> bool {
    for &rid in go_referrers(prog, func, Value::Instr(fa)) {
        let InstrData::UnOp(u) = func.instrs.get(rid) else {
            continue;
        };
        if u.op != Token::MUL {
            continue;
        }
        if is_loaded_value_called(prog, func, Value::Instr(rid)) {
            return true;
        }
    }
    false
}

/// The inner walk of `isFieldValueCalled` — narrower than [`is_value_called`]:
/// no closures, no conversions.
fn is_loaded_value_called(prog: &Program, func: &Function, start: Value) -> bool {
    let mut queue: VecDeque<Value> = VecDeque::from([start]);
    let mut visited: HashSet<Value> = HashSet::new();
    while let Some(cur) = queue.pop_front() {
        if !visited.insert(cur) {
            continue;
        }
        for &rid in go_referrers(prog, func, cur) {
            let instr = func.instrs.get(rid);
            if let Some(common) = call_common(instr) {
                if is_used_in_call(common, cur) {
                    return true;
                }
                continue;
            }
            match instr {
                InstrData::Phi(_) => queue.push_back(Value::Instr(rid)),
                InstrData::Store(st) if st.val == cur => queue.push_back(st.addr),
                InstrData::UnOp(u) if u.x == cur => queue.push_back(Value::Instr(rid)),
                _ => {}
            }
        }
    }
    false
}

/// `isValueCalled`: the wider walk used for globals.
fn is_value_called(prog: &Program, fid: FuncId, start: Value) -> bool {
    let mut queue: VecDeque<(FuncId, Value)> = VecDeque::from([(fid, start)]);
    let mut visited: HashSet<(FuncId, Value)> = HashSet::new();
    while let Some((cf, cur)) = queue.pop_front() {
        if !visited.insert((cf, cur)) {
            continue;
        }
        let func = prog.functions.get(cf);
        for &rid in go_referrers(prog, func, cur) {
            let instr = func.instrs.get(rid);
            if let Some(common) = call_common(instr) {
                if is_used_in_call(common, cur) {
                    return true;
                }
                continue;
            }
            match instr {
                InstrData::Phi(_) => queue.push_back((cf, Value::Instr(rid))),
                InstrData::Store(st) if st.val == cur => queue.push_back((cf, st.addr)),
                InstrData::UnOp(u) if u.x == cur => queue.push_back((cf, Value::Instr(rid))),
                InstrData::ChangeType(c) if c.x == cur => queue.push_back((cf, Value::Instr(rid))),
                InstrData::Convert(c) if c.x == cur => queue.push_back((cf, Value::Instr(rid))),
                InstrData::MakeInterface(m) if m.x == cur => {
                    queue.push_back((cf, Value::Instr(rid)))
                }
                InstrData::MakeClosure(mc) => {
                    let inner = mc.fn_;
                    for (i, &binding) in mc.bindings.iter().enumerate() {
                        if binding != cur {
                            continue;
                        }
                        if let Some((fvid, _)) = prog.functions.get(inner).freevars.iter().nth(i) {
                            queue.push_back((inner, Value::FreeVar(fvid)));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Detector 3: a loop with no way out and no Done guard
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct BlockFeatures {
    has_done_guard: bool,
    has_blocking: bool,
}

struct LoopRegion {
    blocks: Vec<BlockId>,
    has_external_exit: bool,
    pos: Pos,
}

fn detect_loops_without_cancellation_guard(
    ctx: &Ctx<'_>,
    func: &Function,
    context_values: &HashSet<Value>,
    issues: &mut HashMap<Pos, &'static str>,
) {
    if context_values.is_empty() || func.blocks.is_empty() {
        return;
    }

    let mut features: HashMap<BlockId, BlockFeatures> = HashMap::new();
    for (bid, block) in func.live_blocks() {
        features.insert(bid, analyze_block_features(ctx.prog, func, &block.instrs));
    }

    for region in find_loop_regions(func) {
        if region.has_external_exit {
            continue;
        }
        let mut has_done_guard = false;
        let mut has_blocking = false;
        for bid in &region.blocks {
            let f = features.get(bid).copied().unwrap_or_default();
            has_done_guard |= f.has_done_guard;
            has_blocking |= f.has_blocking;
            if has_done_guard && has_blocking {
                break;
            }
        }
        if has_done_guard || !has_blocking {
            continue;
        }
        add_issue(issues, region.pos, MSG_LOOP_WITHOUT_DONE);
    }
}

/// `analyzeBlockFeatures`. Upstream's `if !ok` arm — the one that would mark a
/// bare `go` statement blocking — is unreachable: `*ssa.Go`, `*ssa.Call` and
/// `*ssa.Defer` all satisfy `ssa.CallInstruction`, so the type switch never
/// runs. What is ported here is the reachable half.
fn analyze_block_features(prog: &Program, func: &Function, instrs: &[InstrId]) -> BlockFeatures {
    let mut f = BlockFeatures::default();
    for &iid in instrs {
        let Some(common) = call_common(func.instrs.get(iid)) else {
            continue;
        };
        if is_context_done_call(prog, func, common) {
            f.has_done_guard = true;
        }
        if looks_like_blocking_call(prog, common) {
            f.has_blocking = true;
        }
    }
    f
}

/// `findLoopRegions`: Tarjan's SCCs over the CFG, keeping the components that
/// contain a cycle. Written iteratively — the recursion is over basic blocks,
/// and a generated parser or a long `switch` chain reaches depths a Rust stack
/// would not survive — but with an explicit frame per call so the traversal,
/// and therefore each component's block order (which picks the reported
/// position), matches the recursive original.
fn find_loop_regions(func: &Function) -> Vec<LoopRegion> {
    let mut regions: Vec<LoopRegion> = Vec::new();
    let mut index: u32 = 0;
    let mut stack: Vec<BlockId> = Vec::new();
    let mut on_stack: HashSet<BlockId> = HashSet::new();
    let mut index_map: HashMap<BlockId, u32> = HashMap::new();
    let mut low_link: HashMap<BlockId, u32> = HashMap::new();

    let roots: Vec<BlockId> = func.live_blocks().map(|(bid, _)| bid).collect();
    for root in roots {
        if index_map.contains_key(&root) {
            continue;
        }

        index_map.insert(root, index);
        low_link.insert(root, index);
        index += 1;
        stack.push(root);
        on_stack.insert(root);
        let mut frames: Vec<(BlockId, usize)> = vec![(root, 0)];

        while let Some(&(v, next_succ)) = frames.last() {
            let succs = &func.blocks.get(v).succs;
            if next_succ < succs.len() {
                frames.last_mut().unwrap().1 += 1;
                let w = succs[next_succ];
                if func.blocks.get(w).deleted {
                    continue;
                }
                if !index_map.contains_key(&w) {
                    index_map.insert(w, index);
                    low_link.insert(w, index);
                    index += 1;
                    stack.push(w);
                    on_stack.insert(w);
                    frames.push((w, 0));
                } else if on_stack.contains(&w) {
                    let wi = index_map[&w];
                    let lv = low_link[&v];
                    if wi < lv {
                        low_link.insert(v, wi);
                    }
                }
                continue;
            }

            if low_link[&v] == index_map[&v] {
                let mut scc: Vec<BlockId> = Vec::new();
                let mut scc_set: HashSet<BlockId> = HashSet::new();
                loop {
                    let n = stack.pop().expect("tarjan stack underflow");
                    on_stack.remove(&n);
                    scc.push(n);
                    scc_set.insert(n);
                    if n == v {
                        break;
                    }
                }
                if is_loop_scc(func, &scc, &scc_set) {
                    regions.push(build_region(func, scc, &scc_set, v));
                }
            }

            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                let lv = low_link[&v];
                if lv < low_link[&parent] {
                    low_link.insert(parent, lv);
                }
            }
        }
    }

    regions
}

fn is_loop_scc(func: &Function, scc: &[BlockId], scc_set: &HashSet<BlockId>) -> bool {
    if scc.len() > 1 {
        return true;
    }
    let Some(&b) = scc.first() else {
        return false;
    };
    func.blocks
        .get(b)
        .succs
        .iter()
        .any(|&s| s == b || scc_set.contains(&s))
}

fn build_region(
    func: &Function,
    scc: Vec<BlockId>,
    scc_set: &HashSet<BlockId>,
    root: BlockId,
) -> LoopRegion {
    let mut has_external_exit = false;
    let mut pos = guff::NO_POS;
    for &b in &scc {
        if !pos.is_valid() {
            if let Some(&iid) = first_real_instr(func, b) {
                pos = func.pos(iid);
            }
        }
        if func
            .blocks
            .get(b)
            .succs
            .iter()
            .any(|s| !scc_set.contains(s))
        {
            has_external_exit = true;
            break;
        }
    }
    if !pos.is_valid() {
        for &iid in &func.blocks.get(root).instrs {
            if matches!(func.instrs.get(iid), InstrData::DebugRef(_)) {
                continue;
            }
            if func.pos(iid).is_valid() {
                pos = func.pos(iid);
                break;
            }
        }
    }
    LoopRegion {
        blocks: scc,
        has_external_exit,
        pos,
    }
}

/// `block.Instrs[0]` as a `buildssa` build would see it: guff's SSA carries
/// `DebugRef` pseudo-instructions that `ssa.BuilderMode(0)` never emits.
fn first_real_instr(func: &Function, b: BlockId) -> Option<&InstrId> {
    func.blocks
        .get(b)
        .instrs
        .iter()
        .find(|&&iid| !matches!(func.instrs.get(iid), InstrData::DebugRef(_)))
}
