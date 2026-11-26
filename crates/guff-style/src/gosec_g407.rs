//! G407 — use of a hardcoded IV/nonce for encryption.
//!
//! Port of gosec v2.29.0 `analyzers/hardcoded_nonce.go`, the SSA analyzer
//! golangci-lint 2.14.0 runs by default (2.12.2 excluded it). The nonce
//! argument of each tracked encryption call is traced back through slices,
//! loads, conversions, locals, function results and — through every call site
//! — parameters; a constant, a global, a slice literal or a `make` buffer
//! nothing dynamic fills is reported. "Dynamic" is `crypto/rand.Read` /
//! `io.ReadFull` (or a function that only ever passes the buffer to one), and
//! a buffer a dynamic read fully overwrites before the call is safe: the byte
//! ranges each hardcoded store and each dynamic read touches are replayed in
//! execution order.
//!
//! Values are `(function, value)` pairs: guff-ssa values are function-local.

use std::collections::{HashMap, HashSet};

use guff::token::Token;
use guff_analysis::referrers;
use guff_ssa::function::Function;
use guff_ssa::ids::{BlockId, FuncId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::typestring::type_string;

use crate::gosec_g115::RangeAnalyzer;

const MAX_DEPTH: u32 = 20;

const DEFAULT_DESCRIPTION: &str = "Use of hardcoded IV/nonce for encryption";

/// `tracked`: the encryption call, its argument count, and the nonce's index.
/// Decryption is not tracked: it must reuse the encryption's nonce, which
/// naturally looks hardcoded.
const TRACKED: &[(&str, usize, usize)] = &[
    ("(crypto/cipher.AEAD).Seal", 4, 1),
    ("crypto/cipher.NewCBCEncrypter", 2, 1),
    ("crypto/cipher.NewCFBEncrypter", 2, 1),
    ("crypto/cipher.NewCTREncrypter", 2, 1),
    ("crypto/cipher.NewCTR", 2, 1),
    ("crypto/cipher.NewOFB", 2, 1),
    ("crypto/cipher.NewCFB", 2, 1),
    ("crypto/cipher.NewCBC", 2, 1),
];

const DYNAMIC_FUNCS: &[&str] = &["crypto/rand.Read", "io.ReadFull"];
const DYNAMIC_PKGS: &[&str] = &["crypto/rand", "io"];
const CIPHER_PKG_PREFIXES: &[&str] = &["crypto/cipher", "crypto/aes"];

const STATUS_VISITING: u8 = 1 << 0;
const STATUS_HARD: u8 = 1 << 1;
const STATUS_DYN: u8 = 1 << 2;

type V = (FuncId, Value);
type I = (FuncId, InstrId);

/// `ByteRange`: `[low, high)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ByteRange {
    low: i64,
    high: i64,
}

#[derive(Clone, Copy)]
struct RangeAction {
    instr: I,
    range: ByteRange,
    is_safe: bool,
}

pub(crate) fn collect_g407(prog: &Program, src_funcs: &[FuncId], pending: &mut Vec<(u32, u32, String)>) {
    let mut s = State::new(prog, src_funcs);
    let args = s.initial_args();
    for (val, instr) in args {
        s.reset();
        for (pos, msg) in s.raise_issue(val, String::new(), instr) {
            pending.push((pos, pos, format!("G407: {msg}")));
        }
    }
}

struct State<'a> {
    prog: &'a Program,
    src_funcs: &'a [FuncId],
    visited: HashSet<V>,
    func_map: HashSet<FuncId>,
    closure_cache: HashSet<V>,
    depth: u32,
    usage_cache: HashMap<V, u8>,
    /// `callerMap`: callee name → its `*ssa.Call` sites.
    caller_map: HashMap<String, Vec<I>>,
    /// The per-function `RangeAnalyzer`s, their byte-range caches, and
    /// `Instruction.Block()`.
    ranges: HashMap<FuncId, RangeAnalyzer<'a>>,
    byte_range_cache: HashMap<V, ByteRange>,
    range_depth: u32,
    block_of: HashMap<FuncId, HashMap<InstrId, BlockId>>,
}

impl<'a> State<'a> {
    fn new(prog: &'a Program, src_funcs: &'a [FuncId]) -> Self {
        let mut s = Self {
            prog,
            src_funcs,
            visited: HashSet::new(),
            func_map: HashSet::new(),
            closure_cache: HashSet::new(),
            depth: 0,
            usage_cache: HashMap::new(),
            caller_map: HashMap::new(),
            ranges: HashMap::new(),
            byte_range_cache: HashMap::new(),
            range_depth: 0,
            block_of: HashMap::new(),
        };
        s.build_caller_map();
        s
    }

    fn func(&self, fid: FuncId) -> &'a Function {
        self.prog.functions.get(fid)
    }

    /// `BaseAnalyzerState.Reset`: everything but the usage cache.
    fn reset(&mut self) {
        self.ranges.clear();
        self.byte_range_cache.clear();
        self.range_depth = 0;
        self.visited.clear();
        self.func_map.clear();
        self.closure_cache.clear();
        self.depth = 0;
    }

    /// `TraverseSSA` over the source functions, in block and instruction order.
    fn each_instr(&self) -> Vec<I> {
        let mut out = Vec::new();
        for &fid in self.src_funcs {
            let f = self.func(fid);
            for (_, block) in f.live_blocks() {
                out.extend(block.instrs.iter().map(|&i| (fid, i)));
            }
        }
        out
    }

    /// `BuildCallerMap`: every `*ssa.Call`, keyed by `Method.FullName()` for an
    /// interface call and by `Call.Value.String()` otherwise.
    fn build_caller_map(&mut self) {
        for (fid, iid) in self.each_instr() {
            let InstrData::Call(c) = self.func(fid).instrs.get(iid) else {
                continue;
            };
            let name = match c.call.method {
                Some(m) => method_full_name(self.prog, m),
                None => match c.call.value {
                    Value::Function(f) => full_name(self.prog, f),
                    _ => continue,
                },
            };
            self.caller_map.entry(name).or_default().push((fid, iid));
        }
    }

    /// `getInitialArgs`: the nonce argument of every tracked call.
    fn initial_args(&mut self) -> Vec<(V, I)> {
        let mut out = Vec::new();
        for (fid, iid) in self.each_instr() {
            let InstrData::Call(c) = self.func(fid).instrs.get(iid) else {
                continue;
            };
            if let Some(m) = c.call.method {
                let name = method_full_name(self.prog, m);
                // AEAD.Open decrypts: it must reuse the encryption's nonce.
                if name.contains("AEAD") && name.ends_with("Open") {
                    continue;
                }
                if let Some(&(_, n, idx)) = TRACKED.iter().find(|(t, _, _)| *t == name) {
                    if c.call.args.len() == n {
                        out.push(((fid, c.call.args[idx]), (fid, iid)));
                    }
                }
                continue;
            }
            self.closure_cache.clear();
            let mut funcs = Vec::new();
            self.resolve_funcs((fid, c.call.value), &mut funcs);
            for f in funcs {
                let mut hit = None;
                for name in [full_name(self.prog, f), pkg_qualified_name(self.prog, f)] {
                    if let Some(&(_, n, idx)) = TRACKED.iter().find(|(t, _, _)| *t == name) {
                        hit = Some((n, idx));
                        break;
                    }
                }
                if let Some((n, idx)) = hit {
                    if c.call.args.len() == n {
                        out.push(((fid, c.call.args[idx]), (fid, iid)));
                        break;
                    }
                }
            }
        }
        out
    }

    /// `BaseAnalyzerState.ResolveFuncs`.
    fn resolve_funcs(&mut self, v: V, out: &mut Vec<FuncId>) {
        if self.depth > MAX_DEPTH || !self.closure_cache.insert(v) {
            return;
        }
        self.depth += 1;
        let (fid, val) = v;
        match val {
            Value::Function(f) => out.push(f),
            Value::Instr(i) => match self.func(fid).instrs.get(i) {
                InstrData::MakeClosure(mc) => out.push(mc.fn_),
                InstrData::Phi(p) => {
                    for e in p.edges.iter().flatten() {
                        self.resolve_funcs((fid, *e), out);
                    }
                }
                InstrData::ChangeType(c) => self.resolve_funcs((fid, c.x), out),
                InstrData::UnOp(u) if u.op == Token::MUL => self.resolve_funcs((fid, u.x), out),
                _ => {}
            },
            _ => {}
        }
        self.depth -= 1;
    }

    fn instr(&self, v: V) -> Option<&'a InstrData> {
        match v.1 {
            Value::Instr(i) => Some(self.func(v.0).instrs.get(i)),
            _ => None,
        }
    }

    /// `Value.Referrers()`, nil for constants, globals, builtins and
    /// package-level functions.
    fn referrers(&self, v: V) -> Vec<I> {
        let (fid, val) = v;
        match val {
            Value::Const(_) | Value::Global(_) | Value::Builtin(_) => Vec::new(),
            Value::Function(f) if self.func(f).parent.is_none() => Vec::new(),
            _ => referrers(self.func(fid), val).iter().map(|&i| (fid, i)).collect(),
        }
    }

    fn type_string_of(&self, v: V) -> String {
        let t = value_type_of(self.prog, self.func(v.0), v.1);
        type_string(&self.prog.type_arena, &self.prog.object_arena, &self.prog.package_arena, t, None)
    }

    /// The parameter's index and its function.
    fn param(&self, v: V) -> Option<(FuncId, usize)> {
        let Value::Param(pid) = v.1 else { return None };
        let f = self.func(v.0);
        let parent = f.params.get(pid).parent;
        let idx = self.func(parent).params.iter().position(|(p, _)| p == pid)?;
        Some((parent, idx))
    }

    fn params(&self, f: FuncId) -> Vec<Value> {
        self.func(f).params.iter().map(|(p, _)| Value::Param(p)).collect()
    }

    // ---- raiseIssue ------------------------------------------------------

    /// `raiseIssue`: (position, message) of each issue `val` raises at `from`.
    fn raise_issue(&mut self, val: V, mut desc: String, from: I) -> Vec<(u32, String)> {
        if !self.visited.insert(val) {
            return Vec::new();
        }
        let res = self.analyze_usage(val);
        if res & STATUS_DYN != 0 && self.all_tainted_events_covered(val, from) {
            return Vec::new();
        }
        if desc.is_empty() {
            desc = DEFAULT_DESCRIPTION.to_string();
        }
        let at = self.func(from.0).pos(from.1).0 as u32;
        let mut issues = Vec::new();
        let (fid, value) = val;
        match value {
            Value::Const(_) => {
                desc.push_str(" by passing hardcoded constant");
                issues.push((at, desc));
            }
            Value::Global(_) => {
                desc.push_str(" by passing hardcoded global");
                issues.push((at, desc));
            }
            Value::Param(_) => {
                if let Some((parent, idx)) = self.param(val) {
                    let n = self.func(parent).params.len();
                    desc.push_str(" by passing a parameter to a function and");
                    let name = full_name(self.prog, parent);
                    let callers = self.caller_map.get(&name).cloned().unwrap_or_default();
                    for c in callers {
                        let InstrData::Call(call) = self.func(c.0).instrs.get(c.1) else {
                            continue;
                        };
                        if call.call.args.len() == n {
                            let arg = (c.0, call.call.args[idx]);
                            issues.extend(self.raise_issue(arg, desc.clone(), c));
                        }
                    }
                }
            }
            Value::Instr(i) => match self.func(fid).instrs.get(i) {
                InstrData::Slice(sl) => {
                    let x = (fid, sl.x);
                    if self.is_hardcoded(x) {
                        desc.push_str(" by passing hardcoded slice/array");
                    }
                    return self.raise_issue(x, desc, from);
                }
                InstrData::UnOp(u) if u.op == Token::MUL => {
                    let x = (fid, u.x);
                    if self.is_hardcoded(x) {
                        desc.push_str(" by passing pointer which points to hardcoded variable");
                    }
                    return self.raise_issue(x, desc, from);
                }
                InstrData::Convert(cv) => {
                    let x = (fid, cv.x);
                    if self.type_string_of(val) == "[]byte"
                        && self.type_string_of(x) == "string"
                        && self.is_hardcoded(x)
                    {
                        desc.push_str(" by passing converted string");
                    }
                    return self.raise_issue(x, desc, from);
                }
                InstrData::Alloc(a) => match a.comment.as_str() {
                    "slicelit" => {
                        desc.push_str(" by passing hardcoded slice literal");
                        issues.push((at, desc));
                    }
                    "makeslice" => {
                        let r = self.analyze_usage(val);
                        if self.all_tainted_events_covered(val, from) {
                            return Vec::new();
                        }
                        if r & STATUS_HARD != 0 {
                            desc.push_str(" by passing a buffer from make modified with hardcoded values");
                        } else {
                            desc.push_str(" by passing a zeroed buffer from make");
                        }
                        issues.push((at, desc));
                    }
                    _ => {
                        // The Store that tainted this local.
                        for r in self.referrers(val) {
                            if let InstrData::Store(st) = self.func(r.0).instrs.get(r.1) {
                                if st.addr == value {
                                    issues.extend(self.raise_issue((r.0, st.val), desc.clone(), from));
                                }
                            }
                        }
                    }
                },
                InstrData::MakeSlice(_) => {
                    let r = self.analyze_usage(val);
                    if r & STATUS_HARD != 0 {
                        desc.push_str(" by passing a buffer from make modified with hardcoded values");
                        issues.push((at, desc));
                    } else if r & STATUS_DYN == 0 {
                        desc.push_str(" by passing a zeroed buffer from make");
                        issues.push((at, desc));
                    }
                }
                InstrData::Call(_) => {
                    if self.is_hardcoded(val) {
                        desc.push_str(" by passing a value from function which returns hardcoded value");
                        issues.push((at, desc));
                    }
                }
                _ => {}
            },
            _ => {}
        }
        issues
    }

    // ---- isHardcoded -----------------------------------------------------

    fn is_hardcoded(&mut self, val: V) -> bool {
        if self.depth > MAX_DEPTH {
            return false;
        }
        self.depth += 1;
        let r = self.is_hardcoded_inner(val);
        self.depth -= 1;
        r
    }

    fn is_hardcoded_inner(&mut self, val: V) -> bool {
        let (fid, value) = val;
        match value {
            Value::Const(_) | Value::Global(_) => true,
            Value::Param(_) => {
                let Some((parent, idx)) = self.param(val) else { return false };
                if !self.func_map.insert(parent) {
                    return false;
                }
                // Upstream keys this lookup by `Pkg.Path() + "." + Name()`,
                // which is not `String()` for a method: such a parameter is
                // never traced.
                let name = pkg_qualified_name(self.prog, parent);
                let mut found = false;
                for c in self.caller_map.get(&name).cloned().unwrap_or_default() {
                    let InstrData::Call(call) = self.func(c.0).instrs.get(c.1) else { continue };
                    if let Some(&arg) = call.call.args.get(idx) {
                        if self.is_hardcoded((c.0, arg)) {
                            found = true;
                            break;
                        }
                    }
                }
                self.func_map.remove(&parent);
                found
            }
            Value::Instr(i) => match self.func(fid).instrs.get(i) {
                InstrData::Convert(c) => self.is_hardcoded((fid, c.x)),
                InstrData::Slice(s) => self.is_hardcoded((fid, s.x)),
                InstrData::UnOp(u) if u.op == Token::MUL => self.is_hardcoded((fid, u.x)),
                InstrData::Alloc(a) => a.comment == "slicelit",
                InstrData::MakeSlice(_) => {
                    let r = self.analyze_usage(val);
                    r & STATUS_HARD != 0 || r & STATUS_DYN == 0
                }
                InstrData::Call(c) => {
                    let Value::Function(f) = c.call.value else { return false };
                    if !self.func_map.insert(f) {
                        return false;
                    }
                    let r = self.func_returns_hardcoded(f);
                    self.func_map.remove(&f);
                    r
                }
                _ => false,
            },
            _ => false,
        }
    }

    fn func_returns_hardcoded(&mut self, f: FuncId) -> bool {
        let func = self.func(f);
        let mut results = Vec::new();
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                if let InstrData::Return(r) = func.instrs.get(iid) {
                    results.push(r.results.clone());
                }
            }
        }
        results.into_iter().any(|rs| rs.into_iter().any(|v| self.is_hardcoded((f, v))))
    }

    // ---- analyzeUsage ----------------------------------------------------

    fn analyze_usage(&mut self, val: V) -> u8 {
        if self.depth > MAX_DEPTH {
            return STATUS_DYN;
        }
        if let Some(&r) = self.usage_cache.get(&val) {
            return r;
        }
        self.usage_cache.insert(val, STATUS_VISITING);
        self.depth += 1;
        let (fid, value) = val;
        let mut res: u8 = 0;
        match value {
            Value::Const(_) | Value::Global(_) => res |= STATUS_HARD,
            Value::Param(_) => {
                if self.is_hardcoded(val) {
                    res |= STATUS_HARD;
                }
            }
            Value::Instr(i) => match self.func(fid).instrs.get(i) {
                InstrData::Alloc(a) if a.comment == "slicelit" => res |= STATUS_HARD,
                InstrData::Convert(c) => res |= self.analyze_usage((fid, c.x)),
                InstrData::Slice(s) => res |= self.analyze_usage((fid, s.x)),
                InstrData::UnOp(u) if u.op == Token::MUL => res |= self.analyze_usage((fid, u.x)),
                InstrData::Call(_) => {
                    if self.is_hardcoded(val) {
                        res |= STATUS_HARD;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        for r in self.referrers(val) {
            res |= self.analyze_referrer(r, val);
            if res & STATUS_DYN != 0 && res & STATUS_HARD != 0 {
                let fin = res & !STATUS_VISITING;
                self.usage_cache.insert(val, fin);
                self.depth -= 1;
                return fin;
            }
        }
        if let Some(InstrData::Slice(sl)) = self.instr(val) {
            if res & STATUS_DYN == 0 {
                for sr in self.referrers((fid, sl.x)) {
                    if sr == (fid, match value { Value::Instr(i) => i, _ => unreachable!() }) {
                        continue;
                    }
                    if let InstrData::Slice(other) = self.func(sr.0).instrs.get(sr.1) {
                        if self.is_sub_slice(fid, sl, other) {
                            let other_res = self.analyze_usage((sr.0, Value::Instr(sr.1)));
                            if (other_res & !STATUS_VISITING) & STATUS_DYN != 0 {
                                res |= STATUS_DYN;
                                break;
                            }
                        }
                    }
                }
            }
        }
        let fin = res & !STATUS_VISITING;
        self.usage_cache.insert(val, fin);
        self.depth -= 1;
        fin
    }

    /// `(callee.Pkg.Pkg.Path(), callee.Name())` of a static callee.
    fn analyze_referrer(&mut self, r: I, val: V) -> u8 {
        let mut res: u8 = 0;
        let (fid, iid) = r;
        match self.func(fid).instrs.get(iid) {
            InstrData::Call(call) => {
                let call = &call.call;
                let mut is_dynamic = false;
                let mut is_cipher = false;
                if let Some(path) = static_callee_pkg(self.prog, call) {
                    let Value::Function(f) = call.value else { unreachable!() };
                    let name = format!("{path}.{}", self.func(f).name);
                    if DYNAMIC_FUNCS.contains(&name.as_str()) {
                        is_dynamic = true;
                    } else {
                        is_cipher = CIPHER_PKG_PREFIXES.iter().any(|p| path.starts_with(p));
                    }
                } else if let Some(path) = call
                    .method
                    .and_then(|m| m.pkg(&self.prog.object_arena))
                    .map(|p| self.prog.package_arena.get(p).path().to_string())
                {
                    if DYNAMIC_PKGS.contains(&path.as_str()) {
                        is_dynamic = true;
                    } else {
                        is_cipher = CIPHER_PKG_PREFIXES.iter().any(|p| path.starts_with(p));
                    }
                } else {
                    // `callValue.String()`, matched by substring: only a
                    // closure's string names a function.
                    let s = match call.value {
                        Value::Instr(i) => match self.func(fid).instrs.get(i) {
                            InstrData::MakeClosure(mc) => full_name(self.prog, mc.fn_),
                            _ => String::new(),
                        },
                        _ => String::new(),
                    };
                    if DYNAMIC_FUNCS.iter().any(|k| s.contains(k)) {
                        is_dynamic = true;
                    } else {
                        is_cipher = CIPHER_PKG_PREFIXES.iter().any(|p| s.contains(p));
                    }
                }
                if is_dynamic {
                    return STATUS_DYN;
                }
                if is_cipher {
                    return 0;
                }
                self.closure_cache.clear();
                let mut funcs = Vec::new();
                self.resolve_funcs((fid, call.value), &mut funcs);
                if funcs.is_empty() {
                    // Unknown callee: assume it may fill the buffer.
                    return STATUS_DYN;
                }
                let args = packed_args(self.prog, self.func(fid), call);
                for f in funcs {
                    let params = self.params(f);
                    for (i, &arg) in args.iter().enumerate() {
                        if (fid, arg) == val && i < params.len() {
                            res |= self.analyze_usage((f, params[i]));
                        }
                    }
                }
                res
            }
            InstrData::Slice(sl) => {
                let me = (fid, Value::Instr(iid));
                for rr in self.referrers(me) {
                    res |= self.analyze_referrer(rr, me);
                }
                let buf_len = self.buffered_len((fid, sl.x));
                if !self.is_full_slice(fid, sl, buf_len) {
                    res &= !STATUS_DYN;
                }
                res
            }
            InstrData::IndexAddr(_) | InstrData::Index(_) | InstrData::Lookup(_) => {
                res | (self.analyze_usage((fid, Value::Instr(iid))) & STATUS_HARD)
            }
            InstrData::UnOp(u) if u.op == Token::MUL => res | self.analyze_usage((fid, Value::Instr(iid))),
            InstrData::Convert(_) => res | self.analyze_usage((fid, Value::Instr(iid))),
            InstrData::Store(st) => {
                if (fid, st.addr) == val {
                    let v = self.analyze_usage((fid, st.val));
                    res |= v & (STATUS_HARD | STATUS_DYN);
                }
                res
            }
            _ => res,
        }
    }

    // ---- byte ranges -----------------------------------------------------

    fn all_tainted_events_covered(&mut self, val: V, usage: I) -> bool {
        let mut actions: Vec<RangeAction> = Vec::new();
        let mut v = val;
        let mut guard = 0;
        loop {
            guard += 1;
            if guard > 64 {
                break;
            }
            self.collect_tainted_events(v, usage, &mut actions, 0);
            self.collect_covered_ranges(v, usage, &mut actions, 0);
            let next = match self.instr(v) {
                Some(InstrData::UnOp(u)) if u.op == Token::MUL => Some(u.x),
                Some(InstrData::Slice(s)) => Some(s.x),
                Some(InstrData::Convert(c)) => Some(c.x),
                Some(InstrData::IndexAddr(ia)) => Some(ia.x),
                Some(InstrData::Alloc(_)) => self
                    .referrers(v)
                    .into_iter()
                    .find_map(|r| match self.func(r.0).instrs.get(r.1) {
                        InstrData::Store(st) if st.addr == v.1 => Some(st.val),
                        _ => None,
                    }),
                _ => None,
            };
            match next {
                Some(n) => v = (v.0, n),
                None => break,
            }
        }

        let mut buf_len: i64 = 0;
        match self.instr(v) {
            Some(InstrData::Alloc(a)) => {
                buf_len = self.buffered_len(v);
                if a.comment == "slicelit" || a.comment == "makeslice" {
                    let Value::Instr(i) = v.1 else { unreachable!() };
                    actions.push(RangeAction {
                        instr: (v.0, i),
                        range: ByteRange { low: 0, high: buf_len },
                        is_safe: false,
                    });
                }
            }
            Some(InstrData::MakeSlice(mk)) => {
                if let Some(l) = mk.len.and_then(|l| self.constant_int64(v.0, l)) {
                    if l > 0 {
                        buf_len = l;
                        let Value::Instr(i) = v.1 else { unreachable!() };
                        actions.push(RangeAction {
                            instr: (v.0, i),
                            range: ByteRange { low: 0, high: buf_len },
                            is_safe: false,
                        });
                    }
                }
            }
            _ => {
                if let Some(InstrData::Convert(c)) = self.instr(val) {
                    if let Some(s) = self.const_string(c.x) {
                        buf_len = s.len() as i64;
                    }
                } else if let Some(r) = self.resolve_byte_range(v) {
                    buf_len = r.high;
                }
            }
        }
        if buf_len <= 0 {
            return false;
        }

        // `slices.SortFunc` with `Precedes`, which is not a total order: Go's
        // pdqsort is an insertion sort below 12 elements, so that is what
        // runs here.
        for i in 1..actions.len() {
            let mut j = i;
            while j > 0 && self.cmp_actions(&actions[j], &actions[j - 1]) < 0 {
                actions.swap(j, j - 1);
                j -= 1;
            }
        }

        let mut safe: Vec<ByteRange> = Vec::new();
        let mut i = 0;
        while i < actions.len() {
            if actions[i].is_safe {
                let mut j = i;
                while j < actions.len() && actions[j].is_safe {
                    safe.push(actions[j].range);
                    j += 1;
                }
                safe = merge_ranges(safe);
                i = j;
            } else {
                safe = subtract_range(&safe, actions[i].range);
                i += 1;
            }
        }

        let Some(target) = self.resolve_byte_range(val) else {
            return false;
        };
        safe.iter().any(|r| r.low <= target.low && r.high >= target.high)
    }

    fn cmp_actions(&mut self, a: &RangeAction, b: &RangeAction) -> i32 {
        if self.precedes(a.instr, b.instr) {
            -1
        } else if a.instr == b.instr {
            0
        } else {
            1
        }
    }

    fn collect_tainted_events(&mut self, val: V, usage: I, actions: &mut Vec<RangeAction>, depth: u32) {
        if depth > 64 {
            return;
        }
        for r in self.referrers(val) {
            let hard = self.analyze_referrer(r, val) & STATUS_HARD != 0;
            let store = match self.func(r.0).instrs.get(r.1) {
                InstrData::Store(st) if (r.0, st.addr) == val => Some(st.val),
                _ => None,
            };
            if hard && self.precedes(r, usage) && store.is_some() {
                if let Some(range) = self.resolve_byte_range(val) {
                    actions.push(RangeAction { instr: r, range, is_safe: false });
                }
            }
            if let Some(sv) = store {
                self.collect_tainted_events((r.0, sv), usage, actions, depth + 1);
            }
            match self.func(r.0).instrs.get(r.1) {
                InstrData::Slice(_) | InstrData::IndexAddr(_) => {
                    self.collect_tainted_events((r.0, Value::Instr(r.1)), usage, actions, depth + 1)
                }
                InstrData::UnOp(u) if u.op == Token::MUL => {
                    self.collect_tainted_events((r.0, Value::Instr(r.1)), usage, actions, depth + 1)
                }
                _ => {}
            }
        }
    }

    fn collect_covered_ranges(&mut self, val: V, usage: I, actions: &mut Vec<RangeAction>, depth: u32) {
        if depth > 64 {
            return;
        }
        for r in self.referrers(val) {
            if self.is_full_dynamic_read(r, val) && self.precedes(r, usage) {
                if let Some(range) = self.resolve_byte_range(val) {
                    actions.push(RangeAction { instr: r, range, is_safe: true });
                }
            }
            if let InstrData::Store(st) = self.func(r.0).instrs.get(r.1) {
                if (r.0, st.addr) == val {
                    self.collect_covered_ranges((r.0, st.val), usage, actions, depth + 1);
                }
            }
            match self.func(r.0).instrs.get(r.1) {
                InstrData::Slice(_) | InstrData::IndexAddr(_) => {
                    self.collect_covered_ranges((r.0, Value::Instr(r.1)), usage, actions, depth + 1)
                }
                InstrData::UnOp(u) if u.op == Token::MUL => {
                    self.collect_covered_ranges((r.0, Value::Instr(r.1)), usage, actions, depth + 1)
                }
                _ => {}
            }
        }
    }

    /// `isFullDynamicRead`.
    fn is_full_dynamic_read(&mut self, r: I, val: V) -> bool {
        let (fid, iid) = r;
        let InstrData::Call(call) = self.func(fid).instrs.get(iid) else {
            return false;
        };
        let call = &call.call;
        let is_dynamic = if let Some(path) = static_callee_pkg(self.prog, call) {
            let Value::Function(f) = call.value else { unreachable!() };
            DYNAMIC_FUNCS.contains(&format!("{path}.{}", self.func(f).name).as_str())
        } else if let Some(path) = call
            .method
            .and_then(|m| m.pkg(&self.prog.object_arena))
            .map(|p| self.prog.package_arena.get(p).path().to_string())
        {
            DYNAMIC_PKGS.contains(&path.as_str())
        } else {
            false
        };
        if is_dynamic {
            return call.args.iter().any(|&a| (fid, a) == val);
        }
        // A user function that only ever passes the buffer to a dynamic read;
        // an unresolvable callee counts as one.
        self.closure_cache.clear();
        let mut funcs = Vec::new();
        self.resolve_funcs((fid, call.value), &mut funcs);
        if funcs.is_empty() {
            return true;
        }
        let args = packed_args(self.prog, self.func(fid), call);
        for f in funcs {
            let params = self.params(f);
            for (i, &arg) in args.iter().enumerate() {
                if (fid, arg) == val && i < params.len() {
                    let st = self.analyze_usage((f, params[i]));
                    if st & STATUS_DYN != 0 && st & STATUS_HARD == 0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn range_analyzer(&mut self, fid: FuncId) -> &mut RangeAnalyzer<'a> {
        let prog = self.prog;
        let func = self.func(fid);
        self.ranges.entry(fid).or_insert_with(|| RangeAnalyzer::new(prog, func))
    }

    fn block_of(&mut self, i: I) -> Option<BlockId> {
        let func = self.func(i.0);
        self.block_of
            .entry(i.0)
            .or_insert_with(|| {
                let mut m = HashMap::new();
                for (bid, block) in func.live_blocks() {
                    for &iid in &block.instrs {
                        m.insert(iid, bid);
                    }
                }
                m
            })
            .get(&i.1)
            .copied()
    }

    /// `RangeAnalyzer.Precedes`: within one function.
    fn precedes(&mut self, a: I, b: I) -> bool {
        if a == b {
            return true;
        }
        if a.0 != b.0 {
            return false;
        }
        let (Some(ba), Some(bb)) = (self.block_of(a), self.block_of(b)) else {
            return false;
        };
        if ba != bb {
            return self.range_analyzer(a.0).is_reachable(ba, bb, None);
        }
        for &iid in &self.func(a.0).blocks.get(ba).instrs {
            if iid == a.1 {
                return true;
            }
            if iid == b.1 {
                return false;
            }
        }
        false
    }

    /// `RangeAnalyzer.BufferedLen` → `GetBufferLen`.
    fn buffered_len(&self, v: V) -> i64 {
        let mut cur = v;
        loop {
            let types = &self.prog.type_arena;
            let mut t = value_type_of(self.prog, self.func(cur.0), cur.1).underlying(types);
            if let TypeData::Pointer(p) = types.get(t) {
                t = p.elem().underlying(types);
            }
            if let TypeData::Array(a) = types.get(t) {
                return a.len();
            }
            match self.instr(cur) {
                Some(InstrData::Slice(s)) => cur = (cur.0, s.x),
                _ => return -1,
            }
        }
    }

    fn constant_int64(&self, fid: FuncId, v: Value) -> Option<i64> {
        crate::gosec_g115::constant_int64(self.prog, self.func(fid), v)
    }

    fn const_string(&self, v: Value) -> Option<String> {
        let Value::Const(c) = v else { return None };
        let val = self.prog.constants.get(c).val.as_ref()?;
        (val.kind() == guff_constant::Kind::String).then(|| guff_constant::string_val_lossy(val))
    }

    /// `GetSliceRange`: low, and high (`-1` when absent).
    fn slice_range(&self, fid: FuncId, s: &guff_ssa::instr::Slice) -> (i64, i64) {
        let low = s.low.and_then(|l| self.constant_int64(fid, l)).unwrap_or(0);
        let high = match s.high {
            Some(h) => self.constant_int64(fid, h).unwrap_or(-1),
            None => -1,
        };
        (low, high)
    }

    fn is_full_slice(&self, fid: FuncId, s: &guff_ssa::instr::Slice, buf_len: i64) -> bool {
        let (l, h) = self.slice_range(fid, s);
        if l != 0 {
            return false;
        }
        if h < 0 {
            return true;
        }
        buf_len >= 0 && h == buf_len
    }

    fn is_sub_slice(&self, fid: FuncId, sub: &guff_ssa::instr::Slice, sup: &guff_ssa::instr::Slice) -> bool {
        let (l1, h1) = self.slice_range(fid, sub);
        let (l2, h2) = self.slice_range(fid, sup);
        if l2 > l1 {
            return false;
        }
        if h2 < 0 {
            return true;
        }
        if h1 < 0 {
            return false;
        }
        h1 <= h2
    }

    /// `RangeAnalyzer.ResolveByteRange`.
    fn resolve_byte_range(&mut self, v: V) -> Option<ByteRange> {
        if let Some(&r) = self.byte_range_cache.get(&v) {
            return Some(r);
        }
        if self.range_depth > MAX_DEPTH {
            return None;
        }
        self.range_depth += 1;
        let r = self.recursive_byte_range(v);
        self.range_depth -= 1;
        if let Some(r) = r {
            self.byte_range_cache.insert(v, r);
        }
        r
    }

    fn recursive_byte_range(&mut self, v: V) -> Option<ByteRange> {
        let (fid, _) = v;
        match self.instr(v)? {
            InstrData::Alloc(_) => {
                let l = self.buffered_len(v);
                if l <= 0 {
                    let stored = self.referrers(v).into_iter().find_map(|r| match self.func(r.0).instrs.get(r.1) {
                        InstrData::Store(st) if st.addr == v.1 => Some(st.val),
                        _ => None,
                    })?;
                    return self.recursive_byte_range((fid, stored));
                }
                Some(ByteRange { low: 0, high: l })
            }
            InstrData::MakeSlice(mk) => {
                let l = mk.len.and_then(|l| self.constant_int64(fid, l))?;
                (l > 0).then_some(ByteRange { low: 0, high: l })
            }
            InstrData::Convert(c) => {
                let s = self.const_string(c.x)?;
                let l = s.len() as i64;
                (l > 0).then_some(ByteRange { low: 0, high: l })
            }
            InstrData::Slice(s) => {
                let parent = self.recursive_byte_range((fid, s.x))?;
                let Value::Instr(iid) = v.1 else { return None };
                let block = self.block_of((fid, iid))?;
                let low = match s.low {
                    None => 0,
                    Some(l) => match self.constant_int64(fid, l) {
                        Some(c) => c,
                        None => self.range_max(fid, l, block)?,
                    },
                };
                let high = match s.high {
                    None => parent.high,
                    Some(h) => {
                        let h = match self.constant_int64(fid, h) {
                            Some(c) => c,
                            None => self.range_max(fid, h, block)?,
                        };
                        parent.low + h
                    }
                };
                let new_low = parent.low + low;
                let new_high = high.min(parent.high);
                if new_low >= new_high {
                    return Some(ByteRange { low: new_low, high: new_low });
                }
                Some(ByteRange { low: new_low, high: new_high })
            }
            InstrData::IndexAddr(ia) => {
                let parent = self.recursive_byte_range((fid, ia.x))?;
                if let Some(c) = self.constant_int64(fid, ia.index) {
                    let start = parent.low + c;
                    return Some(ByteRange { low: start, high: start + 1 });
                }
                let Value::Instr(iid) = v.1 else { return None };
                let block = self.block_of((fid, iid))?;
                let res = self.range_analyzer(fid).resolve_range(ia.index, block);
                if res.is_range_check && res.min_value_set && res.max_value_set {
                    let min = res.min_value as i64;
                    let max = res.max_value as i64;
                    if min > max {
                        return Some(parent);
                    }
                    return Some(ByteRange { low: parent.low + min, high: parent.low + max + 1 });
                }
                None
            }
            InstrData::UnOp(u) if u.op == Token::MUL => self.recursive_byte_range((fid, u.x)),
            _ => None,
        }
    }

    /// A non-constant slice bound resolved through a range check: its maximum.
    fn range_max(&mut self, fid: FuncId, v: Value, block: BlockId) -> Option<i64> {
        let res = self.range_analyzer(fid).resolve_range(v, block);
        (res.is_range_check && res.max_value_set).then_some(res.max_value as i64)
    }
}

/// The arguments go/ssa would hand the callee: guff passes a variadic tail
/// unpacked, where go/ssa packs it into one slice — so a tail argument is
/// never the callee's parameter.
fn packed_args(prog: &Program, func: &Function, call: &CallCommon) -> Vec<Value> {
    let n = crate::gosec_g118::unpacked_arg_count(prog, func, call);
    call.args[..n].to_vec()
}

/// `mergeRanges`.
fn merge_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    if ranges.len() <= 1 {
        return ranges;
    }
    ranges.sort_by_key(|r| r.low);
    let mut out: Vec<ByteRange> = vec![ranges[0]];
    for r in &ranges[1..] {
        let last = out.last_mut().unwrap();
        if r.low <= last.high {
            last.high = last.high.max(r.high);
        } else {
            out.push(*r);
        }
    }
    out
}

/// `subtractRange`.
fn subtract_range(safe: &[ByteRange], taint: ByteRange) -> Vec<ByteRange> {
    let mut out = Vec::new();
    for &r in safe {
        if r.high <= taint.low || r.low >= taint.high {
            out.push(r);
            continue;
        }
        if r.low < taint.low {
            out.push(ByteRange { low: r.low, high: taint.low });
        }
        if r.high > taint.high {
            out.push(ByteRange { low: taint.high, high: r.high });
        }
    }
    out
}

/// The package path of a static callee (`fn.Pkg.Pkg.Path()`), recovering it
/// from the type-checker object for a function guff created on demand.
fn static_callee_pkg(prog: &Program, call: &CallCommon) -> Option<String> {
    if call.method.is_some() {
        return None;
    }
    let Value::Function(f) = call.value else { return None };
    func_pkg_path(prog, f)
}

fn func_pkg_path(prog: &Program, f: FuncId) -> Option<String> {
    let func = prog.functions.get(f);
    if let Some(pkg) = func.pkg {
        return Some(prog.package_arena.get(prog.packages.get(pkg).type_pkg()).path().to_string());
    }
    let p = func.object?.pkg(&prog.object_arena)?;
    Some(prog.package_arena.get(p).path().to_string())
}

/// `fn.Pkg.Pkg.Path() + "." + fn.Name()` (just the name without a package).
fn pkg_qualified_name(prog: &Program, f: FuncId) -> String {
    let name = &prog.functions.get(f).name;
    match func_pkg_path(prog, f) {
        Some(p) => format!("{p}.{name}"),
        None => name.clone(),
    }
}

/// go/ssa `Function.String()` (`RelString(nil)`): `pkg.F`, `(pkg.T).M`,
/// `pkg.F$1`.
fn full_name(prog: &Program, f: FuncId) -> String {
    let func = prog.functions.get(f);
    if let Some(parent) = func.parent {
        let pname = &prog.functions.get(parent).name;
        let suffix = func.name.strip_prefix(pname.as_str()).unwrap_or(&func.name);
        return format!("{}{suffix}", full_name(prog, parent));
    }
    let recv = func
        .signature
        .and_then(|s| guff_types::signature::signature_recv(&prog.type_arena, s))
        .and_then(|r| r.typ(&prog.object_arena));
    if let Some(recv) = recv {
        let t = type_string(&prog.type_arena, &prog.object_arena, &prog.package_arena, recv, None);
        return format!("({t}).{}", func.name);
    }
    pkg_qualified_name(prog, f)
}

/// `types.Func.FullName()` for an interface method: `(crypto/cipher.AEAD).Seal`.
fn method_full_name(prog: &Program, m: guff_types::ObjectId) -> String {
    let name = m.name(&prog.object_arena).to_string();
    let recv = m
        .typ(&prog.object_arena)
        .and_then(|s| guff_types::signature::signature_recv(&prog.type_arena, s))
        .and_then(|r| r.typ(&prog.object_arena));
    match recv {
        Some(t) => format!(
            "({}).{name}",
            type_string(&prog.type_arena, &prog.object_arena, &prog.package_arena, t, None)
        ),
        None => name,
    }
}
