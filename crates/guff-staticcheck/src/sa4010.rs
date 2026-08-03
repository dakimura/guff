//! SA4010 — result of append will never be observed.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4010`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff_analysis::passes::buildir;
use guff_analysis::referrers;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_ssa::function::Function;
use guff_ssa::ids::InstrId;
use guff_ssa::instr::{Call, InstrData};
use guff_ssa::program::Program;
use guff_ssa::value::Value;

fn is_append(prog: &Program, func: &Function, iid: InstrId) -> bool {
    let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
        return false;
    };
    if call.method.is_some() {
        return false;
    }
    match call.value {
        Value::Builtin(b) => prog.builtins.get(b).name == "append",
        _ => false,
    }
}

/// True when the append result is only observed by Phi / further appends
/// (upstream `walkRefs`).
fn append_result_unused(prog: &Program, func: &Function, append_iid: InstrId) -> bool {
    let mut is_used = false;
    let mut visited = HashSet::new();
    let mut stack: Vec<InstrId> = referrers(func, Value::Instr(append_iid)).to_vec();
    while let Some(rid) = stack.pop() {
        if !visited.insert(rid) {
            continue;
        }
        match func.instrs.get(rid) {
            InstrData::DebugRef(_) => {}
            InstrData::Phi(_) => {
                stack.extend(referrers(func, Value::Instr(rid)).iter().copied());
            }
            other if other.is_value() => {
                if is_append(prog, func, rid) {
                    stack.extend(referrers(func, Value::Instr(rid)).iter().copied());
                } else {
                    is_used = true;
                    break;
                }
            }
            _ => {
                // Non-value instruction (Store, Return, …) observes the slice.
                is_used = true;
                break;
            }
        }
    }
    !is_used
}

/// Upstream `validateArgument`: slice arg DFG may only be Phi / Slice / Const /
/// MakeSlice / Alloc / append.
fn validate_argument(
    prog: &Program,
    func: &Function,
    v: Value,
    seen: &mut HashSet<Value>,
) -> bool {
    if !seen.insert(v) {
        return true;
    }
    match v {
        Value::Const(_) => true,
        Value::Instr(iid) => match func.instrs.get(iid) {
            InstrData::Phi(p) => p
                .edges
                .iter()
                .flatten()
                .all(|&e| validate_argument(prog, func, e, seen)),
            InstrData::Slice(s) => validate_argument(prog, func, s.x, seen),
            InstrData::MakeSlice(_) | InstrData::Alloc(_) => true,
            InstrData::Call(_) if is_append(prog, func, iid) => {
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    return false;
                };
                call.args
                    .first()
                    .copied()
                    .is_some_and(|a| validate_argument(prog, func, a, seen))
            }
            _ => false,
        },
        _ => false,
    }
}

/// Upstream `validateReferrers`: referrers of DFG values must stay inside the
/// local-allocation slice graph (no escaped aliases).
fn validate_referrers(func: &Function, v: Value, seen: &mut HashSet<InstrId>) -> bool {
    for &rid in referrers(func, v) {
        if !seen.insert(rid) {
            continue;
        }
        match func.instrs.get(rid) {
            InstrData::Phi(_)
            | InstrData::Slice(_)
            | InstrData::MakeSlice(_)
            | InstrData::Alloc(_)
            | InstrData::DebugRef(_) => {}
            _ => return false,
        }
        if func.instrs.get(rid).is_value()
            && !validate_referrers(func, Value::Instr(rid), seen)
        {
            return false;
        }
    }
    true
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
                if !append_result_unused(&ir.prog, func, iid) {
                    continue;
                }
                let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                    continue;
                };
                let Some(&arg0) = call.args.first() else {
                    continue;
                };
                let mut seen_args = HashSet::new();
                if !validate_argument(&ir.prog, func, arg0, &mut seen_args) {
                    continue;
                }
                let mut seen_refs: HashSet<InstrId> = seen_args
                    .iter()
                    .filter_map(|v| match v {
                        Value::Instr(i) => Some(*i),
                        _ => None,
                    })
                    .collect();
                seen_refs.insert(iid);
                let mut ok = true;
                for v in &seen_args {
                    if !validate_referrers(func, *v, &mut seen_refs) {
                        ok = false;
                        break;
                    }
                }
                if ok {
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
