//! SA5000 — assignment to nil map.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5000`.

use std::sync::OnceLock;

use guff_analysis::passes::buildir;
use guff_analysis::{is_nil_const, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::instr::InstrData;

const MSG: &str = "assignment to nil map";

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut reports = Vec::new();
    {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "SA5000 requires buildir analyzer".to_string())?;

        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            for (_, block) in func.blocks.iter() {
                for &iid in &block.instrs {
                    let InstrData::MapUpdate(mu) = func.instrs.get(iid) else {
                        continue;
                    };
                    if is_nil_const(&ir.prog, func, mu.map) {
                        reports.push(func.pos(iid).0 as u32);
                    }
                }
            }
        }
    }
    for pos in reports {
        pass.reportf(pos, MSG);
    }
    Ok(None)
}

fn sa5000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5000",
        doc: "assignment to nil map",
        url: "https://staticcheck.dev/docs/checks/#SA5000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
