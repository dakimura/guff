//! SA4010 — result of append will never be observed.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4010` (simplified).

use std::sync::OnceLock;

use guff_analysis::passes::buildir;
use guff_analysis::{has_non_debug_referrer, referrers};
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::value::Value;

fn is_append(prog: &guff_ssa::program::Program, func: &guff_ssa::function::Function, iid: guff_ssa::ids::InstrId) -> bool {
    let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
        return false;
    };
    match call.value {
        Value::Builtin(b) => prog.builtins.get(b).name == "append",
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4010 requires buildir analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                if !is_append(&ir.prog, func, iid) {
                    continue;
                }
                let val = Value::Instr(iid);
                if !has_non_debug_referrer(referrers(func, val), func) {
                    pending.push((
                        func.pos(iid).0 as u32,
                        "this result of append is never used, except maybe in other appends".into(),
                    ));
                }
            }
        }
    }
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4010",
        doc: "the result of append will never be observed anywhere",
        url: "https://staticcheck.dev/docs/checks/#SA4010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
