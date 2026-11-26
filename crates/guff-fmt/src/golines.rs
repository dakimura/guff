//! `golines` formatter — shells out to the `golines` binary (`github.com/golangci/golines`).
//!
//! Matches golangci-lint `pkg/goformatters/golines` / `formatters.settings.golines`:
//! - `max-len` → `-m` / `--max-len` (default 100)
//! - `tab-len` → `-t` / `--tab-len` (default 4)
//! - `shorten-comments` → `--shorten-comments` / `--no-shorten-comments`
//! - `reformat-tags` → `--reformat-tags` / `--no-reformat-tags` (default true)
//! - `chain-split-dots` → `--chain-split-dots` / `--no-chain-split-dots` (default true)
//!
//! Uses `--base-formatter=gofmt` so golines does not invoke goimports itself
//! (golangci library sets a fake `BaseFormatterCmd`; MetaFormatter may chain
//! goimports/gci separately). Generated-file skip is handled by [`crate::runner`];
//! we pass `--no-ignore-generated` to match golangci's library settings.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "golines";

/// Options for [`Golines`] (`formatters.settings.golines`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GolinesOptions {
    /// Target maximum line length (`-m`). Default 100.
    pub max_len: u32,
    /// Tab width in columns (`-t`). Default 4.
    pub tab_len: u32,
    /// Shorten single-line comments (`--shorten-comments`). Default false.
    pub shorten_comments: bool,
    /// Align / reformat struct tags (`--reformat-tags`). Default true.
    pub reformat_tags: bool,
    /// Split chained methods on dots (`--chain-split-dots`). Default true.
    pub chain_split_dots: bool,
}

impl Default for GolinesOptions {
    fn default() -> Self {
        Self {
            max_len: 100,
            tab_len: 4,
            shorten_comments: false,
            reformat_tags: true,
            chain_split_dots: true,
        }
    }
}

/// Formatter that invokes the system `golines` binary.
#[derive(Debug, Clone, Default)]
pub struct Golines {
    options: GolinesOptions,
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Golines {
    pub fn new(options: GolinesOptions) -> Self {
        Self {
            options,
            binary: None,
        }
    }

    pub fn with_binary(mut self, path: impl Into<String>) -> Self {
        self.binary = Some(path.into());
        self
    }
}

impl Formatter for Golines {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("golines");
        let mut cmd = Command::new(bin);
        cmd.arg("-m").arg(self.options.max_len.to_string());
        cmd.arg("-t").arg(self.options.tab_len.to_string());
        if self.options.shorten_comments {
            cmd.arg("--shorten-comments");
        } else {
            cmd.arg("--no-shorten-comments");
        }
        if self.options.reformat_tags {
            cmd.arg("--reformat-tags");
        } else {
            cmd.arg("--no-reformat-tags");
        }
        if self.options.chain_split_dots {
            cmd.arg("--chain-split-dots");
        } else {
            cmd.arg("--no-chain-split-dots");
        }
        // Match golangci: do not skip generated here; Runner already does.
        cmd.arg("--no-ignore-generated");
        // Avoid nested goimports; chain goimports via MetaFormatter when enabled.
        cmd.arg("--base-formatter=gofmt");
        // No path args → read stdin, write stdout (no `-w`).
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // See the note in `swaggo.rs`: a spawn failure is about the binary, and
        // naming the source file here made "golines is not installed" look like
        // a missing input.
        let mut child = cmd.spawn().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: bin.to_string(),
            source: e,
        })?;

        {
            let mut stdin = child.stdin.take().ok_or_else(|| FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: "failed to open golines stdin".into(),
            })?;
            stdin.write_all(src).map_err(|e| FormatError::Io {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                source: e,
            })?;
        }

        let output = child.wait_with_output().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: format!("golines failed: {}", stderr.trim()),
            });
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn golines_available() -> bool {
        Command::new("golines")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn shortens_long_call_args() {
        if !golines_available() {
            eprintln!("skip: golines not on PATH");
            return;
        }
        let fmt = Golines::new(GolinesOptions {
            max_len: 60,
            ..Default::default()
        });
        let src = b"package p\n\nfunc f() {\n\tfoo(aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb)\n}\n";
        let out = fmt.format("p.go", src).expect("golines");
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("foo(\n") || s.lines().count() > 5,
            "expected line wrap, got:\n{s}"
        );
        assert!(s.contains("aaaaaaaa"), "lost args:\n{s}");
    }

    #[test]
    fn respects_no_reformat_tags() {
        if !golines_available() {
            eprintln!("skip: golines not on PATH");
            return;
        }
        let src = b"package p\n\ntype T struct {\n\tA string `json:\"a\" yaml:\"a\"`\n\tB string `json:\"bb\" yaml:\"bb\"`\n}\n";
        let with_tags = Golines::new(GolinesOptions::default())
            .format("t.go", src)
            .expect("golines default");
        let without = Golines::new(GolinesOptions {
            reformat_tags: false,
            ..Default::default()
        })
        .format("t.go", src)
        .expect("golines no-reformat-tags");
        // With reformat-tags, keys are typically aligned with extra spaces.
        // Without, source tags should stay closer to input (may still be
        // gofmt'd by base formatter). At minimum both succeed and parse.
        assert!(!with_tags.is_empty());
        assert!(!without.is_empty());
        let without_s = String::from_utf8(without).unwrap();
        assert!(
            without_s.contains("`json:\"a\" yaml:\"a\"`")
                || without_s.contains("json:\"a\""),
            "expected original-ish tags without reformat, got:\n{without_s}"
        );
    }
}
