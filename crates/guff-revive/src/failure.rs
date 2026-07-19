//! A single revive rule violation.

/// One lint failure produced by a revive rule.
#[derive(Debug, Clone)]
pub struct Failure {
    pub rule: &'static str,
    pub pos: u32,
    pub message: String,
    /// Optional per-failure confidence override (e.g. error-strings capitalization).
    pub confidence: Option<f64>,
}

impl Failure {
    /// Build a failure with the default confidence for `rule` (see [`Self::confidence`]).
    pub fn new(rule: &'static str, pos: u32, message: impl Into<String>) -> Self {
        Self {
            rule,
            pos,
            message: message.into(),
            confidence: None,
        }
    }

    /// Build a failure with an explicit confidence (e.g. error-strings capitalization = 0.6).
    pub fn with_confidence(
        rule: &'static str,
        pos: u32,
        message: impl Into<String>,
        confidence: f64,
    ) -> Self {
        Self {
            rule,
            pos,
            message: message.into(),
            confidence: Some(confidence),
        }
    }

    /// Confidence level for this failure (revive default for most rules: 1.0).
    pub fn confidence(&self) -> f64 {
        if let Some(c) = self.confidence {
            return c;
        }
        match self.rule {
            "exported" if self.message.contains("stutters")
                || self.message.contains("is repetitive")
                || self.message.contains("should be of the form") =>
            {
                0.8
            }
            "var-declaration" if self.message.contains("omit type") => 0.8,
            "var-declaration" if self.message.contains("zero value") => 0.9,
            _ => 1.0,
        }
    }

    pub fn format(&self) -> String {
        format!("{}: {}", self.rule, self.message)
    }
}
