//! SSA Builder — labelled break/continue/goto.
//!
//! Port of go/ssa's `func.go` (`lblock`, `labelledBlock`, `targetedBlock`).

use crate::builder::Builder;
use crate::function::LBlock;
use crate::ids::{BlockId, FuncId};
use guff::token::Token;

impl<'a> Builder<'a> {
    /// Returns the branch target associated with `name`, creating it if needed.
    /// (Go: `Function.lblockOf`.)
    pub(crate) fn lblock_of(&mut self, name: &str) -> BlockId {
        let fid = self.func_id;
        if let Some(lb) = self.prog.functions.get(fid).lblocks.get(name) {
            return lb.goto_;
        }
        let goto_ = self.new_basic_block(format!("{name}.label"));
        self.prog.functions.get_mut(fid).lblocks.insert(
            name.to_string(),
            LBlock {
                name: name.to_string(),
                resolved: false,
                goto_,
                break_: None,
                continue_: None,
            },
        );
        goto_
    }

    /// Records `break` / `continue` targets on the labelled statement `name`.
    pub(crate) fn set_label_loop_targets(
        &mut self,
        name: &str,
        break_: BlockId,
        continue_: BlockId,
    ) {
        if let Some(lb) = self.prog.functions.get_mut(self.func_id).lblocks.get_mut(name) {
            lb.break_ = Some(break_);
            lb.continue_ = Some(continue_);
        }
    }

    /// Searches for the block associated with a labelled `break` / `continue`.
    /// (Go: `labelledBlock`, without yield-function ancestor search.)
    pub(crate) fn labelled_block(&self, name: &str, tok: Token) -> Option<BlockId> {
        if let Some(block) = self.labelled_block_in(self.func_id, name, tok) {
            return Some(block);
        }
        // Search ancestors if this is a yield function.
        if self.func().jump_var.is_some() {
            let mut fid = self.func_id;
            while let Some(parent) = self.prog.functions.get(fid).parent {
                if let Some(block) = self.labelled_block_in(parent, name, tok) {
                    return Some(block);
                }
                fid = parent;
                if self.prog.functions.get(fid).jump_var.is_none() {
                    break;
                }
            }
        }
        None
    }

    fn labelled_block_in(&self, fid: FuncId, name: &str, tok: Token) -> Option<BlockId> {
        let lb = self.prog.functions.get(fid).lblocks.get(name)?;
        match tok {
            Token::BREAK => lb.break_,
            Token::CONTINUE => lb.continue_,
            Token::GOTO => Some(lb.goto_),
            _ => None,
        }
    }

    /// Returns the nearest unlabelled `break` / `continue` target.
    /// (Go: `targetedBlock`, without yield-function ancestor search.)
    pub(crate) fn targeted_block(&self, tok: Token) -> Option<BlockId> {
        for tgts in self.targets.iter().rev() {
            let block = match tok {
                Token::BREAK => Some(tgts.break_),
                Token::CONTINUE => Some(tgts.continue_),
                _ => None,
            };
            if block.is_some() {
                return block;
            }
        }
        // Yield functions inherit break targets from the parent function.
        if self.func().jump_var.is_some() {
            if let Some(parent) = self.func().parent {
                return self.targeted_block_in(parent, tok);
            }
        }
        None
    }

    fn targeted_block_in(&self, fid: FuncId, tok: Token) -> Option<BlockId> {
        // During range_func the parent's break target is recorded on its lblock
        // or we walk the parent's saved targets via the enclosing range label.
        // The parent Builder pushed (done, loop_) before building the yield fn;
        // mirror that by reading the parent's innermost labelled loop break.
        let f = self.prog.functions.get(fid);
        for lb in f.lblocks.values() {
            if let Some(b) = lb.break_ {
                if tok == Token::BREAK {
                    return Some(b);
                }
            }
        }
        None
    }
}
