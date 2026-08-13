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
//! DEFERRED: `//@contextcheck(req_has_ctx)` / nolint directives; full HTTP
//! handler `r.Context()` edge cases.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

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

/// Strips value-preserving `ChangeType` wrappers. Returning a func literal as a
/// *named* function type (`return func(ns string) error {…}` from a
/// `func() listFunc`) converts at the return, so the `Function` value sits one
/// instruction in. (Go: the ChangeType `emitConv` inserts for a return operand.)
fn unwrap_change_type(func: &Function, v: Value) -> Value {
    let mut cur = v;
    loop {
        let Value::Instr(iid) = cur else { return cur };
        match func.instrs.get(iid) {
            InstrData::ChangeType(ct) => cur = ct.x,
            _ => return cur,
        }
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

struct Runner<'a> {
    pass: &'a Pass<'a>,
    prog: &'a Program,
    ctx_typ: TypeId,
    http_res_typs: Vec<TypeId>,
    http_req_typs: Vec<TypeId>,
    current_fact: CtxFact,
    pending: &'a mut Vec<(u32, String)>,
}

impl<'a> Runner<'a> {
    fn report(&mut self, pos: u32, msg: impl Into<String>) {
        if pos == 0 {
            return;
        }
        self.pending.push((pos, msg.into()));
    }

    fn report_instr(&mut self, func: &Function, iid: InstrId, msg: impl Into<String>) {
        let pos = func.pos(iid);
        if pos.is_valid() {
            self.report(pos.0 as u32, msg);
        }
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

    fn func_rel_string(&self, f: &Function) -> String {
        if let Some(obj) = f.object {
            code::type_func_name(
                &self.prog.type_arena,
                &self.prog.object_arena,
                &self.prog.package_arena,
                obj,
            )
        } else {
            f.name.clone()
        }
    }

    fn func_type_pkg(&self, f: &Function) -> Option<TypePackageId> {
        f.pkg.map(|pid| self.prog.packages.get(pid).type_pkg())
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

    fn check_is_http_handler(&self, f: &Function) -> bool {
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
        let ret = if ctx_out {
            EntryType::None
        } else if ctx_in {
            EntryType::WithCtx
        } else if self.check_is_http_handler(f) {
            EntryType::WithHttpHandler
        } else {
            EntryType::Normal
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

    fn is_background_or_todo(&self, func: &Function, iid: InstrId) -> bool {
        self.callee_name(func, iid)
            .is_some_and(|n| n == "context.Background" || n == "context.TODO")
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

    /// Callees to chase from `iid`: upstream Call/MakeClosure targets, plus bare
    /// `Function` values returned by a `return` (go/ssa emits those for
    /// non-capturing func lits; guff may also do so when free-var capture fails
    /// under incomplete types — helm `RsListFromClient`).
    fn instr_callees(&self, func: &Function, iid: InstrId) -> Vec<FuncId> {
        if let Some(fid) = self.callee_func(func, iid) {
            return vec![fid];
        }
        let InstrData::Return(ret) = func.instrs.get(iid) else {
            return Vec::new();
        };
        ret.results
            .iter()
            .filter_map(|v| match unwrap_change_type(func, *v) {
                Value::Function(fid) => Some(fid),
                _ => None,
            })
            .collect()
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

    fn collect_ctx_ref(&mut self, f: &Function, is_http_handler: bool) -> HashMap<InstrId, bool> {
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
                        break;
                    }
                }
            }
        }

        ref_map
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
        let ref_map = self.collect_ctx_ref(f, is_http);

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
                        self.report_instr(
                            f,
                            iid,
                            format!("Function `{chain_str}` should pass the context parameter"),
                        );
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
                // Upstream `getCtxType` is ok for CallInstruction / MakeClosure.
                // Also walk `return fn` (bare Function) — go/ssa does this for
                // non-capturing func lits; guff may too when free-var capture
                // fails (helm `RsListFromClient` under incomplete client types).
                let callees = self.instr_callees(f, iid);
                let is_call_like = instr_call_common(f, iid).is_some()
                    || matches!(f.instrs.get(iid), InstrData::MakeClosure(_))
                    || !callees.is_empty();
                if !is_call_like {
                    continue;
                }

                if self.is_background_or_todo(f, iid) {
                    ret = false;
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

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    ensure_ctx_fact_decoder();

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
