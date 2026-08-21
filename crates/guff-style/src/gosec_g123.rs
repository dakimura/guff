//! Gosec **G123** — TLS session resumption can bypass `VerifyPeerCertificate` (SSA).
//!
//! Port of securego/gosec v2.26.1 `analyzers/tls_resumption_verifypeer.go`,
//! plus `collectAnalyzerFunctions` from `analyzers/redirect_header_propagation.go`.
//!
//! The finding: a `tls.Config` sets `VerifyPeerCertificate` — a custom
//! certificate check — but neither `VerifyConnection` nor
//! `SessionTicketsDisabled: true`. A *resumed* session never presents a
//! certificate chain, so `VerifyPeerCertificate` is not called and the check is
//! silently skipped.
//!
//! The analysis is a field-store inventory, not a dataflow: every `*tls.Config`
//! reachable from the package's functions gets one record, keyed by the SSA
//! value the field addresses trace back to, and each `Store` into one of the
//! five interesting fields updates it. Two reports come out of that — the
//! direct one, and the case where a config hands off to `GetConfigForClient`
//! and the function it names returns a config with the same hole.
//!
//! The SSA program and the `SrcFuncs` list come from [`crate::gosec_ssa`],
//! which builds them once for every SSA-based gosec analyzer.

use std::collections::{HashMap, HashSet};

use guff::Pos;
use guff_analysis::callcheck::static_callee;
use guff_ssa::function::Function;
use guff_ssa::ids::{FuncId, InstrId};
use guff_ssa::instr::InstrData;
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::arena::TypeData;
use guff_types::TypeId;

use crate::gosec_g118::{call_common, resolve_value_funcs};

const TLS_PKG: &str = "crypto/tls";

pub(crate) const MSG: &str = "G123: tls.Config uses VerifyPeerCertificate while session \
     resumption may remain enabled and VerifyConnection is not set; resumed sessions can \
     bypass custom certificate checks";

/// gosec `MaxDepth`.
const MAX_DEPTH: u32 = 20;

/// A value that identifies one `*tls.Config` — SSA values are function-local,
/// so the function is part of the key.
type ConfigKey = (FuncId, Value);

#[derive(Default)]
struct TlsConfigState {
    verify_peer_set: bool,
    verify_peer_pos: Pos,
    verify_connection_set: bool,
    session_tickets_disabled_true: bool,
    get_config_for_client_set: bool,
    get_config_for_client_pos: Pos,
    get_config_for_client_fns: Vec<FuncId>,
}

/// Collects G123 out of the SSA build [`crate::gosec_ssa`] shares between the
/// gosec analyzers, appending `(pos, message)` into `pending`.
pub(crate) fn collect_g123(
    prog: &Program,
    src_funcs: &[FuncId],
    pending: &mut Vec<(u32, String)>,
) {
    let funcs = collect_analyzer_functions(prog, src_funcs);
    if funcs.is_empty() {
        return;
    }

    let mut configs: HashMap<ConfigKey, TlsConfigState> = HashMap::new();
    for &fid in &funcs {
        let func = prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Store(_) = func.instrs.get(iid) else {
                    continue;
                };
                track_tls_config_field_store(prog, fid, func, iid, &mut configs);
            }
        }
    }

    let mut issues: HashSet<Pos> = HashSet::new();

    // reportDirectTLSConfigs
    for cfg in configs.values() {
        if !cfg.verify_peer_set || cfg.verify_connection_set || cfg.session_tickets_disabled_true {
            continue;
        }
        if cfg.verify_peer_pos.is_valid() {
            issues.insert(cfg.verify_peer_pos);
        }
    }

    // reportGetConfigForClientBypassCandidates
    for cfg in configs.values() {
        if !cfg.get_config_for_client_set || cfg.session_tickets_disabled_true {
            continue;
        }
        if !returns_risky_tls_config(prog, &cfg.get_config_for_client_fns, &configs) {
            continue;
        }
        if cfg.get_config_for_client_pos.is_valid() {
            issues.insert(cfg.get_config_for_client_pos);
        }
    }

    for pos in issues {
        pending.push((pos.0 as u32, MSG.to_string()));
    }
}

/// `collectAnalyzerFunctions`: the source functions plus everything they make a
/// closure of or statically call, transitively. Cross-package callees come back
/// with no blocks, so they cost a lookup and contribute nothing.
fn collect_analyzer_functions(prog: &Program, src_funcs: &[FuncId]) -> Vec<FuncId> {
    if src_funcs.is_empty() {
        return Vec::new();
    }
    let mut seen: HashSet<FuncId> = HashSet::new();
    let mut all: Vec<FuncId> = Vec::new();
    for &fid in src_funcs {
        if seen.insert(fid) {
            all.push(fid);
        }
    }

    let mut i = 0;
    while i < all.len() {
        let fid = all[i];
        i += 1;
        let func = prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let instr = func.instrs.get(iid);
                if let InstrData::MakeClosure(mc) = instr {
                    if seen.insert(mc.fn_) {
                        all.push(mc.fn_);
                    }
                }
                if let Some(common) = call_common(instr) {
                    if let Some(callee) = static_callee(common) {
                        if seen.insert(callee) {
                            all.push(callee);
                        }
                    }
                }
            }
        }
    }

    all
}

fn track_tls_config_field_store(
    prog: &Program,
    fid: FuncId,
    func: &Function,
    store_id: InstrId,
    configs: &mut HashMap<ConfigKey, TlsConfigState>,
) {
    let InstrData::Store(store) = func.instrs.get(store_id) else {
        return;
    };
    let (addr, val) = (store.addr, store.val);

    let Value::Instr(fa_id) = addr else {
        return;
    };
    let InstrData::FieldAddr(fa) = func.instrs.get(fa_id) else {
        return;
    };
    let (base, field_index) = (fa.x, fa.field);

    let base_type = value_type_of(prog, func, base);
    if !is_tls_config_pointer_type(prog, base_type) {
        return;
    }
    let Some(field_name) = tls_config_field_name(prog, base_type, field_index) else {
        return;
    };
    let Some(root) = tls_config_root(prog, func, base, 0) else {
        return;
    };

    let cfg = configs.entry((fid, root)).or_default();
    match field_name {
        "VerifyPeerCertificate" => {
            if !is_nil_value(prog, func, val) {
                cfg.verify_peer_set = true;
                cfg.verify_peer_pos = func.pos(store_id);
            }
        }
        "VerifyConnection" => {
            if !is_nil_value(prog, func, val) {
                cfg.verify_connection_set = true;
            }
        }
        "SessionTicketsDisabled" => {
            if let Some(b) = bool_const_value(prog, val) {
                cfg.session_tickets_disabled_true = b;
            }
        }
        // gosec records `ClientSessionCache` but never reads it back; the field
        // is kept out of the port rather than carried as dead state.
        "GetConfigForClient" => {
            if is_nil_value(prog, func, val) {
                return;
            }
            cfg.get_config_for_client_set = true;
            cfg.get_config_for_client_pos = func.pos(store_id);
            let mut fns = resolve_value_funcs(prog, fid, func, val);
            let mut seen: HashSet<FuncId> = HashSet::new();
            fns.retain(|f| seen.insert(*f));
            cfg.get_config_for_client_fns = fns;
        }
        _ => {}
    }
}

/// `getConfigForClientReturnsRiskyTLSConfig`.
fn returns_risky_tls_config(
    prog: &Program,
    fns: &[FuncId],
    configs: &HashMap<ConfigKey, TlsConfigState>,
) -> bool {
    for &fid in fns {
        let func = prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Return(ret) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(&first) = ret.results.first() else {
                    continue;
                };
                let mut visited: HashSet<Value> = HashSet::new();
                for key in extract_tls_configs(prog, fid, func, first, &mut visited, 0) {
                    let Some(cfg) = configs.get(&key) else {
                        continue;
                    };
                    if cfg.verify_peer_set
                        && !cfg.verify_connection_set
                        && !cfg.session_tickets_disabled_true
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn extract_tls_configs(
    prog: &Program,
    fid: FuncId,
    func: &Function,
    v: Value,
    visited: &mut HashSet<Value>,
    depth: u32,
) -> Vec<ConfigKey> {
    if depth > MAX_DEPTH || !visited.insert(v) {
        return Vec::new();
    }

    if let Some(root) = tls_config_root(prog, func, v, 0) {
        return vec![(fid, root)];
    }

    let Value::Instr(i) = v else {
        return Vec::new();
    };
    match func.instrs.get(i) {
        InstrData::Phi(p) => {
            let edges = p.edges.clone();
            edges
                .into_iter()
                .flatten()
                .flat_map(|e| extract_tls_configs(prog, fid, func, e, visited, depth + 1))
                .collect()
        }
        InstrData::Extract(e) => {
            let tuple = e.tuple;
            extract_tls_configs(prog, fid, func, tuple, visited, depth + 1)
        }
        InstrData::ChangeType(c) => {
            let x = c.x;
            extract_tls_configs(prog, fid, func, x, visited, depth + 1)
        }
        InstrData::TypeAssert(t) => {
            let x = t.x;
            extract_tls_configs(prog, fid, func, x, visited, depth + 1)
        }
        InstrData::MakeInterface(m) => {
            let x = m.x;
            extract_tls_configs(prog, fid, func, x, visited, depth + 1)
        }
        _ => Vec::new(),
    }
}

/// `tlsConfigRoot`: the first value in the chain that *is* a `*tls.Config`.
fn tls_config_root(prog: &Program, func: &Function, v: Value, depth: u32) -> Option<Value> {
    if depth > MAX_DEPTH {
        return None;
    }
    if is_tls_config_pointer_type(prog, value_type_of(prog, func, v)) {
        return Some(v);
    }
    let Value::Instr(i) = v else {
        return None;
    };
    match func.instrs.get(i) {
        InstrData::ChangeType(c) => {
            let x = c.x;
            tls_config_root(prog, func, x, depth + 1)
        }
        InstrData::MakeInterface(m) => {
            let x = m.x;
            tls_config_root(prog, func, x, depth + 1)
        }
        InstrData::TypeAssert(t) => {
            let x = t.x;
            tls_config_root(prog, func, x, depth + 1)
        }
        InstrData::UnOp(u) => {
            let x = u.x;
            tls_config_root(prog, func, x, depth + 1)
        }
        InstrData::FieldAddr(fa) => {
            let x = fa.x;
            tls_config_root(prog, func, x, depth + 1)
        }
        // Upstream follows only the *first* edge of a phi, so a config that
        // reaches the store on one branch and something else on the other is
        // read as whichever branch the builder emitted first.
        InstrData::Phi(p) => {
            let first = p.edges.first().copied().flatten()?;
            tls_config_root(prog, func, first, depth + 1)
        }
        _ => None,
    }
}

/// `tlsConfigFieldName`: the declared name of field `index` of `*tls.Config`.
fn tls_config_field_name(prog: &Program, ptr_type: TypeId, index: usize) -> Option<&str> {
    let TypeData::Pointer(ptr) = prog.type_arena.get(ptr_type) else {
        return None;
    };
    let elem = ptr.elem();
    let TypeData::Named(n) = prog.type_arena.get(elem) else {
        return None;
    };
    let obj = n.obj();
    if obj.name(&prog.object_arena) != "Config" {
        return None;
    }
    let pkg = obj.pkg(&prog.object_arena)?;
    if prog.package_arena.get(pkg).path() != TLS_PKG {
        return None;
    }
    let TypeData::Struct(st) = prog.type_arena.get(elem.underlying(&prog.type_arena)) else {
        return None;
    };
    if index >= st.num_fields() {
        return None;
    }
    Some(st.field(index).name(&prog.object_arena))
}

/// `isTLSConfigPointerType`: exactly `*crypto/tls.Config`.
fn is_tls_config_pointer_type(prog: &Program, t: TypeId) -> bool {
    let TypeData::Pointer(ptr) = prog.type_arena.get(t) else {
        return false;
    };
    let TypeData::Named(n) = prog.type_arena.get(ptr.elem()) else {
        return false;
    };
    let obj = n.obj();
    obj.name(&prog.object_arena) == "Config"
        && obj
            .pkg(&prog.object_arena)
            .is_some_and(|pkg| prog.package_arena.get(pkg).path() == TLS_PKG)
}

/// `isNilValue`: a `Const` with no value whose type can hold `nil`.
///
/// guff keeps the `None` that go/ssa normalizes away for numeric and boolean
/// zeroes (see `guff_ssa::const_val::Const`), so the nillable test is what
/// keeps a plain `0` out.
fn is_nil_value(prog: &Program, func: &Function, v: Value) -> bool {
    let Value::Const(id) = v else {
        return false;
    };
    if prog.constants.get(id).val.is_some() {
        return false;
    }
    let t = value_type_of(prog, func, v).underlying(&prog.type_arena);
    matches!(
        prog.type_arena.get(t),
        TypeData::Pointer(_)
            | TypeData::Slice(_)
            | TypeData::Map(_)
            | TypeData::Chan(_)
            | TypeData::Signature(_)
            | TypeData::Interface(_)
    ) || matches!(prog.type_arena.get(t), TypeData::Basic(b) if b.kind() == guff_types::basic::BasicKind::UntypedNil)
}

/// `boolConstValue`.
fn bool_const_value(prog: &Program, v: Value) -> Option<bool> {
    let Value::Const(id) = v else {
        return None;
    };
    match prog.constants.get(id).val.as_ref()? {
        guff_constant::Value::Bool(b) => Some(*b),
        _ => None,
    }
}
