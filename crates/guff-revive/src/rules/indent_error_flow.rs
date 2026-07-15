//! `indent-error-flow` — drop else after if-return.

use guff_analysis::Pass;

use crate::failure::Failure;
use crate::ifelse;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    ifelse::apply_indent_error_flow(pass)
}
