//! SA4017 — discarding return value of pure function call
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4017`.

use std::sync::OnceLock;

use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};


use guff_analysis::passes::buildir;
use guff_analysis::{has_non_debug_referrer, is_call_to_any, referrers, short_call_name};
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::value::Value;

const PURE_FUNCS: &[&str] = &[
    "strings.ToLower", "strings.ToUpper", "strings.TrimSpace",
    "strconv.Itoa", "strconv.FormatInt",
];

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<buildir::BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA4017 requires buildir analyzer".to_string())?;
    let mut pending = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        for (_, block) in func.live_blocks() {
            for &iid in &block.instrs {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else { continue };
                let val = Value::Instr(iid);
                if has_non_debug_referrer(referrers(func, val), func) {
                    continue;
                }
                if !is_call_to_any(&ir.prog, call, PURE_FUNCS) {
                    continue;
                }
                let name = short_call_name(&ir.prog, call).unwrap_or_default();
                pending.push((
                    func.pos(iid).0 as u32,
                    format!("{name} doesn't have side effects and its return value is ignored"),
                ));
            }
        }
    }
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}


fn sa4017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4017",
        doc: "discarding return value of pure function call",
        url: "https://staticcheck.dev/docs/checks/#SA4017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
