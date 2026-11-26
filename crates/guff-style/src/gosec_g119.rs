//! Gosec **G119** — unsafe redirect policy may propagate sensitive headers (SSA).
//!
//! Port of securego/gosec v2.26.1
//! `analyzers/redirect_header_propagation.go` (+ `analyzers/dependency_checker.go`).
//!
//! `http.Client.CheckRedirect` is `func(req *Request, via []*Request) error`.
//! A policy that copies the previous request's headers onto the new one —
//! `req.Header = via[len(via)-1].Header.Clone()` — carries `Authorization`,
//! `Cookie` and friends across an origin change. gosec reports two things: any
//! store into a `http.Header` field reachable from the `req` parameter, and any
//! `(http.Header).Set`/`Add` of one of the three sensitive names on such a
//! header.
//!
//! The rule keys entirely on the *signature*: a function with a
//! `*http.Request` parameter and a `[]*http.Request` parameter is a redirect
//! policy, whatever it is called and wherever it was declared. beats writes
//! three of them (cel, jamf, okta inputs), each as a func literal assigned to
//! `client.CheckRedirect`.
//!
//! The SSA program and the `SrcFuncs` list come from [`crate::gosec_ssa`],
//! which builds them once for every SSA-based gosec analyzer.

use std::collections::{HashMap, HashSet};

use guff_analysis::callcheck::static_callee;
use guff_ssa::function::Function;
use guff_ssa::ids::FuncId;
use guff_ssa::instr::InstrData;
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::TypeId;

use crate::gosec_g118::call_common;
use crate::gosec_g123::collect_analyzer_functions;

const HTTP_PKG: &str = "net/http";

/// gosec `MaxDepth`.
const MAX_DEPTH: u32 = 20;

pub(crate) const MSG_UNSAFE_REDIRECT_HEADER_COPY: &str =
    "G119: Unsafe redirect policy may propagate sensitive headers across origins";
pub(crate) const MSG_SENSITIVE_REDIRECT_HEADER: &str =
    "G119: Sensitive headers should not be re-added in redirect policy callbacks";

/// gosec `sensitiveRedirectHeaders`, compared lower-cased.
const SENSITIVE_REDIRECT_HEADERS: &[&str] = &["authorization", "proxy-authorization", "cookie"];

/// gosec `dependencyChecker`: does `value` derive from `target`?
///
/// The memo is keyed by the pair, and `visiting` breaks the cycles a `Phi` can
/// make. Both are per `valueDependsOn` call upstream — a fresh checker each
/// time — so the memo never crosses functions, which matters because SSA values
/// are function-local.
struct DependencyChecker {
    memo: HashMap<(Value, Value), bool>,
    visiting: HashSet<(Value, Value)>,
}

impl DependencyChecker {
    fn new() -> Self {
        Self {
            memo: HashMap::new(),
            visiting: HashSet::new(),
        }
    }

    fn depends_on(&mut self, func: &Function, value: Value, target: Value, depth: u32) -> bool {
        if depth > MAX_DEPTH {
            return false;
        }
        if value == target {
            return true;
        }
        let key = (value, target);
        if let Some(&result) = self.memo.get(&key) {
            return result;
        }
        if !self.visiting.insert(key) {
            return false;
        }

        let mut result = false;
        if let Value::Instr(iid) = value {
            match func.instrs.get(iid) {
                InstrData::ChangeType(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::MakeInterface(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::TypeAssert(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::UnOp(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::FieldAddr(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::Field(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1);
                }
                InstrData::IndexAddr(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1)
                        || self.depends_on(func, x.index, target, depth + 1);
                }
                InstrData::Index(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1)
                        || self.depends_on(func, x.index, target, depth + 1);
                }
                InstrData::Slice(x) => {
                    result = self.depends_on(func, x.x, target, depth + 1)
                        || x.low
                            .is_some_and(|v| self.depends_on(func, v, target, depth + 1))
                        || x.high
                            .is_some_and(|v| self.depends_on(func, v, target, depth + 1))
                        || x.max
                            .is_some_and(|v| self.depends_on(func, v, target, depth + 1));
                }
                InstrData::Extract(x) => {
                    result = self.depends_on(func, x.tuple, target, depth + 1);
                }
                InstrData::Phi(x) => {
                    // guff's `Phi.edges` are `Option<Value>`: a predecessor
                    // whose edge was never filled in has none.
                    result = x
                        .edges
                        .iter()
                        .filter_map(|e| *e)
                        .any(|e| self.depends_on(func, e, target, depth + 1));
                }
                InstrData::Call(c) => {
                    result = self.depends_on(func, c.call.value, target, depth + 1)
                        || c.call
                            .args
                            .iter()
                            .any(|&a| self.depends_on(func, a, target, depth + 1));
                }
                _ => {}
            }
        }

        self.visiting.remove(&key);
        self.memo.insert(key, result);
        result
    }
}

fn value_depends_on(func: &Function, value: Value, target: Value) -> bool {
    DependencyChecker::new().depends_on(func, value, target, 0)
}

/// `isHTTPRequestPointerType`: exactly `*net/http.Request`.
fn is_http_request_pointer_type(prog: &Program, t: TypeId) -> bool {
    let TypeData::Pointer(ptr) = prog.type_arena.get(t) else {
        return false;
    };
    named_is(prog, ptr.elem(), "Request")
}

/// `isRequestSliceType`: `[]*net/http.Request`.
fn is_request_slice_type(prog: &Program, t: TypeId) -> bool {
    let TypeData::Slice(s) = prog.type_arena.get(t) else {
        return false;
    };
    is_http_request_pointer_type(prog, s.elem())
}

/// `isHTTPHeaderType`: `net/http.Header`, or a pointer to one.
fn is_http_header_type(prog: &Program, t: TypeId) -> bool {
    let t = match prog.type_arena.get(t) {
        TypeData::Pointer(ptr) => ptr.elem(),
        _ => t,
    };
    named_is(prog, t, "Header")
}

fn named_is(prog: &Program, t: TypeId, name: &str) -> bool {
    let TypeData::Named(n) = prog.type_arena.get(t) else {
        return false;
    };
    let obj = n.obj();
    obj.name(&prog.object_arena) == name
        && obj
            .pkg(&prog.object_arena)
            .is_some_and(|pkg| prog.package_arena.get(pkg).path() == HTTP_PKG)
}

/// `findRedirectLikeParams`: the *first* `*http.Request` parameter, and whether
/// any parameter is a `[]*http.Request`.
///
/// Upstream's loop `continue`s after taking the request, so a signature whose
/// only `[]*Request` sits *before* the `*Request` still answers `hasVia` —
/// the two questions are independent.
fn find_redirect_like_params(prog: &Program, func: &Function) -> (Option<Value>, bool) {
    let mut req = None;
    let mut has_via = false;
    for (pid, param) in func.params.iter() {
        if req.is_none() && is_http_request_pointer_type(prog, param.typ) {
            req = Some(Value::Param(pid));
            continue;
        }
        if is_request_slice_type(prog, param.typ) {
            has_via = true;
        }
    }
    (req, has_via)
}

/// `isRequestHeaderStore`: a store whose address is a `http.Header` field of
/// something derived from `req`.
fn is_request_header_store(
    prog: &Program,
    func: &Function,
    addr: Value,
    req: Value,
) -> bool {
    let Value::Instr(iid) = addr else {
        return false;
    };
    let InstrData::FieldAddr(fa) = func.instrs.get(iid) else {
        return false;
    };
    if !is_http_header_type(prog, fa.typ) {
        return false;
    }
    value_depends_on(func, fa.x, req)
}

/// `isRequestHeaderValue`: a `http.Header` derived from `req`.
fn is_request_header_value(prog: &Program, func: &Function, v: Value, req: Value) -> bool {
    let t = value_type_of(prog, func, v);
    is_http_header_type(prog, t) && value_depends_on(func, v, req)
}

/// `isHeaderMutationCall`: a *static* call to `Set` or `Add` whose receiver is
/// a `http.Header`.
fn is_header_mutation_call(prog: &Program, instr: &InstrData) -> bool {
    let Some(common) = call_common(instr) else {
        return false;
    };
    let Some(callee) = static_callee(common) else {
        return false;
    };
    let callee_fn = prog.functions.get(callee);
    if callee_fn.name != "Set" && callee_fn.name != "Add" {
        return false;
    }
    let Some(sig) = callee_fn.signature else {
        return false;
    };
    let TypeData::Signature(s) = prog.type_arena.get(sig) else {
        return false;
    };
    let Some(recv) = s.recv() else {
        return false;
    };
    let recv_type = recv.typ(&prog.object_arena);
    recv_type.is_some_and(|t| is_http_header_type(prog, t))
}

/// `extractStringConst`.
fn extract_string_const(prog: &Program, v: Value) -> Option<String> {
    let Value::Const(id) = v else {
        return None;
    };
    let c = prog.constants.get(id);
    let val = c.val.as_ref()?;
    match val {
        guff_constant::Value::String(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    }
}

/// Collects G119 out of the SSA build [`crate::gosec_ssa`] shares between the
/// gosec analyzers, appending `(pos, pos, message)` into `pending`.
pub(crate) fn collect_g119(
    prog: &Program,
    src_funcs: &[FuncId],
    pending: &mut Vec<(u32, u32, String)>,
) {
    let funcs = collect_analyzer_functions(prog, src_funcs);
    // `issuesByPos`: one issue per position, first writer wins.
    let mut seen_pos: HashSet<u32> = HashSet::new();
    for fid in funcs {
        let func = prog.functions.get(fid);
        let (Some(req), true) = find_redirect_like_params(prog, func) else {
            continue;
        };
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let instr = func.instrs.get(iid);
                match instr {
                    InstrData::Store(st) => {
                        if !is_request_header_store(prog, func, st.addr, req) {
                            continue;
                        }
                        let pos = func.pos(iid);
                        if pos.is_valid() && seen_pos.insert(pos.0 as u32) {
                            pending.push((
                                pos.0 as u32,
                                pos.0 as u32,
                                MSG_UNSAFE_REDIRECT_HEADER_COPY.to_string(),
                            ));
                        }
                    }
                    InstrData::Call(c) => {
                        if !is_header_mutation_call(prog, instr) {
                            continue;
                        }
                        if c.call.args.len() < 2 {
                            continue;
                        }
                        if !is_request_header_value(prog, func, c.call.args[0], req) {
                            continue;
                        }
                        let Some(name) = extract_string_const(prog, c.call.args[1]) else {
                            continue;
                        };
                        if !SENSITIVE_REDIRECT_HEADERS.contains(&name.to_ascii_lowercase().as_str())
                        {
                            continue;
                        }
                        let pos = func.pos(iid);
                        if pos.is_valid() && seen_pos.insert(pos.0 as u32) {
                            pending.push((
                                pos.0 as u32,
                                pos.0 as u32,
                                MSG_SENSITIVE_REDIRECT_HEADER.to_string(),
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
