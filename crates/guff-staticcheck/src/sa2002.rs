//! SA2002 — `testing.T.FailNow` (and similar) called inside a goroutine.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa2002`.

use std::sync::OnceLock;

use guff_analysis::callcheck;
use guff_analysis::passes::buildir;
use guff_analysis::{closure_fn_in, short_call_name, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::function::Function;
use guff_ssa::instr::{Call, Go, InstrData};
use guff_ssa::program::Program;
use guff_types::arena::TypeData;
use guff_types::pointer::pointer_elem;
use guff_types::signature::signature_recv;
use guff_types::typestring::type_string;

const TESTING_FATAL_METHODS: &[&str] = &[
    "FailNow", "Fatal", "Fatalf", "SkipNow", "Skip", "Skipf",
];

fn is_testing_common_method(prog: &Program, common: &guff_ssa::instr::CallCommon) -> Option<String> {
    if common.method.is_some() {
        return None;
    }
    let target = callcheck::resolve_call_target(common, prog)?;
    let name = short_call_name(prog, common)?;
    if !TESTING_FATAL_METHODS.iter().any(|&m| m == name) {
        return None;
    }
    let sig = target.typ(&prog.object_arena)?;
    let recv = signature_recv(&prog.type_arena, sig)?;
    let recv_typ = recv.typ(&prog.object_arena)?;
    if !is_pointer_to_testing_recv(prog, recv_typ) {
        return None;
    }
    let _ = target;
    Some(name)
}

fn is_pointer_to_testing_recv(prog: &Program, typ: guff_types::TypeId) -> bool {
    let underlying = typ.underlying(&prog.type_arena);
    let TypeData::Pointer(_) = prog.type_arena.get(underlying) else {
        return false;
    };
    let elem = pointer_elem(&prog.type_arena, underlying);
    let rendered = type_string(
        &prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        elem,
        None,
    );
    rendered == "testing.common" || rendered == "testing.T" || rendered == "testing.B"
}

fn check_goroutine_fn(
    prog: &Program,
    caller: &Function,
    go_call: &guff_ssa::instr::CallCommon,
) -> Option<String> {
    let callee = closure_fn_in(caller, go_call.value)?;
    let goroutine = prog.functions.get(callee);
    if goroutine.blocks.is_empty() {
        return None;
    }
    for (_, block) in goroutine.live_blocks() {
        for &iid in &block.instrs {
            let InstrData::Call(Call { call, .. }) = goroutine.instrs.get(iid) else {
                continue;
            };
            if let Some(name) = is_testing_common_method(prog, call) {
                return Some(name);
            }
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA2002 requires buildir analyzer".to_string())?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Go(Go { call, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(name) = check_goroutine_fn(&ir.prog, func, call) else {
                    continue;
                };
                pending.push((
                    func.pos(iid).0 as u32,
                    format!(
                        "the goroutine calls T.{name}, which must be called in the same goroutine as the test"
                    ),
                ));
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa2002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA2002",
        doc: "testing.T.FailNow or SkipNow called inside a goroutine",
        url: "https://staticcheck.dev/docs/checks/#SA2002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA2002 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa2002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa2002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
