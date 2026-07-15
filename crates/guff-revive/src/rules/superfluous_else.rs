//! `superfluous-else` — drop else after if-control-flow.

use guff_analysis::Pass;

use crate::failure::Failure;
use crate::ifelse;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    ifelse::apply_superfluous_else(pass)
}
