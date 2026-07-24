//! SA2003 — deferred `Lock` right after locking.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa2003`.

use std::sync::OnceLock;

use guff_analysis::passes::buildir;
use guff_analysis::{filter_debug, is_call_to_any, short_call_name, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{Call, Defer, InstrData};

const LOCK_CALLS: &[&str] = &["(*sync.Mutex).Lock", "(*sync.RWMutex).RLock"];

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA2003 requires buildir analyzer".to_string())?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            let instrs = filter_debug(&block.instrs, func);
            if instrs.len() < 2 {
                continue;
            }
            for i in 0..instrs.len() - 1 {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(instrs[i]) else {
                    continue;
                };
                if !is_call_to_any(&ir.prog, call, LOCK_CALLS) {
                    continue;
                }
                let InstrData::Defer(Defer { call: defer_call }) = func.instrs.get(instrs[i + 1])
                else {
                    continue;
                };
                if !is_call_to_any(&ir.prog, defer_call, LOCK_CALLS) {
                    continue;
                }
                let Some(lock_recv) = call.args.first() else {
                    continue;
                };
                if defer_call.args.first() != Some(lock_recv) {
                    continue;
                }
                let name = short_call_name(&ir.prog, call).unwrap_or_default();
                let alt = match name.as_str() {
                    "Lock" => "Unlock",
                    "RLock" => "RUnlock",
                    _ => continue,
                };
                pending.push((
                    func.pos(instrs[i + 1]).0 as u32,
                    format!(
                        "deferring {name} right after having locked already; did you mean to defer {alt}?"
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

fn sa2003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA2003",
        doc: "deferred Lock right after locking",
        url: "https://staticcheck.dev/docs/checks/#SA2003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA2003 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa2003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa2003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
