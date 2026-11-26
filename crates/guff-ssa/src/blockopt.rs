//! SSA block optimizations.
//!
//! Port of go/ssa's `blockopt.go`.

use crate::function::Function;
use crate::ids::BlockId;
use crate::instr::InstrData;
use crate::arena::ArenaId;

/// optimize_blocks performs some simple block optimizations on a completed
/// function: dead block elimination, block fusion, jump threading.
/// (Go: `optimizeBlocks`)
pub fn optimize_blocks(f: &mut Function) {
    delete_unreachable_blocks(f);

    let mut changed = true;
    while changed {
        changed = false;

        let block_ids: Vec<_> = f.blocks.iter().map(|(id, _)| id).collect();
        for id in block_ids {
            if f.blocks.get(id).deleted {
                continue;
            }

            if fuse_blocks(f, id) {
                changed = true;
            }

            if jump_threading(f, id) {
                changed = true;
                continue;
            }
        }
    }

    remove_deleted_blocks(f);
}

fn delete_unreachable_blocks(f: &mut Function) {
    if f.blocks.is_empty() {
        return;
    }

    // Mark reachable blocks
    let mut reachable = crate::hash::HashSet::default();
    let root_id = f.blocks.iter().next().map(|(id, _)| id).unwrap();
    mark_reachable(f, root_id, &mut reachable);
    
    // TODO: f.Recover

    let block_ids: Vec<_> = f.blocks.iter().map(|(id, _)| id).collect();
    for id in block_ids {
        if !reachable.contains(&id) {
            // Delete unreachable block
            let succs = f.blocks.get(id).succs.clone();
            for succ_id in succs {
                if reachable.contains(&succ_id) {
                    f.blocks.get_mut(succ_id).remove_pred(id);
                }
            }
            f.blocks.get_mut(id).deleted = true;
        }
    }
}

fn mark_reachable(f: &Function, id: BlockId, reachable: &mut crate::hash::HashSet<BlockId>) {
    if reachable.insert(id) {
        for &succ_id in &f.blocks.get(id).succs {
            mark_reachable(f, succ_id, reachable);
        }
    }
}

fn fuse_blocks(f: &mut Function, a_id: BlockId) -> bool {
    let a = f.blocks.get(a_id);
    if a.succs.len() != 1 {
        return false;
    }
    let b_id = a.succs[0];
    let b = f.blocks.get(b_id);
    if b.preds.len() != 1 || b_id == a_id {
        return false;
    }

    if b.has_phi(f) {
        return false;
    }

    // Eliminate jump at end of A, then copy all of B across.
    let b_instrs = b.instrs.clone();
    let a = f.blocks.get_mut(a_id);
    if !a.instrs.is_empty() {
        a.instrs.pop(); // Remove Jump
    }
    a.instrs.extend(b_instrs);

    // A inherits B's successors
    let b_succs = f.blocks.get(b_id).succs.clone();
    f.blocks.get_mut(a_id).succs = b_succs.clone();

    // Fix up Preds links of all successors of B.
    for c_id in b_succs {
        f.blocks.get_mut(c_id).replace_pred(b_id, a_id);
    }

    f.blocks.get_mut(b_id).deleted = true;
    true
}

fn jump_threading(f: &mut Function, b_id: BlockId) -> bool {
    if b_id.index() == 0 {
        return false;
    }
    let b = f.blocks.get(b_id);
    if b.instrs.len() != 1 {
        return false;
    }
    if !matches!(f.instrs.get(b.instrs[0]), InstrData::Jump(_)) {
        return false;
    }
    let c_id = b.succs[0];
    if c_id == b_id {
        return false;
    }
    if f.blocks.get(c_id).has_phi(f) {
        return false;
    }

    let b_preds = b.preds.clone();
    for (i, &a_id) in b_preds.iter().enumerate() {
        f.blocks.get_mut(a_id).replace_succ(b_id, c_id);
        
        let a = f.blocks.get(a_id);
        if a.succs.len() == 2 && a.succs[0] == c_id && a.succs[1] == c_id {
            // Replace degenerate If by Jump
            f.blocks.get_mut(a_id).instrs.pop();
            f.blocks.get_mut(a_id).instrs.push(f.instrs.alloc(InstrData::Jump(crate::instr::Jump {})));
            f.blocks.get_mut(a_id).succs.pop();
            f.blocks.get_mut(c_id).remove_pred(b_id);
        } else {
            if i == 0 {
                f.blocks.get_mut(c_id).replace_pred(b_id, a_id);
            } else {
                f.blocks.get_mut(c_id).preds.push(a_id);
            }
        }
    }

    f.blocks.get_mut(b_id).deleted = true;
    true
}

fn remove_deleted_blocks(f: &mut Function) {
    // We keep BlockIds (arena indices) stable so that succs/preds/dom links
    // remain valid, but renumber the semantic `index` field of the surviving
    // blocks so that it matches their position in the compacted block list,
    // exactly as go/ssa's optimizeBlocks does. This is what the disassembler
    // and golden comparison rely on.
    let ids: Vec<BlockId> = f.blocks.iter().map(|(id, _)| id).collect();
    let mut next = 0i32;
    for id in ids {
        let b = f.blocks.get_mut(id);
        if b.deleted {
            continue;
        }
        b.index = next;
        next += 1;
    }
}
