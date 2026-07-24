//! Port of [`github.com/ykadowak/zerologlint`](https://github.com/ykadowak/zerologlint).
//!
//! Detects `zerolog.Event` values from `github.com/rs/zerolog/log` or
//! `zerolog.Logger` receivers that are never dispatched with `Msg` / `Send`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff_analysis::callcheck::static_callee;
use guff_analysis::passes::buildir;
use guff_analysis::{AnalysisResult, Analyzer, Diagnostic, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::ids::{FuncId, InstrId};
use guff_ssa::instr::{CallCommon, InstrData};
use guff_ssa::program::{value_type_of, Program};
use guff_ssa::value::Value;
use guff_types::signature::signature_recv;
use guff_types::typestring::type_string;

const MSG: &str = "must be dispatched by Msg or Send method";

type EventKey = (FuncId, InstrId);

struct Linter {
    event_set: HashSet<EventKey>,
    delete_later: HashSet<EventKey>,
    rec_limit: u32,
}

impl Linter {
    fn new() -> Self {
        Self {
            event_set: HashSet::new(),
            delete_later: HashSet::new(),
            rec_limit: 100,
        }
    }

    fn inspect_call(
        &mut self,
        prog: &Program,
        func: &Function,
        fid: FuncId,
        iid: InstrId,
        common: &CallCommon,
    ) {
        if is_in_log_pkg(prog, common) || is_logger_recv(prog, common) {
            if is_zerolog_event(prog, func, common.value) {
                self.event_set.insert((fid, iid));
                return;
            }
        }

        let Some(callee) = static_callee(common) else {
            return;
        };
        let callee_fn = prog.functions.get(callee);
        if !is_dispatch_method(&callee_fn.name) {
            let mut should_return = true;
            for (pid, _) in callee_fn.params.iter() {
                if is_zerolog_event(prog, callee_fn, Value::Param(pid)) {
                    for (_, block) in callee_fn.live_blocks() {
                        for &inner in &block.instrs {
                            match callee_fn.instrs.get(inner) {
                                InstrData::Call(c) => {
                                    if inspect_dispatch_in_function(prog, callee_fn, &c.call) {
                                        should_return = false;
                                        break;
                                    }
                                }
                                InstrData::Defer(d) => {
                                    if inspect_dispatch_in_function(prog, callee_fn, &d.call) {
                                        should_return = false;
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if !should_return {
                            break;
                        }
                    }
                }
            }
            if should_return {
                return;
            }
        }

        for &arg in &common.args {
            if !is_zerolog_event(prog, func, arg) {
                continue;
            }
            if let Value::Instr(phi_id) = arg {
                if matches!(func.instrs.get(phi_id), InstrData::Phi(_)) {
                    if let InstrData::Phi(phi) = func.instrs.get(phi_id) {
                        for edge in &phi.edges {
                            if let Some(v) = edge {
                                self.dfs_edge(prog, func, fid, *v, &mut HashSet::new(), 0);
                            }
                        }
                    }
                    continue;
                }
            }
            if let Some(root) = root_key(prog, func, fid, arg) {
                self.event_set.remove(&root);
            }
        }
    }

    fn dfs_edge(
        &mut self,
        prog: &Program,
        func: &Function,
        fid: FuncId,
        v: Value,
        visit: &mut HashSet<Value>,
        cnt: u32,
    ) {
        if cnt > self.rec_limit || visit.contains(&v) {
            return;
        }
        visit.insert(v);
        let Some(root) = root_value(prog, func, v) else {
            return;
        };
        if let Value::Instr(phi_id) = root {
            if let InstrData::Phi(phi) = func.instrs.get(phi_id) {
                for edge in &phi.edges {
                    if let Some(ev) = edge {
                        self.dfs_edge(prog, func, fid, *ev, visit, cnt + 1);
                    }
                }
                return;
            }
        }
        if let Some(key) = root_key(prog, func, fid, root) {
            self.delete_later.insert(key);
        }
    }
}

fn root_key(prog: &Program, func: &Function, fid: FuncId, v: Value) -> Option<EventKey> {
    match root_value(prog, func, v)? {
        Value::Instr(id) => Some((fid, id)),
        _ => None,
    }
}

fn root_value(prog: &Program, func: &Function, v: Value) -> Option<Value> {
    let Value::Instr(call_id) = v else {
        return Some(v);
    };
    let InstrData::Call(call) = func.instrs.get(call_id) else {
        return Some(v);
    };
    if call.call.args.is_empty() {
        return Some(Value::Instr(call_id));
    }
    let root = call.call.args[0];
    if !is_zerolog_event(prog, func, root) {
        return Some(Value::Instr(call_id));
    }
    root_value(prog, func, root)
}

fn inspect_dispatch_in_function(prog: &Program, func: &Function, common: &CallCommon) -> bool {
    let Some(callee) = static_callee(common) else {
        return false;
    };
    if !is_dispatch_method(&prog.functions.get(callee).name) {
        return false;
    }
    common
        .args
        .iter()
        .any(|&arg| is_zerolog_event(prog, func, arg))
}

fn func_pkg_path(prog: &Program, fid: FuncId) -> Option<String> {
    let f = prog.functions.get(fid);
    let pkg = f.pkg?;
    let ssa_pkg = prog.packages.get(pkg);
    Some(
        prog.package_arena
            .get(ssa_pkg.pkg)
            .path()
            .to_string(),
    )
}

fn is_in_log_pkg(prog: &Program, common: &CallCommon) -> bool {
    match common.value {
        Value::Function(fid) => func_pkg_path(prog, fid)
            .is_some_and(|p| p.ends_with("github.com/rs/zerolog/log")),
        Value::Global(gid) => {
            let g = prog.globals.get(gid);
            let ssa_pkg = prog.packages.get(g.pkg);
            prog.package_arena
                .get(ssa_pkg.pkg)
                .path()
                .ends_with("github.com/rs/zerolog/log")
        }
        _ => false,
    }
}

fn is_logger_recv(prog: &Program, common: &CallCommon) -> bool {
    let Some(callee) = static_callee(common) else {
        return false;
    };
    let func = prog.functions.get(callee);
    let Some(sig) = func.signature else {
        return false;
    };
    let Some(recv) = signature_recv(&prog.type_arena, sig) else {
        return false;
    };
    let Some(recv_typ) = recv.typ(&prog.object_arena) else {
        return false;
    };
    let ts = type_string(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        recv_typ,
        None,
    );
    ts.ends_with("zerolog.Logger")
}

fn is_zerolog_event(prog: &Program, func: &Function, v: Value) -> bool {
    let typ = match v {
        Value::Instr(iid) => {
            let Some(t) = func.instrs.get(iid).result_type() else {
                return false;
            };
            t
        }
        _ => value_type_of(prog, func, v),
    };
    let ts = type_string(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        typ,
        None,
    );
    ts.ends_with("github.com/rs/zerolog.Event")
}

fn is_dispatch_method(name: &str) -> bool {
    matches!(name, "Send" | "Msg" | "Msgf" | "MsgFunc")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "zerologlint requires buildir analyzer".to_string())?;
        let mut linter = Linter::new();
        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            for (_, block) in func.live_blocks() {
                for &iid in &block.instrs {
                    match func.instrs.get(iid) {
                        InstrData::Call(c) => {
                            linter.inspect_call(&ir.prog, func, fid, iid, &c.call)
                        }
                        InstrData::Defer(d) => {
                            linter.inspect_call(&ir.prog, func, fid, iid, &d.call)
                        }
                        _ => {}
                    }
                }
            }
        }
        for key in linter.delete_later {
            linter.event_set.remove(&key);
        }
        for (fid, iid) in linter.event_set {
            let func = ir.prog.functions.get(fid);
            let pos = func.pos(iid);
            if pos.is_valid() {
                reports.push(pos.0 as u32);
            }
        }
    }
    for pos in reports {
        pass.report(Diagnostic {
            pos,
            message: MSG.into(),
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

/// The `zerologlint` analyzer.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "zerologlint",
        doc: "Detects the wrong usage of `zerolog` that a user forgets to dispatch with `Send` or `Msg`",
        url: "https://github.com/ykadowak/zerologlint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    })
}
