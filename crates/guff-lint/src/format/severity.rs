//! Severity sanitizer used by checkstyle / SARIF (golangci `severitySanitizer`).

/// Maps unsupported severity strings onto a format-specific default.
#[derive(Debug, Clone)]
pub(crate) struct SeveritySanitizer {
    allowed: &'static [&'static str],
    default: &'static str,
}

impl SeveritySanitizer {
    pub(crate) const fn new(allowed: &'static [&'static str], default: &'static str) -> Self {
        Self { allowed, default }
    }

    pub(crate) fn sanitize(&self, severity: &str) -> &'static str {
        self.allowed
            .iter()
            .copied()
            .find(|&a| a == severity)
            .unwrap_or(self.default)
    }
}

/// Checkstyle allowed levels + default (`error`).
pub(crate) const CHECKSTYLE: SeveritySanitizer = SeveritySanitizer::new(
    &["ignore", "info", "warning", "error"],
    "error",
);

/// SARIF allowed levels + default (`error`).
pub(crate) const SARIF: SeveritySanitizer =
    SeveritySanitizer::new(&["none", "note", "warning", "error"], "error");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkstyle_keeps_known_falls_back_unknown() {
        assert_eq!(CHECKSTYLE.sanitize("warning"), "warning");
        assert_eq!(CHECKSTYLE.sanitize(""), "error");
        assert_eq!(CHECKSTYLE.sanitize("foo"), "error");
    }

    #[test]
    fn sarif_keeps_known_falls_back_unknown() {
        assert_eq!(SARIF.sanitize("note"), "note");
        assert_eq!(SARIF.sanitize(""), "error");
        assert_eq!(SARIF.sanitize("foo"), "error");
    }
}
