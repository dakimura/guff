//! SA1015 — `time.Tick` used in a way that leaks memory.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1015`.

use std::sync::OnceLock;

use guff_analysis::code::{is_in_test_at, is_main_like, stdlib_version, version_compare};
use guff_analysis::passes::buildir::{self, BuildIrResult};
use guff_analysis::{each_call, is_call_to as ssa_is_call_to, terminates};
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::function::Function;

const MSG: &str = "using time.Tick leaks the underlying ticker, consider using it only in endless functions, tests and the main package, and use time.NewTicker here";

fn func_pos(pass: &Pass<'_>, func: &Function) -> u32 {
    if let Some(obj) = func.object {
        if let Some(artifacts) = pass.pkg().type_artifacts.as_ref() {
            if let guff_types::arena::ObjectData::Func(_) = artifacts.objects.get(obj) {
                return obj.pos(&artifacts.objects);
            }
        }
    }
    func.instr_pos
        .values()
        .find(|p| p.0 > 0)
        .map(|p| p.0 as u32)
        .unwrap_or(0)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let ir = pass
        .result_of::<BuildIrResult>(buildir::analyzer())
        .ok_or_else(|| "SA1015 requires buildir analyzer".to_string())?;

    let mut pending = Vec::new();
    for &fid in &ir.src_funcs {
        let func = ir.prog.functions.get(fid);
        let fpos = func_pos(pass, func);
        if fpos == 0 || version_compare(&stdlib_version(pass, fpos), "go1.23") >= 0 {
            continue;
        }
        if is_main_like(pass) || is_in_test_at(pass, fpos) {
            continue;
        }
        if !terminates(func, &ir.prog) {
            continue;
        }
        each_call(func, &ir.prog, |_, f, iid, call, _| {
            if !ssa_is_call_to(&ir.prog, call, "time.Tick") {
                return;
            }
            let pos = f.pos(iid).0 as u32;
            if pos != 0 {
                pending.push(pos);
            }
        });
    }
    for pos in pending {
        pass.reportf(pos, MSG);
    }
    Ok(None)
}

fn sa1015_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1015",
        doc: "using time.Tick in a way that will leak",
        url: "https://staticcheck.dev/docs/checks/#SA1015",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1015 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1015_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1015_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
