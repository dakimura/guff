//! Meta-formatter that chains enabled formatters (golangci `MetaFormatter`).

use crate::gofmt::{Gofmt, GofmtOptions};
use crate::runner::FormatError;
use crate::Formatter;

/// Known formatter names (golangci-lint v2 `formatters.enable`).
pub const KNOWN_FORMATTERS: &[&str] = &["gci", "gofmt", "gofumpt", "goimports", "golines", "swaggo"];

pub fn is_formatter(name: &str) -> bool {
    KNOWN_FORMATTERS.contains(&name)
}

/// Chains enabled formatters. When none are enabled, falls back to plain `gofmt`
/// (golangci falls back to `go/format.Source`).
pub struct MetaFormatter {
    formatters: Vec<Box<dyn Formatter>>,
}

impl MetaFormatter {
    /// Build from `formatters.enable` and gofmt options.
    ///
    /// Unknown / not-yet-implemented names return an error (golangci rejects
    /// invalid names). Known-but-unimplemented formatters (gofumpt, …) return
    /// a clear DEFERRED message.
    pub fn new(enable: &[String], gofmt: GofmtOptions) -> Result<Self, FormatError> {
        for name in enable {
            if !is_formatter(name) {
                return Err(FormatError::InvalidFormatter(name.clone()));
            }
        }

        let mut formatters: Vec<Box<dyn Formatter>> = Vec::new();

        if enable.iter().any(|n| n == "gofmt") {
            formatters.push(Box::new(Gofmt::new(gofmt.clone())));
        }

        // DEFERRED: gofumpt / goimports / swaggo / gci / golines (R15 follow-up).
        for name in enable {
            match name.as_str() {
                "gofmt" => {}
                "gofumpt" | "goimports" | "gci" | "golines" | "swaggo" => {
                    return Err(FormatError::Deferred(name.clone()));
                }
                _ => {}
            }
        }

        if formatters.is_empty() {
            // golangci: empty enable → go/format.Source; we use gofmt without -s.
            formatters.push(Box::new(Gofmt::new(GofmtOptions::default())));
        }

        Ok(Self { formatters })
    }

    pub fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let mut data = src.to_vec();
        for formatter in &self.formatters {
            match formatter.format(filename, &data) {
                Ok(next) => data = next,
                Err(e) => {
                    // Match golangci: log warning and keep previous bytes.
                    // Callers that want hard failure can use individual formatters.
                    eprintln!("guff fmt: ({}) formatting {filename}: {e}", formatter.name());
                    continue;
                }
            }
        }
        Ok(data)
    }

    pub fn formatter_names(&self) -> Vec<&str> {
        self.formatters.iter().map(|f| f.name()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_enable_defaults_to_gofmt() {
        let m = MetaFormatter::new(&[], GofmtOptions::default()).unwrap();
        assert_eq!(m.formatter_names(), vec!["gofmt"]);
    }

    #[test]
    fn rejects_unknown() {
        let err = match MetaFormatter::new(&["not-a-fmt".into()], GofmtOptions::default()) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(matches!(err, FormatError::InvalidFormatter(_)));
    }

    #[test]
    fn deferred_gofumpt() {
        let err = match MetaFormatter::new(&["gofumpt".into()], GofmtOptions::default()) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(matches!(err, FormatError::Deferred(_)));
    }
}
