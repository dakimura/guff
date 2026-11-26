//! `early-return` — reduce nesting by inverting if conditions.

use guff_analysis::Pass;

use crate::failure::Failure;
use crate::ifelse;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    ifelse::apply_early_return(pass)
}
