//! SSA Basic Block.

use crate::ids::{BlockId, FuncId, InstrId};
use crate::dom::DomInfo;
use crate::arena::ArenaId;

/// BasicBlock represents an SSA basic block.
/// (Go: `BasicBlock`)
pub struct BasicBlock {
    /// index of this block within Parent().Blocks
    pub index: i32,
    /// optional label; no semantic significance
    pub comment: String,
    /// parent function
    pub parent: FuncId,
    /// instructions in this block
    pub instrs: Vec<InstrId>,
    /// predecessor blocks
    pub preds: Vec<BlockId>,
    /// successor blocks
    pub succs: Vec<BlockId>,
    /// dominance information
    pub dom: DomInfo,
    /// whether this block is deleted (transient during blockopt)
    pub deleted: bool,
}

impl BasicBlock {
    pub fn new(index: i32, parent: FuncId) -> Self {
        Self {
            index,
            comment: String::new(),
            parent,
            instrs: Vec::new(),
            preds: Vec::new(),
            succs: Vec::new(),
            dom: DomInfo::new(),
            deleted: false,
        }
    }

    pub fn idom(&self) -> Option<BlockId> {
        self.dom.idom
    }

    pub fn dominees(&self) -> &[BlockId] {
        &self.dom.children
    }

    pub fn dominates(&self, other: &BasicBlock) -> bool {
        self.dom.pre <= other.dom.pre && other.dom.post <= self.dom.post
    }

    /// pred_index returns the index of block b in self.preds.
    pub fn pred_index(&self, b: BlockId) -> usize {
        for (i, &p) in self.preds.iter().enumerate() {
            if p == b {
                return i;
            }
        }
        panic!("block {} is not a predecessor of {}", b.index(), self.index);
    }

    pub fn remove_pred(&mut self, p: BlockId) {
        self.preds.retain(|&x| x != p);
    }

    pub fn replace_succ(&mut self, old: BlockId, new: BlockId) {
        for s in self.succs.iter_mut() {
            if *s == old {
                *s = new;
            }
        }
    }

    pub fn replace_pred(&mut self, old: BlockId, new: BlockId) {
        for p in self.preds.iter_mut() {
            if *p == old {
                *p = new;
            }
        }
    }

    pub fn has_phi(&self, f: &crate::function::Function) -> bool {
        for &id in &self.instrs {
            if let crate::instr::InstrData::Phi(_) = f.instrs.get(id) {
                return true;
            } else {
                break; // phis always come first
            }
        }
        false
    }
}
