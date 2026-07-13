//! SSA IR Invariant Checker.
//!
//! Port of go/ssa's `sanity.go`.

use crate::function::Function;
use crate::arena::ArenaId;
use crate::program::Program;

/// sanity_check performs internal consistency checks on the SSA IR.
pub fn sanity_check(prog: &Program) {
    for (_, f) in prog.functions.iter() {
        sanity_check_function(f);
    }
}

pub fn sanity_check_function(f: &Function) {
    for (id, block) in f.blocks.iter() {
        if block.deleted {
            continue;
        }
        // CFG consistency: for every succ, we must be in their preds.
        for &succ_id in &block.succs {
            let succ = f.blocks.get(succ_id);
            if succ.deleted {
                continue;
            }
            if !succ.preds.contains(&id) {
                panic!("block {} is successor of {}, but {} is not in its preds", succ_id.index(), id.index(), id.index());
            }
        }

        // CFG consistency: for every pred, we must be in their succs.
        for &pred_id in &block.preds {
            let pred = f.blocks.get(pred_id);
            if pred.deleted {
                continue;
            }
            if !pred.succs.contains(&id) {
                panic!("block {} is predecessor of {}, but {} is not in its succs", pred_id.index(), id.index(), id.index());
            }
        }

        for &instr_id in &block.instrs {
            if !f.instrs.contains(instr_id) {
                panic!("instruction {} in block {} not found in function arena", instr_id.index(), id.index());
            }
        }
    }
}
