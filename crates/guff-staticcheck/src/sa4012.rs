//! SA4012 — comparing a value against NaN
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4012`.

use std::sync::OnceLock;

use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff_analysis::callcheck;
use guff_analysis::passes::buildir;
use guff_analysis::{filter_debug, is_call_to};
use guff_ssa::instr::{BinOp, Call, InstrData};
use guff_ssa::value::Value;

fn is_nan_call(prog: &guff_ssa::program::Program, func: &guff_ssa::function::Function, v: Value) -> bool {
    let v = callcheck::flatten_ssa_value(func, v);
    let Value::Instr(iid) = v else { return false };
    let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else { return false };
    is_call_to(prog, call, "math.NaN")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4012 requires buildir analyzer".to_string())?;
    let mut pending: Vec<(u32, String)> = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.blocks.iter() {
            for iid in filter_debug(&block.instrs, func) {
                let InstrData::BinOp(BinOp { x, y, .. }) = func.instrs.get(iid) else { continue };
                if is_nan_call(&ir.prog, func, *x) || is_nan_call(&ir.prog, func, *y) {
                    pending.push((
                        func.pos(iid).0 as u32,
                        "no value is equal to NaN, not even NaN itself".into(),
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


fn sa4012_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4012",
        doc: "comparing a value against NaN",
        url: "https://staticcheck.dev/docs/checks/#SA4012",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4012_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4012_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
