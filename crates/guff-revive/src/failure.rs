//! A single revive rule violation.

/// One lint failure produced by a revive rule.
#[derive(Debug, Clone)]
pub struct Failure {
    pub rule: &'static str,
    pub pos: u32,
    pub message: String,
}

impl Failure {
    pub fn format(&self) -> String {
        format!("{}: {}", self.rule, self.message)
    }
}
