//! SA5007 — infinite recursive call.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5007`.

use std::sync::OnceLock;

use guff_analysis::passes::buildir;
use guff_analysis::{dominates_all_returns, each_call, is_call_to, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::ids::BlockId;
use guff_ssa::instr::InstrData;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "SA5007 requires buildir analyzer".to_string())?;

        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            each_call(func, &ir.prog, |bid, caller, iid, _call, callee| {
                let Some(callee) = callee else {
                    return;
                };
                if callee != fid {
                    return;
                }
                if matches!(caller.instrs.get(iid), InstrData::Go(_)) {
                    return;
                }
                if dominates_all_returns(caller, bid) {
                    reports.push(caller.pos(iid).0 as u32);
                }
            });
        }
    }
    for pos in reports {
        pass.reportf(pos, "infinite recursive call");
    }
    Ok(None)
}

fn sa5007_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5007",
        doc: "infinite recursive call",
        url: "https://staticcheck.dev/docs/checks/#SA5007",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5007_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5007_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
