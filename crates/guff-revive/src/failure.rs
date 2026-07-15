//! A single revive rule violation.

/// One lint failure produced by a revive rule.
#[derive(Debug, Clone)]
pub struct Failure {
    pub rule: &'static str,
    pub pos: u32,
    pub message: String,
}

impl Failure {
    /// Confidence level for this failure (revive default for most rules: 1.0).
    pub fn confidence(&self) -> f64 {
        match self.rule {
            "exported" if self.message.contains("stutters")
                || self.message.contains("is repetitive")
                || self.message.contains("should be of the form") =>
            {
                0.8
            }
            _ => 1.0,
        }
    }

    pub fn format(&self) -> String {
        format!("{}: {}", self.rule, self.message)
    }
}
