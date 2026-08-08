//! SA1025 — incorrect use of `(*time.Timer).Reset`'s return value.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1025`.

use std::sync::OnceLock;

use guff::token::Token;
use guff_analysis::filter_debug;
use guff_analysis::is_call_to;
use guff_analysis::passes::buildir;
use guff_analysis::referrers;
use guff_analysis::walk_dominated;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_ssa::ids::BlockId;
use guff_ssa::instr::{Call, If, InstrData};
use guff_ssa::value::Value;

const MSG: &str =
    "it is not possible to use Reset's return value correctly, as there is a race condition between draining the channel and the new timer expiring";

fn branch_drains_timer_c(func: &guff_ssa::function::Function, if_block: BlockId, if_instr: &If) -> bool {
    let block = func.blocks.get(if_block);
    let if_idx = block.instrs.iter().position(|&i| {
        matches!(func.instrs.get(i), InstrData::If(i) if i.cond == if_instr.cond)
    });
    let Some(if_pos) = if_idx else {
        return false;
    };
    if if_pos + 2 >= block.succs.len() {
        // If is followed by Jump to branches; use succs from if block.
    }
    let mut found = false;
    for &succ in &block.succs {
        if func.blocks.get(succ).preds.len() != 1 {
            continue;
        }
        walk_dominated(func, succ, if_block, |_, b| {
            for &iid in &b.instrs {
                if let InstrData::UnOp(uop) = func.instrs.get(iid) {
                    if uop.op == Token::ARROW {
                        found = true;
                        return false;
                    }
                }
            }
            true
        });
    }
    found
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let pending = {
        let ir = pass
            .result_of::<buildir::BuildIrResult>(buildir::analyzer())
            .ok_or_else(|| "SA1025 requires buildir analyzer".to_string())?;

        let mut pending = Vec::new();
        for &fid in &ir.src_funcs {
            let func = ir.prog.functions.get(fid);
            for (bid, block) in func.live_blocks() {
                let instrs = filter_debug(&block.instrs, func);
                for &iid in &instrs {
                    let InstrData::Call(Call { call, .. }) = func.instrs.get(iid) else {
                        continue;
                    };
                    if !is_call_to(&ir.prog, call, "(*time.Timer).Reset") {
                        continue;
                    }
                    let reset_val = Value::Instr(iid);
                    for &ref_id in referrers(func, reset_val) {
                        let InstrData::If(iff) = func.instrs.get(ref_id) else {
                            continue;
                        };
                        if branch_drains_timer_c(func, bid, iff) {
                            pending.push((func.pos(iid).0 as u32, MSG.to_string()));
                        }
                    }
                }
            }
        }
        pending
    };
    // Remap only if we have findings: `call_node_starts` walks the AST.
    let call_starts = (!pending.is_empty())
        .then(|| guff_analysis::call_node_starts(pass))
        .unwrap_or_default();
    for (pos, msg) in pending {
        pass.reportf(call_starts.get(&pos).copied().unwrap_or(pos), msg);
    }
    Ok(None)
}

fn sa1025_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1025",
        doc: "it is not possible to use (*time.Timer).Reset's return value correctly",
        url: "https://staticcheck.dev/docs/checks/#SA1025",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![buildir::analyzer()],
        fact_types: vec![],
    }
}

/// SA1025 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1025_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1025_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
