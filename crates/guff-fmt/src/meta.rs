//! Meta-formatter that chains enabled formatters (golangci `MetaFormatter`).

use crate::gci::{Gci, GciOptions};
use crate::gofmt::{Gofmt, GofmtOptions};
use crate::gofumpt::{Gofumpt, GofumptOptions};
use crate::goimports::{Goimports, GoimportsOptions};
use crate::golines::{Golines, GolinesOptions};
use crate::runner::FormatError;
use crate::swaggo::Swaggo;
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
    /// Build from `formatters.enable` and per-formatter options.
    ///
    /// Unknown names return an error (golangci rejects invalid names).
    /// Formatters are chained in `enable` order.
    pub fn new(
        enable: &[String],
        gofmt: GofmtOptions,
        gofumpt: GofumptOptions,
        goimports: GoimportsOptions,
        gci: GciOptions,
        golines: GolinesOptions,
    ) -> Result<Self, FormatError> {
        for name in enable {
            if !is_formatter(name) {
                return Err(FormatError::InvalidFormatter(name.clone()));
            }
        }

        let mut formatters: Vec<Box<dyn Formatter>> = Vec::new();

        for name in enable {
            match name.as_str() {
                "gofmt" => formatters.push(Box::new(Gofmt::new(gofmt.clone()))),
                "gofumpt" => formatters.push(Box::new(Gofumpt::new(gofumpt.clone()))),
                "goimports" => formatters.push(Box::new(Goimports::new(goimports.clone()))),
                "gci" => formatters.push(Box::new(Gci::new(gci.clone()))),
                "golines" => formatters.push(Box::new(Golines::new(golines.clone()))),
                "swaggo" => formatters.push(Box::new(Swaggo::new())),
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

    /// Primary formatter name when this meta wraps exactly one formatter.
    pub fn primary_name(&self) -> Option<&str> {
        (self.formatters.len() == 1).then(|| self.formatters[0].name())
    }

    /// Options fingerprint for the sole formatter (format-check cache key).
    pub fn options_fingerprint(&self) -> Option<String> {
        (self.formatters.len() == 1).then(|| self.formatters[0].options_fingerprint())
    }

    /// Fast check-mode pre-filter. If this meta wraps exactly one formatter with
    /// a batch list mode, return the files it flags as unformatted (as the
    /// tool's own echoed paths) in a single invocation. `None` → the caller
    /// falls back to per-file checks.
    ///
    /// Only single-formatter metas qualify: `guff run` builds one meta per
    /// enabled formatter for check mode, so each formatter's diagnostics stay
    /// independently attributable. Chained multi-formatter metas (fix mode) do
    /// not use this path.
    pub fn batch_list_unformatted(
        &self,
        files: &[std::path::PathBuf],
    ) -> Option<Vec<std::path::PathBuf>> {
        if self.formatters.len() != 1 {
            return None;
        }
        let refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();
        self.formatters[0].list_unformatted(&refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> (
        GofmtOptions,
        GofumptOptions,
        GoimportsOptions,
        GciOptions,
        GolinesOptions,
    ) {
        (
            GofmtOptions::default(),
            GofumptOptions::default(),
            GoimportsOptions::default(),
            GciOptions::default(),
            GolinesOptions::default(),
        )
    }

    #[test]
    fn empty_enable_defaults_to_gofmt() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&[], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["gofmt"]);
    }

    #[test]
    fn rejects_unknown() {
        let (a, b, c, d, e) = opts();
        let err = match MetaFormatter::new(&["not-a-fmt".into()], a, b, c, d, e) {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        assert!(matches!(err, FormatError::InvalidFormatter(_)));
    }

    #[test]
    fn enables_gofumpt() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&["gofumpt".into()], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["gofumpt"]);
    }

    #[test]
    fn enables_goimports() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&["goimports".into()], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["goimports"]);
    }

    #[test]
    fn enables_gci() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&["gci".into()], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["gci"]);
    }

    #[test]
    fn enables_golines() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&["golines".into()], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["golines"]);
    }

    #[test]
    fn enables_swaggo() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(&["swaggo".into()], a, b, c, d, e).unwrap();
        assert_eq!(m.formatter_names(), vec!["swaggo"]);
    }

    #[test]
    fn chains_in_enable_order() {
        let (a, b, c, d, e) = opts();
        let m = MetaFormatter::new(
            &[
                "gofmt".into(),
                "gofumpt".into(),
                "goimports".into(),
                "gci".into(),
                "golines".into(),
            ],
            a,
            b,
            c,
            d,
            e,
        )
        .unwrap();
        assert_eq!(
            m.formatter_names(),
            vec!["gofmt", "gofumpt", "goimports", "gci", "golines"]
        );
    }
}
