//! `gofmt` formatter — shells out to the Go toolchain `gofmt` binary.
//!
//! Matches golangci-lint `pkg/goformatters/gofmt` settings:
//! - `simplify` → `-s`
//! - `rewrite-rules` → repeated `-r 'pattern -> replacement'`

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "gofmt";

/// One gofmt rewrite rule (`-r`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RewriteRule {
    pub pattern: String,
    pub replacement: String,
}

/// Options for [`Gofmt`] (`formatters.settings.gofmt`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GofmtOptions {
    /// Pass `-s` (simplify code).
    pub simplify: bool,
    /// Pass `-r` for each rule.
    pub rewrite_rules: Vec<RewriteRule>,
}

/// Formatter that invokes the system `gofmt` binary.
#[derive(Debug, Clone, Default)]
pub struct Gofmt {
    options: GofmtOptions,
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Gofmt {
    pub fn new(options: GofmtOptions) -> Self {
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

impl Formatter for Gofmt {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("gofmt");
        let mut cmd = Command::new(bin);
        if self.options.simplify {
            cmd.arg("-s");
        }
        for rule in &self.options.rewrite_rules {
            cmd.arg("-r").arg(format!("{} -> {}", rule.pattern, rule.replacement));
        }
        // gofmt reads stdin when no path args are given.
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })?;

        {
            let mut stdin = child.stdin.take().ok_or_else(|| FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: "failed to open gofmt stdin".into(),
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
                message: format!("gofmt failed: {}", stderr.trim()),
            });
        }

        Ok(output.stdout)
    }

    fn list_unformatted(&self, files: &[&Path]) -> Option<Vec<PathBuf>> {
        let bin = self.binary.as_deref().unwrap_or("gofmt");
        crate::runner::batch_list(files, || {
            let mut c = Command::new(bin);
            c.arg("-l");
            if self.options.simplify {
                c.arg("-s");
            }
            for rule in &self.options.rewrite_rules {
                c.arg("-r")
                    .arg(format!("{} -> {}", rule.pattern, rule.replacement));
            }
            c
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_badly_spaced_source() {
        let fmt = Gofmt::new(GofmtOptions::default());
        let src = b"package main\nfunc main(  ) {\nx:=1\n}\n";
        let out = fmt.format("main.go", src).expect("gofmt");
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("func main() {"));
        assert!(s.contains("x := 1"));
    }

    #[test]
    fn simplify_collapses_slice() {
        let fmt = Gofmt::new(GofmtOptions {
            simplify: true,
            ..Default::default()
        });
        // gofmt -s rewrites s[a:len(s)] → s[a:]
        let src = b"package p\n\nfunc f(s []int) []int {\n\treturn s[1:len(s)]\n}\n";
        let out = fmt.format("p.go", src).expect("gofmt -s");
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("s[1:]"),
            "expected simplify rewrite, got:\n{s}"
        );
    }
}
