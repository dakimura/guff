//! SSA Builder — Conditions.
//!
//! Port of go/ssa's `builder.go` (cond part).

use crate::builder::Builder;
use crate::ids::BlockId;
use guff::ast::{Expr, BinaryExpr};
use guff::token::Token;

impl<'a> Builder<'a> {
    /// cond translates a boolean expression to control flow.
    /// (Go: `builder.cond`)
    pub fn cond(&mut self, e: &Expr, t: BlockId, f: BlockId) {
        match e {
            Expr::BinaryExpr(bin) => match bin.op {
                Token::LAND => {
                    let middle = self.new_basic_block("cond.and".to_string());
                    self.cond(&bin.x, middle, f);
                    self.set_block(Some(middle));
                    self.cond(&bin.y, t, f);
                    return;
                }
                Token::LOR => {
                    let middle = self.new_basic_block("cond.or".to_string());
                    self.cond(&bin.x, t, middle);
                    self.set_block(Some(middle));
                    self.cond(&bin.y, t, f);
                    return;
                }
                _ => {}
            },
            Expr::ParenExpr(p) => {
                self.cond(&p.x, t, f);
                return;
            }
            _ => {}
        }

        let cond = self.expr(e);
        self.emit_if(cond, t, f);
    }
}
