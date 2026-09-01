//! Port of [`github.com/kkHAIKE/contextcheck`](https://github.com/kkHAIKE/contextcheck).
//!
//! Checks that functions taking `context.Context` (or HTTP handlers) inherit
//! context from their parameters rather than calling `context.Background` /
//! `context.TODO`, and that callees without a context parameter receive one
//! when they internally use a non-inherited context.
//!
//! Uses `buildir` SSA. Package facts are exported for intra- and same-module
//! cross-package analysis (import packages are typechecked when this analyzer
//! is enabled). External modules without source in the fact closure are skipped.
//!
//! `//nolint:…contextcheck` and `// @contextcheck(req_has_ctx)` **doc**
//! comments are read by `doc_flag`, which is upstream's own directive handling
//! and separate from golangci-lint's `//nolint` processor: a skipped function
//! records no fact at all, so its *callers* fall silent too.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::FileSet;
use guff_analysis::callcheck::{resolve_call_target, static_callee};
use guff_analysis::code;
use guff_analysis::passes::buildir::BuildIrResult;
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, Fact, FactTypeId, Pass, RunError, RunFn};
use guff_ssa::function::Function;
use guff_ssa::ids::{FuncId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData, MakeClosure, Phi, Store, UnOp};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::predicates::identical;
use guff_types::signature::{signature_params, signature_recv, signature_results};
use guff_types::tuple::{tuple_at, tuple_len};
use guff_types::TypeId;
use guff_types::PackageId as TypePackageId;
use serde::{Deserialize, Serialize};

const CTX_PKG: &str = "context";
const CTX_NAME: &str = "Context";
const HTTP_PKG: &str = "net/http";
const HTTP_RES: &str = "ResponseWriter";
const HTTP_REQ: &str = "Request";

const MSG_NON_INHERITED: &str =
    "Non-inherited new context, use function like `context.WithXXX` instead";
const MSG_NON_INHERITED_HTTP: &str =
    "Non-inherited new context, use function like `context.WithXXX` or `r.Context` instead";

const CTX_IN: i32 = 1;
const CTX_OUT: i32 = 2;
const CTX_IN_FIELD: i32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryType {
    None,
    Normal,
    WithCtx,
    WithHttpHandler,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct ResInfo {
    valid: bool,
    funcs: Vec<String>,
    #[serde(default)]
    entry_type: i32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
struct CtxFact {
    entries: HashMap<String, ResInfo>,
}

impl Fact for CtxFact {
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
        "CtxFact"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

fn decode_ctx_fact(payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    serde_json::from_value::<CtxFact>(payload)
        .ok()
        .map(|f| Box::new(f) as Box<dyn Fact>)
}

fn instr_call_common(func: &Function, iid: InstrId) -> Option<&CallCommon> {
    match func.instrs.get(iid) {
        InstrData::Call(c) => Some(&c.call),
        InstrData::Defer(d) => Some(&d.call),
        InstrData::Go(g) => Some(&g.call),
        _ => None,
    }
}

fn ensure_ctx_fact_decoder() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        guff_analysis::register_fact_decoder("CtxFact", decode_ctx_fact);
    });
}

fn value_key(v: Value) -> Option<u64> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    Some(h.finish())
}

fn types_identical(prog: &Program, x: TypeId, y: TypeId) -> bool {
    let mut types = prog.type_arena.clone();
    identical(
        &mut types,
        &prog.object_arena,
        &prog.package_arena,
        x,
        y,
    )
}

/// The two upstream doc-comment directives, keyed by the declared function's
/// name position — the same key `getDocFromFunc` matches on (`fd.Name.Pos() ==
/// f.Pos()`), so only top-level `FuncDecl`s can carry one.
#[derive(Clone, Copy, Debug, Default)]
struct DocFlags {
    /// `// @contextcheck(req_has_ctx)`: treat any function taking an
    /// `*http.Request` as an HTTP handler.
    req_ctx: bool,
    /// `//nolint:` … `contextcheck`: skip the function entirely.
    skip: bool,
}

struct Runner<'a> {
    pass: &'a Pass<'a>,
    prog: &'a Program,
    ctx_typ: TypeId,
    http_res_typs: Vec<TypeId>,
    http_req_typs: Vec<TypeId>,
    current_fact: CtxFact,
    doc_flags: HashMap<i64, DocFlags>,
    pending: &'a mut Vec<(u32, String)>,
}

impl<'a> Runner<'a> {
    fn report(&mut self, pos: u32, msg: impl Into<String>) {
        if pos == 0 {
            return;
        }
        self.pending.push((pos, msg.into()));
    }

    /// Upstream `Reportf`: an instruction with no position of its own reports
    /// at its **parent function**'s position, and only a parent without one
    /// drops the diagnostic.
    ///
    /// A lifted `ctx = context.Background()` inside an `if` becomes a `Phi`,
    /// and go/ssa gives a `Phi` no position — so the whole class of
    /// conditionally-replaced contexts reports at the enclosing `func` token,
    /// not at the assignment. Without the fallback guff dropped every one of
    /// them.
    fn report_instr(&mut self, func: &Function, iid: InstrId, msg: impl Into<String>) {
        let mut pos = func.pos(iid);
        if !pos.is_valid() {
            pos = self.func_pos_of(func);
        }
        if pos.is_valid() {
            self.report(pos.0 as u32, msg);
        }
    }

    /// The position `ssa.Function.Pos()` answers: the `func` token of a
    /// literal, the declared identifier of a named function.
    fn func_pos_of(&self, f: &Function) -> guff::Pos {
        if f.decl_pos.is_valid() {
            return f.decl_pos;
        }
        f.object
            .map(|obj| guff::Pos(obj.pos(&self.prog.object_arena) as i64))
            .unwrap_or(guff::NO_POS)
    }

    /// The doc-comment directives upstream reads off a declared function.
    fn doc_flag(&self, f: &Function) -> DocFlags {
        let pos = self.func_pos_of(f);
        if !pos.is_valid() {
            return DocFlags::default();
        }
        self.doc_flags.get(&pos.0).copied().unwrap_or_default()
    }

    fn is_ctx_type(&self, typ: TypeId) -> bool {
        let typ = unalias_readonly(&self.prog.type_arena, typ);
        if types_identical(self.prog, typ, self.ctx_typ) {
            return true;
        }
        if let TypeData::Pointer(p) = self.prog.type_arena.get(typ) {
            return types_identical(self.prog, p.elem(), self.ctx_typ);
        }
        false
    }

    fn is_http_res_type(&self, typ: TypeId) -> bool {
        let typ = unalias_readonly(&self.prog.type_arena, typ);
        self.http_res_typs
            .iter()
            .any(|&t| types_identical(self.prog, typ, t))
    }

    fn is_http_req_type(&self, typ: TypeId) -> bool {
        let typ = unalias_readonly(&self.prog.type_arena, typ);
        if self
            .http_req_typs
            .iter()
            .any(|&t| types_identical(self.prog, typ, t))
        {
            return true;
        }
        if let TypeData::Pointer(p) = self.prog.type_arena.get(typ) {
            return self
                .http_req_typs
                .iter()
                .any(|&t| types_identical(self.prog, p.elem(), t));
        }
        false
    }

    /// Port of `ssa.Function.RelString`.
    ///
    /// A bound-method closure (`c.Complete` handed to a library) and a method
    /// expression thunk are *separate* functions from the method they delegate
    /// to, and go/ssa keeps their `$bound` / `$thunk` suffix in the name
    /// `RelString` prints. contextcheck depends on that: the suffixed key never
    /// has a fact, so a method value never inherits its method's verdict, and
    /// `checkFuncWithoutCtx` skips the two suffixes by name. Building the key
    /// from the *object* alone dropped the suffix, so `newPrompt(c.Complete)`
    /// read as a direct call to `(*Completer).Complete` and reported its whole
    /// chain (scaleway-cli `core/shell.go:230`).
    fn func_rel_string(&self, f: &Function) -> String {
        if let Some(obj) = f.object {
            let base = code::type_func_name(
                &self.prog.type_arena,
                &self.prog.object_arena,
                &self.prog.package_arena,
                obj,
            );
            for suffix in ["$bound", "$thunk"] {
                if f.name.ends_with(suffix) {
                    return format!("{base}{suffix}");
                }
            }
            base
        } else {
            f.name.clone()
        }
    }

    /// The type-checker package a function belongs to, which is the key its
    /// `CtxFact` is stored under.
    ///
    /// A function the SSA builder created on demand for an *imported* package
    /// has no `Function.pkg` — only the package it was declared from was ever
    /// built as an SSA package. Its declaring object still names one, and
    /// without that fallback every cross-package callee looked like a function
    /// with no fact: jaeger's `NewFactoryBase$1 -> (*FactoryBase).Close ->
    /// (*esclient.BulkIndexer).Close`, where the last hop is the one that
    /// passes `context.Background()`.
    fn func_type_pkg(&self, f: &Function) -> Option<TypePackageId> {
        if let Some(pid) = f.pkg {
            return Some(self.prog.packages.get(pid).type_pkg());
        }
        f.object.and_then(|obj| obj.pkg(&self.prog.object_arena))
    }

    fn get_value(&self, key: &str, f: &Function) -> Option<ResInfo> {
        if let Some(res) = self.current_fact.entries.get(key) {
            return Some(res.clone());
        }
        if key.starts_with("entry:") {
            return None;
        }
        let pkg = self.func_type_pkg(f)?;
        let mut fact = CtxFact::default();
        if self.pass.import_package_fact(pkg, &mut fact) {
            return fact.entries.get(key).cloned();
        }
        None
    }

    fn set_fact(&mut self, key: &str, valid: bool, funcs: &[String]) {
        let mut names = self
            .current_fact
            .entries
            .get(key)
            .map(|r| r.funcs.clone())
            .unwrap_or_default();
        if !valid {
            names.extend(funcs.iter().cloned());
        }
        self.current_fact.entries.insert(
            key.to_string(),
            ResInfo {
                valid,
                funcs: names,
                ..Default::default()
            },
        );
    }

    fn check_is_ctx_sig(&self, f: &Function) -> (bool, bool) {
        let Some(sig) = f.signature else {
            return (false, false);
        };
        let mut ctx_in = false;
        let mut ctx_out = false;

        if let Some(params) = signature_params(&self.prog.type_arena, sig) {
            for i in 0..tuple_len(&self.prog.type_arena, Some(params)) {
                let elem = tuple_at(&self.prog.type_arena, params, i);
                if let Some(t) = elem.typ(&self.prog.object_arena) {
                    if self.is_ctx_type(t) {
                        ctx_in = true;
                        break;
                    }
                }
            }
        }

        for (_, fv) in f.freevars.iter() {
            if self.is_ctx_type(fv.typ) {
                ctx_in = true;
                break;
            }
        }

        if let Some(results) = signature_results(&self.prog.type_arena, sig) {
            for i in 0..tuple_len(&self.prog.type_arena, Some(results)) {
                let elem = tuple_at(&self.prog.type_arena, results, i);
                if let Some(t) = elem.typ(&self.prog.object_arena) {
                    if self.is_ctx_type(t) {
                        ctx_out = true;
                        break;
                    }
                }
            }
        }

        (ctx_in, ctx_out)
    }

    fn check_is_http_handler(&self, f: &Function, req_ctx: bool) -> bool {
        let Some(sig) = f.signature else {
            return false;
        };
        let Some(params) = signature_params(&self.prog.type_arena, sig) else {
            return false;
        };
        let n = tuple_len(&self.prog.type_arena, Some(params));
        let mut has_req = false;
        for i in 0..n {
            let elem = tuple_at(&self.prog.type_arena, params, i);
            if let Some(t) = elem.typ(&self.prog.object_arena) {
                if self.is_http_req_type(t) {
                    has_req = true;
                    break;
                }
            }
        }
        if !has_req {
            return false;
        }
        // `// @contextcheck(req_has_ctx)` promotes *any* function taking a
        // request to a handler, without the two-parameter shape below.
        if req_ctx {
            return true;
        }
        let results = signature_results(&self.prog.type_arena, sig);
        let res_len = tuple_len(&self.prog.type_arena, results);
        if res_len == 0 && n == 2 {
            let t0 = tuple_at(&self.prog.type_arena, params, 0)
                .typ(&self.prog.object_arena)
                .unwrap_or(self.ctx_typ);
            let t1 = tuple_at(&self.prog.type_arena, params, 1)
                .typ(&self.prog.object_arena)
                .unwrap_or(self.ctx_typ);
            if self.is_http_res_type(t0) && self.is_http_req_type(t1) {
                return true;
            }
        }
        !self.get_http_req_ctx(f, true).is_empty()
    }

    fn check_is_entry(&mut self, f: &Function) -> EntryType {
        let key = format!("entry:{}", self.func_rel_string(f));
        if let Some(res) = self.current_fact.entries.get(&key) {
            return match res.entry_type {
                1 => EntryType::WithCtx,
                2 => EntryType::WithHttpHandler,
                3 => EntryType::Normal,
                _ => EntryType::None,
            };
        }

        let (ctx_in, ctx_out) = self.check_is_ctx_sig(f);
        // Upstream reads the doc directives *after* the signature decides, so
        // neither `skip` nor `req_has_ctx` can demote a function that already
        // takes or returns a context.
        let ret = if ctx_out {
            EntryType::None
        } else if ctx_in {
            EntryType::WithCtx
        } else {
            let flags = self.doc_flag(f);
            if self.check_is_http_handler(f, flags.req_ctx) {
                EntryType::WithHttpHandler
            } else if flags.skip {
                EntryType::None
            } else {
                EntryType::Normal
            }
        };

        let entry_code = match ret {
            EntryType::None => 0,
            EntryType::WithCtx => 1,
            EntryType::WithHttpHandler => 2,
            EntryType::Normal => 3,
        };
        self.current_fact.entries.insert(
            key,
            ResInfo {
                entry_type: entry_code,
                ..Default::default()
            },
        );
        ret
    }

    fn get_call_ctx_type(&self, func: &Function, common: &CallCommon, ret_typ: TypeId) -> i32 {
        let mut tp = 0;
        for &arg in &common.args {
            let arg_typ = value_type_of(self.prog, func, arg);
            if self.is_ctx_type(arg_typ) {
                if let Value::Instr(uid) = arg {
                    if let InstrData::UnOp(UnOp {
                        x: Value::Instr(faid),
                        ..
                    }) = func.instrs.get(uid)
                    {
                        if matches!(func.instrs.get(*faid), InstrData::FieldAddr(_)) {
                            tp |= CTX_IN_FIELD;
                        }
                    }
                }
                tp |= CTX_IN;
                break;
            }
        }

        if self.is_ctx_type(ret_typ) {
            tp |= CTX_OUT;
        } else if matches!(self.prog.type_arena.get(ret_typ), TypeData::Tuple(_)) {
            for i in 0..tuple_len(&self.prog.type_arena, Some(ret_typ)) {
                let elem = tuple_at(&self.prog.type_arena, ret_typ, i);
                if let Some(t) = elem.typ(&self.prog.object_arena) {
                    if self.is_ctx_type(t) {
                        tp |= CTX_OUT;
                        break;
                    }
                }
            }
        }
        tp
    }

    fn get_call_ctx_type_instr(&self, func: &Function, iid: InstrId, common: &CallCommon) -> i32 {
        let ret_typ = match func.instrs.get(iid) {
            InstrData::Call(c) => c.typ,
            _ => return 0,
        };
        self.get_call_ctx_type(func, common, ret_typ)
    }

    fn get_make_closure_ctx_type(&self, func: &Function, mc: &MakeClosure) -> i32 {
        let mut tp = 0;
        for &v in &mc.bindings {
            if self.is_ctx_type(value_type_of(self.prog, func, v)) {
                tp |= CTX_IN;
                break;
            }
        }
        tp
    }

    fn callee_name(&self, func: &Function, iid: InstrId) -> Option<String> {
        let common = instr_call_common(func, iid)?;
        let obj = resolve_call_target(common, self.prog)?;
        Some(code::type_func_name(
            &self.prog.type_arena,
            &self.prog.object_arena,
            &self.prog.package_arena,
            obj,
        ))
    }

    /// Is this call a `Context()` **method** call (i.e. `r.Context()`)?
    ///
    /// Mirrors upstream's `f.Name() == ctxName && f.Signature.Recv() != nil`,
    /// covering both the static form (concrete receiver such as
    /// `*http.Request`) and the invoke form (interface method).
    fn is_recv_context_call(&self, common: &CallCommon) -> bool {
        if let Some(method) = common.method {
            return method.name(&self.prog.object_arena) == CTX_NAME;
        }
        // Resolve through the type-checker object rather than the SSA function:
        // `(*http.Request).Context` lives in an external package, so its body is
        // never built and `self.prog.functions` has no entry for it.
        let Some(obj) = resolve_call_target(common, self.prog) else {
            return false;
        };
        if obj.name(&self.prog.object_arena) != CTX_NAME {
            return false;
        }
        obj.typ(&self.prog.object_arena)
            .and_then(|sig| signature_recv(&self.prog.type_arena, sig))
            .is_some()
    }

    fn callee_func(&self, func: &Function, iid: InstrId) -> Option<FuncId> {
        // Match upstream `getFunction`: CallInstruction static callees and
        // MakeClosure's anonymous function (needed to chase closure bodies that
        // call `context.Background` / `TODO`).
        if let InstrData::MakeClosure(mc) = func.instrs.get(iid) {
            return Some(mc.fn_);
        }
        let common = instr_call_common(func, iid)?;
        if common.method.is_some() {
            return None;
        }
        if let Some(fid) = static_callee(common) {
            return Some(fid);
        }
        let obj = resolve_call_target(common, self.prog)?;
        self.prog
            .functions
            .iter()
            .find_map(|(fid, f)| (f.object == Some(obj)).then_some(fid))
    }

    fn get_http_req_ctx(&self, f: &Function, least1: bool) -> Vec<Value> {
        let Some(referrers) = f.referrers.as_ref() else {
            return Vec::new();
        };
        let mut rets = Vec::new();
        let mut checked = HashSet::new();

        for (pid, param) in f.params.iter() {
            if !self.is_http_req_type(param.typ) {
                continue;
            }
            let param_val = Value::Param(pid);
            let Some(refs) = referrers.get(&param_val) else {
                continue;
            };
            for &iid in refs {
                self.collect_req_context_calls(f, iid, &mut rets, &mut checked);
                if least1 && !rets.is_empty() {
                    return rets;
                }
            }
        }
        rets
    }

    fn collect_req_context_calls(
        &self,
        f: &Function,
        iid: InstrId,
        rets: &mut Vec<Value>,
        checked: &mut HashSet<InstrId>,
    ) {
        if !checked.insert(iid) {
            return;
        }
        let Some(referrers) = f.referrers.as_ref() else {
            return;
        };

        match f.instrs.get(iid) {
            InstrData::Call(_) | InstrData::Defer(_) | InstrData::Go(_) => {
                let Some(common) = instr_call_common(f, iid) else {
                    return;
                };
                if common.args.len() != 1 {
                    return;
                }
                let tp = self.get_call_ctx_type_instr(f, iid, common);
                if tp & CTX_OUT == 0 {
                    return;
                }
                // Upstream `getHttpReqCtx` resolves the callee *function* and
                // accepts it when `f.Name() == "Context"` and the signature has
                // a receiver. `(*http.Request).Context` is a concrete method, so
                // it is a static call with `Method == nil` — matching only on
                // `common.method` missed every plain `ctx := r.Context()` and
                // flagged the canonical http.HandlerFunc body.
                if self.is_recv_context_call(common) {
                    rets.push(Value::Instr(iid));
                }
            }
            InstrData::UnOp(_) | InstrData::Phi(_) | InstrData::Extract(_) => {
                if let Some(refs) = referrers.get(&Value::Instr(iid)) {
                    for &next in refs {
                        self.collect_req_context_calls(f, next, rets, checked);
                    }
                }
            }
            InstrData::Store(s) => {
                if let Some(refs) = referrers.get(&s.addr) {
                    for &next in refs {
                        self.collect_req_context_calls(f, next, rets, checked);
                    }
                }
            }
            _ => {}
        }
    }

    fn walk_ctx_refs(
        &self,
        f: &Function,
        val: Value,
        from_addr: bool,
        checked_vals: &mut HashSet<u64>,
        ref_map: &mut HashMap<InstrId, bool>,
        store_instrs: &mut HashSet<InstrId>,
        phi_instrs: &mut HashSet<InstrId>,
    ) {
        let Some(key) = value_key(val) else {
            return;
        };
        if !checked_vals.insert(key) {
            return;
        }
        let Some(referrers) = f.referrers.as_ref() else {
            return;
        };
        let Some(refs) = referrers.get(&val) else {
            return;
        };

        for &iid in refs {
            match f.instrs.get(iid) {
                InstrData::Call(_) | InstrData::Defer(_) | InstrData::Go(_) => {
                    ref_map.insert(iid, true);
                    let Some(common) = instr_call_common(f, iid) else {
                        continue;
                    };
                    let tp = self.get_call_ctx_type_instr(f, iid, common);
                    if tp & CTX_OUT != 0 {
                        self.walk_ctx_refs(
                            f,
                            Value::Instr(iid),
                            false,
                            checked_vals,
                            ref_map,
                            store_instrs,
                            phi_instrs,
                        );
                    }
                }
                InstrData::Store(s) => {
                    if from_addr {
                        store_instrs.insert(iid);
                    } else {
                        self.walk_ctx_refs(
                            f,
                            s.addr,
                            true,
                            checked_vals,
                            ref_map,
                            store_instrs,
                            phi_instrs,
                        );
                    }
                }
                InstrData::UnOp(_) => {
                    self.walk_ctx_refs(
                        f,
                        Value::Instr(iid),
                        false,
                        checked_vals,
                        ref_map,
                        store_instrs,
                        phi_instrs,
                    );
                }
                InstrData::MakeClosure(mc) => {
                    for &b in &mc.bindings {
                        if self.is_ctx_type(value_type_of(self.prog, f, b)) {
                            ref_map.insert(iid, true);
                            break;
                        }
                    }
                }
                InstrData::Extract(ex) => {
                    if self.is_ctx_type(ex.typ) {
                        self.walk_ctx_refs(
                            f,
                            Value::Instr(iid),
                            false,
                            checked_vals,
                            ref_map,
                            store_instrs,
                            phi_instrs,
                        );
                    }
                }
                InstrData::Phi(_) => {
                    phi_instrs.insert(iid);
                    self.walk_ctx_refs(
                        f,
                        Value::Instr(iid),
                        false,
                        checked_vals,
                        ref_map,
                        store_instrs,
                        phi_instrs,
                    );
                }
                _ => {}
            }
        }
    }

    /// Returns the call instructions that consume the inherited context, and
    /// whether the function is *clean*.
    ///
    /// Upstream returns `ok = false` as soon as it reports a non-inherited
    /// context, and `checkFuncWithCtx` then stops — a function that replaces
    /// its own context is not asked anything further. Dropping that flag made
    /// guff carry on and report the callee chains upstream never looks at.
    fn collect_ctx_ref(
        &mut self,
        f: &Function,
        is_http_handler: bool,
    ) -> (HashMap<InstrId, bool>, bool) {
        let mut ok = true;
        let mut ref_map = HashMap::new();
        let mut checked_vals = HashSet::new();
        let mut store_instrs = HashSet::new();
        let mut phi_instrs = HashSet::new();

        if is_http_handler {
            for v in self.get_http_req_ctx(f, false) {
                self.walk_ctx_refs(
                    f,
                    v,
                    false,
                    &mut checked_vals,
                    &mut ref_map,
                    &mut store_instrs,
                    &mut phi_instrs,
                );
            }
        } else {
            for (pid, param) in f.params.iter() {
                if self.is_ctx_type(param.typ) {
                    self.walk_ctx_refs(
                        f,
                        Value::Param(pid),
                        false,
                        &mut checked_vals,
                        &mut ref_map,
                        &mut store_instrs,
                        &mut phi_instrs,
                    );
                }
            }
            for (fvid, fv) in f.freevars.iter() {
                if self.is_ctx_type(fv.typ) {
                    self.walk_ctx_refs(
                        f,
                        Value::FreeVar(fvid),
                        false,
                        &mut checked_vals,
                        &mut ref_map,
                        &mut store_instrs,
                        &mut phi_instrs,
                    );
                }
            }
        }

        for iid in store_instrs {
            let InstrData::Store(Store { val, .. }) = f.instrs.get(iid) else {
                continue;
            };
            if let Some(k) = value_key(*val) {
                if !checked_vals.contains(&k) {
                    self.report_instr(f, iid, MSG_NON_INHERITED);
                    ok = false;
                }
            }
        }
        for iid in phi_instrs {
            let InstrData::Phi(Phi { edges, .. }) = f.instrs.get(iid) else {
                continue;
            };
            for edge in edges.iter().flatten() {
                if let Some(k) = value_key(*edge) {
                    if !checked_vals.contains(&k) {
                        self.report_instr(f, iid, MSG_NON_INHERITED);
                        ok = false;
                        break;
                    }
                }
            }
        }

        (ref_map, ok)
    }

    fn instr_ctx_flags(&self, func: &Function, iid: InstrId) -> i32 {
        match func.instrs.get(iid) {
            InstrData::Call(_) | InstrData::Defer(_) | InstrData::Go(_) => {
                let Some(common) = instr_call_common(func, iid) else {
                    return 0;
                };
                self.get_call_ctx_type_instr(func, iid, common)
            }
            InstrData::MakeClosure(mc) => self.get_make_closure_ctx_type(func, mc),
            _ => 0,
        }
    }

    fn check_func_with_ctx(&mut self, f: &Function, tp: EntryType) {
        let is_http = tp == EntryType::WithHttpHandler;
        let (ref_map, ok) = self.collect_ctx_ref(f, is_http);
        if !ok {
            return;
        }

        for (_, block) in f.live_blocks() {
            for &iid in &block.instrs {
                let tp_flags = self.instr_ctx_flags(f, iid);
                let is_call_like = instr_call_common(f, iid).is_some()
                    || matches!(f.instrs.get(iid), InstrData::MakeClosure(_));
                if tp_flags == 0 && !is_call_like {
                    continue;
                }

                if tp_flags & CTX_OUT == 0
                    && tp_flags & CTX_IN != 0
                    && !ref_map.contains_key(&iid)
                {
                    let msg = if is_http {
                        MSG_NON_INHERITED_HTTP
                    } else {
                        MSG_NON_INHERITED
                    };
                    self.report_instr(f, iid, msg);
                }

                let key = if let Some(callee) = self.callee_func(f, iid) {
                    self.func_rel_string(self.prog.functions.get(callee))
                } else if let Some(name) = self.callee_name(f, iid) {
                    name
                } else {
                    continue;
                };
                let lookup_fn = self
                    .callee_func(f, iid)
                    .map(|fid| self.prog.functions.get(fid));
                if let Some(res) = self.get_value(&key, lookup_fn.unwrap_or(f)) {
                    if !res.valid {
                        let chain: Vec<String> = res.funcs.iter().rev().cloned().collect();
                        let chain_str = chain.join("->");
                        let msg =
                            format!("Function `{chain_str}` should pass the context parameter");
                        // go/ssa emits `MakeClosure` with no position, so a
                        // capturing func literal has nowhere to report from.
                        // Upstream falls back to the callee itself, whose
                        // position is the literal's `func` token.
                        if f.pos(iid).is_valid() {
                            self.report_instr(f, iid, msg);
                        } else if let Some(callee) = self.callee_func(f, iid) {
                            let callee_fn = self.prog.functions.get(callee);
                            let mut pos = self.func_pos_of(callee_fn);
                            if !pos.is_valid() {
                                pos = callee_fn
                                    .parent
                                    .map(|p| self.func_pos_of(self.prog.functions.get(p)))
                                    .unwrap_or(guff::NO_POS);
                            }
                            if pos.is_valid() {
                                self.report(pos.0 as u32, msg);
                            }
                        }
                    }
                }
            }
        }
    }

    fn check_func_without_ctx(&mut self, f: &Function, checking: &mut HashMap<String, bool>) -> bool {
        let mut ret = true;
        let org_key = self.func_rel_string(f);
        let mut saved = false;

        for (_, block) in f.live_blocks() {
            for &iid in &block.instrs {
                // Upstream `getCtxType` is ok for CallInstruction / MakeClosure
                // and nothing else. A `return fn` that hands back a bare
                // `Function` — what go/ssa emits for a func literal with no
                // free variables — is deliberately not followed: upstream
                // reaches a returned literal through the `MakeClosure` its
                // *capturing* form emits, and reports nothing when there is
                // none.
                let callees: Vec<FuncId> = self.callee_func(f, iid).into_iter().collect();
                let is_call_like = instr_call_common(f, iid).is_some()
                    || matches!(f.instrs.get(iid), InstrData::MakeClosure(_));
                if !is_call_like {
                    continue;
                }

                let tp_flags = self.instr_ctx_flags(f, iid);
                if tp_flags & CTX_OUT != 0 {
                    continue;
                }

                if tp_flags & CTX_IN != 0 && tp_flags & CTX_IN_FIELD == 0 {
                    ret = false;
                }

                for callee in callees {
                    let callee_fn = self.prog.functions.get(callee);
                    let key = self.func_rel_string(callee_fn);

                    if let Some(res) = self.get_value(&key, callee_fn) {
                        if !res.valid {
                            ret = false;
                            if !saved {
                                saved = true;
                                self.set_fact(&org_key, false, &res.funcs);
                            }
                        }
                        continue;
                    }

                    if key.ends_with("$thunk") || key.ends_with("$bound") {
                        continue;
                    }

                    if self.check_is_entry(callee_fn) == EntryType::Normal {
                        if callee_fn.blocks.is_empty() {
                            continue;
                        }
                        if checking.get(&key).copied().unwrap_or(false) {
                            continue;
                        }
                        checking.insert(key.clone(), true);
                        let valid = self.check_func_without_ctx(callee_fn, checking);
                        self.set_fact(&key, valid, &[callee_fn.name.clone()]);
                        if !valid && !saved {
                            if let Some(res) = self.get_value(&key, callee_fn) {
                                saved = true;
                                self.set_fact(&org_key, false, &res.funcs);
                            }
                        }
                        if !valid {
                            ret = false;
                        }
                    }
                }
            }
        }
        ret
    }

    fn run(&mut self, src_funcs: &[FuncId]) {
        let mut entry_funcs = Vec::new();

        for &fid in src_funcs {
            let f = self.prog.functions.get(fid);
            let key = self.func_rel_string(f);
            if self.current_fact.entries.contains_key(&key) {
                continue;
            }
            let entry = self.check_is_entry(f);
            match entry {
                EntryType::WithCtx | EntryType::WithHttpHandler => {
                    entry_funcs.push((fid, entry));
                }
                EntryType::None => {}
                EntryType::Normal => {
                    if self.get_value(&key, f).is_some() {
                        continue;
                    }
                    let mut checking = HashMap::new();
                    checking.insert(key.clone(), true);
                    let valid = self.check_func_without_ctx(f, &mut checking);
                    self.set_fact(&key, valid, &[f.name.clone()]);
                }
            }
        }

        for (fid, tp) in entry_funcs {
            let f = self.prog.functions.get(fid);
            self.check_func_with_ctx(f, tp);
        }
    }
}

fn find_named_type(prog: &Program, pkg_path: &str, name: &str) -> Option<TypeId> {
    for oid in prog.object_arena.ids() {
        let ObjectData::TypeName(tn) = prog.object_arena.get(oid) else {
            continue;
        };
        if tn.name() != name {
            continue;
        }
        let Some(pkg) = oid.pkg(&prog.object_arena) else {
            continue;
        };
        if prog.package_arena.get(pkg).path() != pkg_path {
            continue;
        }
        return tn.typ();
    }
    None
}

fn collect_http_types(prog: &Program) -> (Vec<TypeId>, Vec<TypeId>) {
    let mut res = Vec::new();
    let mut req = Vec::new();
    if let Some(t) = find_named_type(prog, HTTP_PKG, HTTP_RES) {
        res.push(t);
    }
    if let Some(t) = find_named_type(prog, HTTP_PKG, HTTP_REQ) {
        req.push(t);
    }
    (res, req)
}

/// Upstream `nolintRe` is `^//\s?nolint:` — the slashes, **at most one**
/// whitespace character, then `nolint:`.
fn is_nolint_comment(text: &str) -> bool {
    let Some(rest) = text.strip_prefix("//") else {
        return false;
    };
    let rest = match rest.chars().next() {
        Some(c) if c.is_whitespace() => &rest[c.len_utf8()..],
        _ => rest,
    };
    rest.starts_with("nolint:")
}

/// Port of `docFlag`, keyed the way `getDocFromFunc` looks a function up: the
/// declared name's position. Only top-level `FuncDecl`s are scanned, so a func
/// literal can never carry a directive.
///
/// The analysis load parses with `Mode::NONE`, which drops every comment past
/// the file header — `fd.doc` is `None` for all but the first declaration — so
/// a file that can carry a directive is re-parsed with comments in a private
/// `FileSet` and its positions mapped back. Both directives contain the
/// substring `contextcheck`, which is the gate: a file without it is not
/// re-parsed at all.
fn collect_doc_flags(pass: &Pass<'_>) -> HashMap<i64, DocFlags> {
    let mut out: HashMap<i64, DocFlags> = HashMap::new();
    let pkg = pass.pkg();
    for (i, file) in pass.files().iter().enumerate() {
        let owned;
        let src: &[u8] = match pkg.source_bytes(i) {
            Some(b) => b,
            None => match pkg
                .compiled_go_files
                .get(i)
                .and_then(|path| std::fs::read(path).ok())
            {
                Some(b) => {
                    owned = b;
                    &owned
                }
                None => continue,
            },
        };
        if !std::str::from_utf8(src).is_ok_and(|t| t.contains("contextcheck")) {
            continue;
        }
        let name = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("file.go");
        let reparsed_fset = FileSet::new();
        let Ok(reparsed) = parse_file(&reparsed_fset, name, src, COMMENTS_ONLY) else {
            continue;
        };
        for decl in &reparsed.decls {
            let guff::ast::Decl::FuncDecl(fd) = decl else {
                continue;
            };
            let Some(doc) = fd.doc.as_ref() else {
                continue;
            };
            let mut flags = DocFlags::default();
            for c in &doc.list {
                if is_nolint_comment(&c.text) && c.text.contains("contextcheck") {
                    flags.skip = true;
                } else if c.text.starts_with("// @contextcheck(req_has_ctx)") {
                    flags.req_ctx = true;
                }
            }
            if !flags.skip && !flags.req_ctx {
                continue;
            }
            if let Some(pos) = map_reparsed_pos(pass, file, &reparsed_fset, fd.name.pos().0) {
                out.insert(pos as i64, flags);
            }
        }
    }
    out
}

/// Translate a position from the private reparse `FileSet` into the pass's.
/// Both parses cover the same bytes, so the byte offset is the bridge.
fn map_reparsed_pos(
    pass: &Pass<'_>,
    file: &guff::ast::File,
    reparsed_fset: &FileSet,
    pos: i64,
) -> Option<u32> {
    let from = reparsed_fset.file(guff::Pos(pos))?;
    let to = pass.fset().file(file.pos())?;
    let offset = from.offset(guff::Pos(pos));
    if offset < 0 || offset > to.size() {
        return None;
    }
    Some(to.pos(offset).0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    ensure_ctx_fact_decoder();

    let doc_flags = collect_doc_flags(pass);
    let mut pending = Vec::new();
    let mut exported_fact = None;

    {
        let ir = pass
            .result_of::<BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "contextcheck requires buildir analyzer".to_string())?;

        let Some(ctx_typ) = find_named_type(&ir.prog, CTX_PKG, CTX_NAME) else {
            return Ok(None);
        };
        let (http_res, http_req) = collect_http_types(&ir.prog);

        let type_pkg = pass
            .pkg()
            .types
            .ok_or_else(|| "contextcheck requires types".to_string())?;

        let mut runner = Runner {
            pass,
            prog: &ir.prog,
            ctx_typ,
            http_res_typs: http_res,
            http_req_typs: http_req,
            current_fact: CtxFact::default(),
            doc_flags,
            pending: &mut pending,
        };

        let src_funcs = ir.src_funcs.clone();
        runner.run(&src_funcs);

        if !runner.current_fact.entries.is_empty() {
            exported_fact = Some((type_pkg, runner.current_fact));
        }
    }

    for (pos, msg) in pending {
        pass.report(Diagnostic {
            pos,
            message: msg,
            ..Diagnostic::default()
        });
    }
    if let Some((pkg, fact)) = exported_fact {
        pass.export_package_fact(pkg, Box::new(fact));
    }

    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| {
        ensure_ctx_fact_decoder();
        Analyzer {
            name: "contextcheck",
            doc: "check whether the function uses a non-inherited context",
            url: "https://github.com/kkHAIKE/contextcheck",
            run: run as RunFn,
            run_despite_errors: true,
            requires: vec![buildir::analyzer()],
            fact_types: vec![FactTypeId::of::<CtxFact>()],
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nolint_doc_regexp_boundaries() {
        // Upstream's `^//\s?nolint:`: the slashes, at most one whitespace
        // character, then `nolint:`. `docFlag` additionally asks that the
        // comment mention `contextcheck`, which is checked at the call site.
        for yes in [
            "//nolint:contextcheck",
            "// nolint:contextcheck",
            "//\tnolint:contextcheck",
            "//nolint:gosec,contextcheck // why",
        ] {
            assert!(is_nolint_comment(yes), "{yes}");
        }
        for no in [
            "//  nolint:contextcheck",
            "// nolint : contextcheck",
            "//nolint",
            "/* nolint:contextcheck */",
            "// see nolint:contextcheck below",
        ] {
            assert!(!is_nolint_comment(no), "{no}");
        }
    }

    #[test]
    fn ctx_fact_roundtrip_json() {
        let mut fact = CtxFact::default();
        fact.entries.insert(
            "foo".into(),
            ResInfo {
                valid: false,
                funcs: vec!["bar".into()],
                ..Default::default()
            },
        );
        let payload = fact.encode_payload();
        let decoded = decode_ctx_fact(payload).unwrap();
        let down = decoded.as_any().downcast_ref::<CtxFact>().unwrap();
        assert_eq!(down.entries["foo"].funcs, vec!["bar"]);
    }
}
