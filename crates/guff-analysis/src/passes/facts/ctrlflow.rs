//! Port of the no-return half of `go/analysis/passes/ctrlflow`.
//!
//! Upstream `ctrlflow` builds a `go/cfg` control-flow graph per function and
//! records, as an **object fact**, that a function "cannot return normally":
//! its CFG has no live block holding a `return` (a `defer` counts as one, since
//! `recover` can turn a panic into a return) and control does not fall off the
//! end. The property is inductive over the static call graph — `os.Exit` is
//! no-return because it ends in `syscall.Exit`, `log.Fatal` because it ends in
//! `os.Exit`, `(*testing.common).Skip` because it ends in `runtime.Goexit` —
//! and it crosses package boundaries through the fact.
//!
//! golangci-lint 2.12.2 reaches this through `buildssa`, which since
//! `golang.org/x/tools` v0.44.0 calls `prog.SetNoReturn(cfgs.NoReturn)`:
//! `emitCall` then emits a `Panic` after a static call to a no-return callee
//! and starts an `unreachable.noreturn` block, so **every SSA linter** sees the
//! statements after `t.Skip(…)` as dead code.
//!
//! # What this pass is, and is not
//!
//! It computes the same predicate, but as a structural walk of the statement
//! tree rather than a `go/cfg` port: [`Flow`] carries "is a `return`/`defer`
//! reachable here" and "does control reach the next statement", and
//! [`Ctx::call_may_return`] is upstream's `callMayReturn`. The two agree on
//! every shape that decides whether a *function* is no-return; they can differ
//! where `goto` reaches a `return` that no structural path does, which is
//! recorded in the DEFERRED list below rather than papered over.
//!
//! guff's SSA builder does not consume this yet (`guff_ssa::emit::emit_call`
//! still carries its DEFERRED note). The one consumer today is `unparam`, which
//! reads it to stop counting uses, call sites and returns that upstream's IR
//! never reaches. Wiring it into `Program` is the general form and belongs with
//! its own measurement — the SA5011 and `lostcancel` name-based abort lists
//! exist because the cut is missing, and both have to be re-measured when it
//! arrives.
//!
//! # Where the induction stops
//!
//! Upstream analyses every dependency, so the fact behind `os.Exit` is one it
//! computed from `os`'s own source. guff type-checks only the packages being
//! linted: an import resolves to a metadata stub with no type artifacts, and
//! `guff_runner::action` skips scheduling a fact producer on it (the same-module
//! expansion exists, but `guff_lint` turns it on for `contextcheck` alone —
//! gating it on `fact_types` cost hundreds of MB of peak RSS on prometheus and
//! changed no findings). So the induction runs *inside* a package and the base
//! cases come from [`known_intrinsic`]'s table, which names the standard-library
//! aborts upstream infers.
//!
//! What is left is a no-return helper in another package: reported by upstream,
//! silent here. Measured on a two-package module — `dep.Die()` ending in
//! `os.Exit` — and recorded in `docs/COMPAT-HARDENING.md` §7 rather than papered
//! over. Every other shape in that grid agrees.
//!
//! DEFERRED vs. `go/cfg`:
//! - a function whose body contains a `goto` is not classified at all
//!   ([`contains_goto`]): the structural walk cannot follow the edge, and
//!   without it `goto end; os.Exit(1); end: return` reads as no-return when
//!   upstream — which does follow it — says the `return` is live. Measured, as
//!   a guff-only report, before the guard went in. What it costs is a genuine
//!   no-return function that happens to contain a `goto`: silence, which is
//!   what guff did before this pass existed.
//! - a `break` out of a nested `switch`/`select`/loop is matched with
//!   [`has_break_list`], the type-checker's own predicate, which is exact for
//!   labelled breaks and for the unlabelled break of the *enclosing*
//!   statement — the two cases `go/cfg` models with real edges.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, Decl, Expr, File, FuncLit, Stmt};
use guff::position::Pos;
use guff::walk::{inspect, NodeRef};
use guff_types::arena::{ObjectData, ObjectId, TypeData};
use guff_types::return_check::has_break_list;
use guff_types::Info;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::code::unparen;
use crate::facts::{Fact, FactTypeId};
use crate::pass::Pass;

/// Fact attached to functions that cannot return normally.
///
/// Port of `ctrlflow.noReturn`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoReturn;

impl Fact for NoReturn {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }

    fn type_name(&self) -> &'static str {
        "NoReturn"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::Value::Object(serde_json::Map::new())
    }
}

fn decode_no_return(_payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    Some(Box::new(NoReturn))
}

/// Register the [`NoReturn`] fact decoder (called from builtin init).
pub(crate) fn register_ctrlflow_fact_decoder() {
    crate::fact_codec::register_fact_decoder("NoReturn", decode_no_return);
}

/// The answers `ctrlflow` is asked for: "does this callee ever return?".
///
/// Upstream's `CFGs.NoReturn` is "defined for at least all function symbols
/// that appear as the static callee of a `CallExpr` in the current package,
/// even if the callee was imported from a dependency" — which is exactly the
/// set this pass resolves while walking, so the map is built the same way.
#[derive(Clone, Default)]
pub struct CtrlFlowResult {
    no_return: HashSet<ObjectId>,
}

impl CtrlFlowResult {
    /// Whether the function object `obj` cannot return normally.
    pub fn is_no_return(&self, obj: ObjectId) -> bool {
        self.no_return.contains(&obj)
    }

    /// Whether `call`, appearing as a statement, cannot return — the question
    /// `go/cfg`'s builder asks through `mayReturn`.
    pub fn call_never_returns(&self, info: &Info, call: &CallExpr) -> bool {
        if is_panic_builtin_call(call) {
            return true;
        }
        match callee_object(info, call) {
            Some(obj) => self.no_return.contains(&obj),
            None => false,
        }
    }
}

/// Reachability of a statement (or statement list), in the two facts `go/cfg`
/// reduces to when it answers `NoReturn`.
#[derive(Clone, Copy)]
struct Flow {
    /// A `return` — or a `defer`, which `go/cfg` marks `returns` because a
    /// deferred `recover` can turn a panic into one — is reachable here.
    returns: bool,
    /// Control reaches the statement after this one.
    falls: bool,
}

impl Flow {
    const RUNS_ON: Self = Flow {
        returns: false,
        falls: true,
    };
    const STOPS: Self = Flow {
        returns: false,
        falls: false,
    };
}

/// `callMayReturn`'s `panic` case.
///
/// Upstream compares `info.Uses[id]` against the universe `panic`; this is the
/// structural approximation `guff_types::return_check` and `unparam` already
/// use, which differs only for a user-defined `panic` shadowing the builtin.
fn is_panic_builtin_call(call: &CallExpr) -> bool {
    matches!(unparen(&call.fun), Expr::Ident(id) if id.name == "panic")
}

/// `typeutil.Callee`, narrowed to what a static callee can be: the object an
/// identifier or selector denotes, looking through a generic instantiation.
fn callee_object(info: &Info, call: &CallExpr) -> Option<ObjectId> {
    let mut fun = unparen(&call.fun);
    // `f[int](x)` — look through the instantiation to the generic function.
    loop {
        match fun {
            Expr::IndexExpr(ix) => fun = unparen(&ix.x),
            Expr::IndexListExpr(ix) => fun = unparen(&ix.x),
            _ => break,
        }
    }
    match fun {
        Expr::Ident(id) => info.uses.get(&id.id).copied(),
        Expr::SelectorExpr(sel) => info.uses.get(&sel.sel.id).copied(),
        _ => None,
    }
}

fn analyzer_impl() -> Analyzer {
    register_ctrlflow_fact_decoder();
    Analyzer {
        name: "fact_ctrlflow",
        doc: "record functions that cannot return normally",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/ctrlflow",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![FactTypeId::of::<NoReturn>()],
    }
}

/// The ctrlflow no-return fact analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(analyzer_impl)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let (result, to_export) = {
        let Some(info) = pass.types_info() else {
            return Ok(Some(Box::new(CtrlFlowResult::default())));
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return Ok(Some(Box::new(CtrlFlowResult::default())));
        };
        let mut ctx = Ctx {
            pass,
            info,
            types: &artifacts.types,
            objects: &artifacts.objects,
            packages: &artifacts.packages,
            decls: HashMap::new(),
            started: HashSet::new(),
            memo: HashMap::new(),
            local: Vec::new(),
        };
        ctx.collect_decls(pass.files());
        ctx.build_all(pass.files());
        let no_return = ctx
            .memo
            .iter()
            .filter_map(|(obj, &nr)| nr.then_some(*obj))
            .collect();
        (CtrlFlowResult { no_return }, ctx.local)
    };
    for obj in to_export {
        pass.export_object_fact(obj, Box::new(NoReturn));
    }
    Ok(Some(Box::new(result)))
}

struct Ctx<'a, 'p> {
    pass: &'a Pass<'p>,
    info: &'a Info,
    types: &'a guff_types::arena::TypeArena,
    objects: &'a guff_types::arena::ObjectArena,
    packages: &'a guff_types::arena::PackageArena,
    /// Function bodies declared in this package, by object. (Go: `funcDecls`.)
    decls: HashMap<ObjectId, &'a BlockStmt>,
    /// Cycle breaker for the recursion `callMayReturn` drives. (Go: `started`.)
    started: HashSet<ObjectId>,
    /// Answer per callee, imported ones included. (Go: `c.noReturn`.)
    memo: HashMap<ObjectId, bool>,
    /// Objects declared here that are no-return: the facts to export.
    local: Vec<ObjectId>,
}

impl<'a, 'p> Ctx<'a, 'p> {
    fn collect_decls(&mut self, files: &'a [File]) {
        for file in files {
            for decl in &file.decls {
                let Decl::FuncDecl(fd) = decl else {
                    continue;
                };
                let Some(body) = &fd.body else {
                    continue;
                };
                let Some(Some(obj)) = self.info.defs.get(&fd.name.id).copied() else {
                    continue;
                };
                self.decls.insert(obj, body);
            }
        }
    }

    /// Upstream builds a CFG for every declared function (which exports the
    /// facts) and then one for every function literal. The literals cannot
    /// carry a fact, but building them resolves the callees named inside them,
    /// which is what leaves `NoReturn` "defined for at least all function
    /// symbols that appear as the static callee of a CallExpr".
    fn build_all(&mut self, files: &'a [File]) {
        let decl_objs: Vec<ObjectId> = files
            .iter()
            .flat_map(|f| f.decls.iter())
            .filter_map(|d| match d {
                Decl::FuncDecl(fd) => self.info.defs.get(&fd.name.id).copied().flatten(),
                _ => None,
            })
            .collect();
        for obj in decl_objs {
            self.build_decl(obj);
        }
        // Resolve the callee of every call that appears as a statement, so a
        // consumer asking about one gets an answer rather than a default.
        let mut calls: Vec<&'a CallExpr> = Vec::new();
        for file in files {
            inspect(NodeRef::File(file), |n| {
                if let Some(NodeRef::ExprStmt(e)) = n {
                    if let Expr::CallExpr(call) = unparen(&e.x) {
                        calls.push(call);
                    }
                }
                true
            });
        }
        for call in calls {
            let _ = self.call_may_return(call);
        }
    }

    /// (Go: `buildDecl`.)
    fn build_decl(&mut self, obj: ObjectId) -> bool {
        if let Some(&known) = self.memo.get(&obj) {
            return known;
        }
        if !self.started.insert(obj) {
            // Cycle: upstream breaks it arbitrarily but deterministically by
            // treating the in-progress function as one that may return.
            return false;
        }
        let no_return = match known_intrinsic(self.func_full_name(obj).as_deref()) {
            Some(known) => known,
            None => match self.decls.get(&obj).copied() {
                Some(body) => self.body_no_return(body),
                None => false,
            },
        };
        self.memo.insert(obj, no_return);
        if no_return && self.decls.contains_key(&obj) {
            self.local.push(obj);
        }
        no_return
    }

    /// (Go: `callMayReturn`.)
    fn call_may_return(&mut self, call: &CallExpr) -> bool {
        if is_panic_builtin_call(call) {
            return false;
        }
        let Some(obj) = callee_object(self.info, call) else {
            return true; // callee not statically known; be conservative
        };
        if !self.is_static_func(obj) {
            return true;
        }
        !self.no_return_object(obj)
    }

    fn no_return_object(&mut self, obj: ObjectId) -> bool {
        if self.decls.contains_key(&obj) {
            return self.build_decl(obj);
        }
        if let Some(&known) = self.memo.get(&obj) {
            return known;
        }
        // Not declared here: upstream reads the fact its own analysis of that
        // package exported. `knownIntrinsic` is consulted at the call site too,
        // for the same reason `purity` consults `pureStdlib` there — the base
        // case of the induction must hold even when the defining package was
        // not walked (`runtime.Goexit` has an ordinary body; only the table
        // says it never returns).
        let no_return = match known_intrinsic(self.func_full_name(obj).as_deref()) {
            Some(known) => known,
            None => {
                let mut fact = NoReturn;
                self.pass.import_object_fact(obj, &mut fact)
            }
        };
        self.memo.insert(obj, no_return);
        no_return
    }

    /// `typeutil.StaticCallee` minus the `Callee` half: a function object whose
    /// receiver, if any, is not an interface (an interface method has no static
    /// callee, which is why `t.Skip` on a `testing.TB` does not cut).
    fn is_static_func(&self, obj: ObjectId) -> bool {
        if !matches!(self.objects.get(obj), ObjectData::Func(_)) {
            return false;
        }
        let Some(sig) = obj.typ(self.objects) else {
            return false;
        };
        let Some(recv) = guff_types::signature::signature_recv(self.types, sig) else {
            return true;
        };
        let Some(recv_type) = recv.typ(self.objects) else {
            return true;
        };
        let under = recv_type.underlying(self.types);
        !matches!(self.types.get(under), TypeData::Interface(_))
    }

    /// `types.Func.FullName()`: `path.Name`, or `(recv).Name` for a method.
    fn func_full_name(&self, obj: ObjectId) -> Option<String> {
        if !matches!(self.objects.get(obj), ObjectData::Func(_)) {
            return None;
        }
        Some(crate::code::type_func_name(
            self.types,
            self.objects,
            self.packages,
            obj,
        ))
    }

    /// `cfg.New(body, callMayReturn).NoReturn()`: no live block returns, and
    /// control does not fall off the end.
    fn body_no_return(&mut self, body: &'a BlockStmt) -> bool {
        if contains_goto(&body.list) {
            return false;
        }
        let flow = walk_list(self, &body.list, "");
        !(flow.returns || flow.falls)
    }
}

impl FlowSink for Ctx<'_, '_> {
    fn may_return(&mut self, call: &CallExpr) -> bool {
        self.call_may_return(call)
    }
}

/// What the shared walk needs from its caller: the `mayReturn` oracle `go/cfg`
/// is built with, and somewhere to put the ranges it finds unreachable.
trait FlowSink {
    fn may_return(&mut self, call: &CallExpr) -> bool;

    /// A statement range no live block covers. Only [`DeadCode`] records these.
    fn dead(&mut self, _from: Pos, _to: Pos) {}
}

fn walk_list<S: FlowSink + ?Sized>(sink: &mut S, list: &[Stmt], label: &str) -> Flow {
    let mut out = Flow::RUNS_ON;
    for (i, stmt) in list.iter().enumerate() {
        if !out.falls {
            // `go/cfg` builds these statements into a block nothing reaches,
            // and `cfg.New`'s liveness sweep leaves it dead.
            let last = list.last().expect("iterating a non-empty list");
            sink.dead(stmt.pos(), last.end());
            let _ = i;
            break;
        }
        let flow = walk_stmt(sink, stmt, label);
        out.returns |= flow.returns;
        out.falls = flow.falls;
    }
    out
}

fn walk_stmt<S: FlowSink + ?Sized>(sink: &mut S, stmt: &Stmt, label: &str) -> Flow {
    match stmt {
        Stmt::ReturnStmt(_) => Flow {
            returns: true,
            falls: false,
        },
        // go/cfg: "assume conservatively that this behaves like
        // `defer func() { recover() }`, so any subsequent panic may act like a
        // return".
        Stmt::DeferStmt(_) => Flow {
            returns: true,
            falls: true,
        },
        Stmt::ExprStmt(e) => match unparen(&e.x) {
            Expr::CallExpr(call) if !sink.may_return(call) => Flow::STOPS,
            _ => Flow::RUNS_ON,
        },
        Stmt::BranchStmt(_) => Flow::STOPS,
        Stmt::BlockStmt(b) => walk_list(sink, &b.list, ""),
        Stmt::LabeledStmt(l) => walk_stmt(sink, &l.stmt, l.label.name.as_str()),
        Stmt::IfStmt(s) => {
            let then = walk_list(sink, &s.body.list, "");
            let other = match &s.else_ {
                Some(e) => walk_stmt(sink, e, ""),
                None => Flow::RUNS_ON,
            };
            Flow {
                returns: then.returns || other.returns,
                falls: then.falls || other.falls,
            }
        }
        Stmt::ForStmt(s) => {
            let body = walk_list(sink, &s.body.list, "");
            // `for {}` with no break never reaches the statement after it.
            let endless = s.cond.is_none() && !has_break_list(&s.body.list, label, true);
            Flow {
                returns: body.returns,
                falls: !endless,
            }
        }
        Stmt::RangeStmt(s) => {
            let body = walk_list(sink, &s.body.list, "");
            Flow {
                returns: body.returns,
                falls: true,
            }
        }
        Stmt::SwitchStmt(s) => walk_switch(sink, &s.body.list, label),
        Stmt::TypeSwitchStmt(s) => walk_switch(sink, &s.body.list, label),
        Stmt::SelectStmt(s) => walk_select(sink, &s.body.list, label),
        _ => Flow::RUNS_ON,
    }
}

/// A `switch` is left when a clause body falls out of it, when a `break` jumps
/// out, or — with no `default` — when no case matched at all.
fn walk_switch<S: FlowSink + ?Sized>(sink: &mut S, clauses: &[Stmt], label: &str) -> Flow {
    let mut returns = false;
    let mut falls = false;
    let mut has_default = false;
    for clause in clauses {
        let Stmt::CaseClause(cc) = clause else {
            continue;
        };
        if cc.list.is_empty() {
            has_default = true;
        }
        let body = walk_list(sink, &cc.body, "");
        returns |= body.returns;
        if body.falls || has_break_list(&cc.body, label, true) {
            falls = true;
        }
    }
    Flow {
        returns,
        falls: falls || !has_default,
    }
}

/// Whether `list` holds a `goto`, looking through statements only (a `goto`
/// inside a nested function literal is that function's business).
///
/// `go/cfg` gives a `goto` a real edge to its label; the walk here stops at it,
/// which can make a live `return` invisible. Rather than guess, neither
/// [`Ctx::body_no_return`] nor [`DeadCode`] classifies a body that contains one.
fn contains_goto(list: &[Stmt]) -> bool {
    list.iter().any(stmt_contains_goto)
}

fn stmt_contains_goto(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::BranchStmt(b) => b.tok == guff::token::Token::GOTO,
        Stmt::BlockStmt(b) => contains_goto(&b.list),
        Stmt::LabeledStmt(l) => stmt_contains_goto(&l.stmt),
        Stmt::IfStmt(s) => {
            contains_goto(&s.body.list)
                || s.else_.as_ref().is_some_and(|e| stmt_contains_goto(e))
        }
        Stmt::ForStmt(s) => contains_goto(&s.body.list),
        Stmt::RangeStmt(s) => contains_goto(&s.body.list),
        Stmt::SwitchStmt(s) => contains_goto(&s.body.list),
        Stmt::TypeSwitchStmt(s) => contains_goto(&s.body.list),
        Stmt::SelectStmt(s) => contains_goto(&s.body.list),
        Stmt::CaseClause(c) => contains_goto(&c.body),
        Stmt::CommClause(c) => contains_goto(&c.body),
        _ => false,
    }
}

/// A `select` blocks until one of its cases runs, so it is left only through a
/// clause body — there is no "nothing matched" path.
fn walk_select<S: FlowSink + ?Sized>(sink: &mut S, clauses: &[Stmt], label: &str) -> Flow {
    let mut returns = false;
    let mut falls = false;
    for clause in clauses {
        let Stmt::CommClause(cc) = clause else {
            continue;
        };
        let body = walk_list(sink, &cc.body, "");
        returns |= body.returns;
        if body.falls || has_break_list(&cc.body, label, true) {
            falls = true;
        }
    }
    Flow { returns, falls }
}

/// The source ranges of the statements upstream's IR never reaches.
///
/// `buildssa` gives every SSA linter this for free: the `Panic` `emitCall`
/// inserts after a no-return callee leaves the rest of the block unreachable,
/// and `deleteUnreachableBlocks` removes it, so the instructions are simply not
/// there. guff's SSA still builds them (see `guff_ssa::emit::emit_call`), so a
/// consumer that has to agree with upstream asks here whether a position is
/// inside dead code.
///
/// A statement that is dead is dead whole: a `func` literal written inside one
/// is never reached by a `MakeClosure`, so `ssautil.AllFunctions` does not
/// reach it either and none of its calls are call sites.
#[derive(Clone, Default)]
pub struct DeadCode {
    /// Disjoint, sorted by start.
    ranges: Vec<(Pos, Pos)>,
}

impl DeadCode {
    /// Walks every function body in `files` — declarations and literals — and
    /// records what `go/cfg`'s liveness sweep would leave dead.
    pub fn build(result: &CtrlFlowResult, info: &Info, files: &[File]) -> Self {
        let mut marker = DeadMarker {
            result,
            info,
            ranges: Vec::new(),
        };
        for file in files {
            for decl in &file.decls {
                if let Decl::FuncDecl(fd) = decl {
                    if let Some(body) = &fd.body {
                        if !contains_goto(&body.list) {
                            walk_list(&mut marker, &body.list, "");
                        }
                    }
                }
            }
        }
        let mut lits: Vec<&FuncLit> = Vec::new();
        for file in files {
            inspect(NodeRef::File(file), |n| {
                if let Some(NodeRef::FuncLit(lit)) = n {
                    lits.push(lit);
                }
                true
            });
        }
        for lit in lits {
            if !contains_goto(&lit.body.list) {
                walk_list(&mut marker, &lit.body.list, "");
            }
        }
        let mut ranges = marker.ranges;
        ranges.sort_by_key(|(from, _)| *from);
        DeadCode { ranges }
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    /// Whether `pos` falls inside a statement no live block covers.
    pub fn contains(&self, pos: Pos) -> bool {
        // Ranges nest (a dead statement inside a dead statement), so the last
        // range starting at or before `pos` is not necessarily the one that
        // covers it; scan back over the ones that can.
        let idx = self.ranges.partition_point(|(from, _)| *from <= pos);
        self.ranges[..idx].iter().rev().any(|(_, to)| pos <= *to)
    }
}

struct DeadMarker<'a> {
    result: &'a CtrlFlowResult,
    info: &'a Info,
    ranges: Vec<(Pos, Pos)>,
}

impl FlowSink for DeadMarker<'_> {
    fn may_return(&mut self, call: &CallExpr) -> bool {
        !self.result.call_never_returns(self.info, call)
    }

    fn dead(&mut self, from: Pos, to: Pos) {
        self.ranges.push((from, to));
    }
}

/// The base cases of the induction, by `types.Func.FullName()`.
///
/// Two lists, for two different reasons.
///
/// [`KNOWN_INTRINSIC`] is `ctrlflow.knownIntrinsic`, verbatim: functions whose
/// *body* does not say what the runtime does. Upstream consults it before
/// building a CFG, so it holds however the defining package is reached.
///
/// [`STDLIB_NO_RETURN`] is the part guff has to name that upstream infers.
/// Upstream analyses every dependency and exports a fact from it; guff only
/// runs analyzers on the packages being linted, so a call into `os`, `log` or
/// `testing` has no fact behind it (`SA1019` re-parses dependency sources for
/// the same reason, and `purity` names `pureStdlib` at the call site for it).
/// Each entry here was measured against golangci-lint 2.12.2 — one
/// `func f(p bool) { noop(); <call>; if p { … } }` per name, `unparam` reporting
/// `p` exactly when the call cuts — rather than read off the source.
///
/// What is left out is what guff cannot name: a no-return helper in a package
/// that is *not* being linted. Inside one run this still works, because a
/// package of the same module is analysed too and its fact is imported — only
/// a helper in a third-party dependency is missed.
fn known_intrinsic(full_name: Option<&str>) -> Option<bool> {
    let name = full_name?;
    if KNOWN_INTRINSIC.binary_search(&name).is_ok() || STDLIB_NO_RETURN.binary_search(&name).is_ok()
    {
        return Some(true);
    }
    if ALWAYS_RETURNS.binary_search(&name).is_ok() {
        return Some(false);
    }
    None
}

/// `ctrlflow.knownIntrinsic`'s no-return half, verbatim. Sorted; keep it that
/// way, the lookup is a binary search.
const KNOWN_INTRINSIC: &[&str] = &[
    "(*github.com/sirupsen/logrus.Entry).Panicf",
    "(*github.com/sirupsen/logrus.Entry).Panicln",
    "(*github.com/sirupsen/logrus.Logger).Exit",
    "(*github.com/sirupsen/logrus.Logger).Panic",
    "(*github.com/sirupsen/logrus.Logger).Panicf",
    "(*github.com/sirupsen/logrus.Logger).Panicln",
    "(*go.uber.org/zap.Logger).Fatal",
    "(*go.uber.org/zap.Logger).Panic",
    "(*go.uber.org/zap.SugaredLogger).Fatal",
    "(*go.uber.org/zap.SugaredLogger).Fatalf",
    "(*go.uber.org/zap.SugaredLogger).Fatalw",
    "(*go.uber.org/zap.SugaredLogger).Panic",
    "(*go.uber.org/zap.SugaredLogger).Panicf",
    "(*go.uber.org/zap.SugaredLogger).Panicw",
    "k8s.io/klog.Exit",
    "k8s.io/klog.ExitDepth",
    "k8s.io/klog.Exitf",
    "k8s.io/klog.Exitln",
    "k8s.io/klog.Fatal",
    "k8s.io/klog.FatalDepth",
    "k8s.io/klog.Fatalf",
    "k8s.io/klog.Fatalln",
    "k8s.io/klog/v2.Exit",
    "k8s.io/klog/v2.ExitDepth",
    "k8s.io/klog/v2.Exitf",
    "k8s.io/klog/v2.Exitln",
    "k8s.io/klog/v2.Fatal",
    "k8s.io/klog/v2.FatalDepth",
    "k8s.io/klog/v2.Fatalf",
    "k8s.io/klog/v2.Fatalln",
    "runtime.Goexit",
    "runtime.exit",
    "runtime.fatalpanic",
    "runtime.fatalthrow",
    "syscall.Exit",
    "syscall.ExitProcess",
    "syscall.ExitThread",
];

/// Standard-library functions upstream proves no-return by analysing `os`,
/// `log` and `testing`; guff does not have those packages' facts. Sorted.
const STDLIB_NO_RETURN: &[&str] = &[
    "(*log.Logger).Fatal",
    "(*log.Logger).Fatalf",
    "(*log.Logger).Fatalln",
    "(*log.Logger).Panic",
    "(*log.Logger).Panicf",
    "(*log.Logger).Panicln",
    // `(*testing.T).Fatal` and friends are promoted from the embedded
    // `common`, which is what both tools resolve the selector to.
    "(*testing.common).FailNow",
    "(*testing.common).Fatal",
    "(*testing.common).Fatalf",
    "(*testing.common).Skip",
    "(*testing.common).SkipNow",
    "(*testing.common).Skipf",
    "log.Fatal",
    "log.Fatalf",
    "log.Fatalln",
    "log.Panic",
    "log.Panicf",
    "log.Panicln",
    "os.Exit",
];

/// Compiler intrinsics that *do* return, contrary to their bodies
/// (`ctrlflow.knownIntrinsic`'s second half). Sorted.
const ALWAYS_RETURNS: &[&str] = &["hash/maphash.Comparable", "internal/abi.EscapeNonString"];

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;

    use super::*;

    /// A sink with no type information: `die(…)` never returns, everything
    /// else does. Enough to exercise the statement walk, which is where
    /// `go/cfg` parity lives.
    struct NameSink;

    impl FlowSink for NameSink {
        fn may_return(&mut self, call: &CallExpr) -> bool {
            !matches!(unparen(&call.fun), Expr::Ident(id) if id.name == "die" || id.name == "panic")
        }
    }

    fn no_return_body(body: &str) -> bool {
        let fset = FileSet::new();
        let src = format!("package p\n\nfunc f() {{\n{body}\n}}\n");
        let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse");
        let guff::ast::Decl::FuncDecl(fd) = &file.decls[0] else {
            panic!("expected a func decl");
        };
        let body = fd.body.as_ref().expect("body");
        if contains_goto(&body.list) {
            return false;
        }
        let flow = walk_list(&mut NameSink, &body.list, "");
        !(flow.returns || flow.falls)
    }

    #[test]
    fn falls_off_the_end_can_return() {
        assert!(!no_return_body("\tprintln(1)"));
        assert!(!no_return_body(""));
    }

    #[test]
    fn a_terminating_call_ends_the_body() {
        assert!(no_return_body("\tdie()"));
        assert!(no_return_body("\tprintln(1)\n\tdie()"));
        // Statements after it are unreachable, `return` included.
        assert!(no_return_body("\tdie()\n\treturn"));
    }

    #[test]
    fn a_reachable_return_wins() {
        assert!(!no_return_body("\tif x {\n\t\treturn\n\t}\n\tdie()"));
        assert!(!no_return_body("\treturn"));
    }

    #[test]
    fn both_arms_must_end() {
        assert!(no_return_body("\tif x {\n\t\tdie()\n\t} else {\n\t\tpanic(1)\n\t}"));
        assert!(!no_return_body("\tif x {\n\t\tdie()\n\t}\n\tprintln(1)"));
    }

    #[test]
    fn defer_counts_as_a_return() {
        // go/cfg marks the block `returns` because a deferred `recover` can
        // turn a panic into one.
        assert!(!no_return_body("\tdefer cleanup()\n\tdie()"));
    }

    #[test]
    fn an_endless_loop_never_falls_out() {
        assert!(no_return_body("\tfor {\n\t\tprintln(1)\n\t}"));
        assert!(!no_return_body("\tfor {\n\t\tbreak\n\t}"));
        assert!(!no_return_body("\tfor x {\n\t\tprintln(1)\n\t}"));
        // A `range` may run zero times.
        assert!(!no_return_body("\tfor range xs {\n\t\tdie()\n\t}"));
    }

    #[test]
    fn a_switch_needs_a_default() {
        assert!(no_return_body(
            "\tswitch x {\n\tcase 1:\n\t\tdie()\n\tdefault:\n\t\tpanic(1)\n\t}"
        ));
        // Without a default, "no case matched" reaches the statement after it.
        assert!(!no_return_body("\tswitch x {\n\tcase 1:\n\t\tdie()\n\t}"));
        // A `break` leaves the switch.
        assert!(!no_return_body(
            "\tswitch x {\n\tcase 1:\n\t\tbreak\n\tdefault:\n\t\tdie()\n\t}"
        ));
    }

    #[test]
    fn a_select_blocks_until_a_case_runs() {
        assert!(no_return_body("\tselect {\n\tcase <-ch:\n\t\tdie()\n\t}"));
        assert!(!no_return_body("\tselect {\n\tcase <-ch:\n\t\tprintln(1)\n\t}"));
    }

    #[test]
    fn dead_ranges_cover_the_tail() {
        struct Marker(Vec<(Pos, Pos)>);
        impl FlowSink for Marker {
            fn may_return(&mut self, call: &CallExpr) -> bool {
                !matches!(unparen(&call.fun), Expr::Ident(id) if id.name == "die")
            }
            fn dead(&mut self, from: Pos, to: Pos) {
                self.0.push((from, to));
            }
        }
        let fset = FileSet::new();
        let src = b"package p\n\nfunc f(x bool) {\n\tdie()\n\tif x {\n\t\tprintln(1)\n\t}\n}\n";
        let file = parse_file(&fset, "p.go", src, Mode::NONE).expect("parse");
        let guff::ast::Decl::FuncDecl(fd) = &file.decls[0] else {
            panic!("expected a func decl");
        };
        let mut marker = Marker(Vec::new());
        walk_list(&mut marker, &fd.body.as_ref().unwrap().list, "");
        assert_eq!(marker.0.len(), 1, "{:?}", marker.0);
        let (from, to) = marker.0[0];
        // The `if` statement, whole: its `x` is inside, so no use of `x` counts.
        let if_off = src.windows(4).position(|w| w == b"if x").expect("if") as i64;
        assert!(from.0 <= if_off + 3 && to.0 >= if_off, "{from:?}..{to:?}");
    }

    #[test]
    fn a_goto_body_is_not_classified() {
        // `go/cfg` follows the edge and finds the `return` behind `end:`;
        // the walk here cannot, so it declines to answer.
        assert!(!no_return_body("\tgoto end\n\tdie()\nend:\n\treturn"));
        // Even when the goto changes nothing.
        assert!(!no_return_body("\tgoto end\nend:\n\tdie()"));
    }

    #[test]
    fn ctrlflow_validates() {
        assert!(crate::validate::validate(&[analyzer()]).is_ok());
    }
}
