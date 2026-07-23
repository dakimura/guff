//! `gofmt` formatter — native Rust port by default (PERF_TASKS Task 1b).
//!
//! Matches golangci-lint `pkg/goformatters/gofmt` settings:
//! - `simplify` → `-s` (still subprocess / unimplemented natively)
//! - `rewrite-rules` → repeated `-r 'pattern -> replacement'` (subprocess)
//!
//! Native path (no subprocess) is used when there are no rewrite rules and
//! either simplify is off, or `GUFF_NATIVE_FMT=0` is not forcing subprocess.
//! Set `GUFF_NATIVE_FMT=0` to force the system `gofmt` binary for all cases.
//! Set `GUFF_NATIVE_FMT=1` to prefer native even when simplify is on (simplify
//! is then ignored until Task 1b `-s` lands).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::native::{self, NativeOptions};
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

/// Formatter: native `go/format` port, with subprocess fallback.
#[derive(Debug, Clone, Default)]
pub struct Gofmt {
    options: GofmtOptions,
    /// Override binary path (tests / non-standard installs / subprocess path).
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

    /// Whether to use the in-process native implementation.
    fn use_native(&self) -> bool {
        // Explicit force-off.
        if std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "0") {
            return false;
        }
        // Rewrite rules have no native port yet.
        if !self.options.rewrite_rules.is_empty() {
            return false;
        }
        // simplify (-s) not ported yet — keep subprocess unless forced on.
        if self.options.simplify {
            return std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "1");
        }
        // Default: native (harness-proven on prometheus + GOROOT).
        true
    }
}

impl Formatter for Gofmt {
    fn name(&self) -> &str {
        NAME
    }

    fn options_fingerprint(&self) -> String {
        let rules: String = self
            .options
            .rewrite_rules
            .iter()
            .map(|r| format!("{}->{}", r.pattern, r.replacement))
            .collect::<Vec<_>>()
            .join(";");
        crate::fingerprint_parts(&[
            ("simplify", if self.options.simplify { "1" } else { "0" }),
            ("rules", &rules),
            ("native", if self.use_native() { "1" } else { "0" }),
        ])
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        if self.use_native() {
            let opts = NativeOptions {
                simplify: self.options.simplify,
                filename: filename.to_string(),
                ..Default::default()
            };
            return native::gofmt::format(src, &opts).map_err(|e| match e {
                FormatError::Message {
                    formatter: _,
                    path,
                    message,
                } => FormatError::Message {
                    formatter: NAME.to_string(),
                    path,
                    message,
                },
                other => other,
            });
        }

        let bin = self.binary.as_deref().unwrap_or("gofmt");
        let mut cmd = Command::new(bin);
        if self.options.simplify {
            cmd.arg("-s");
        }
        for rule in &self.options.rewrite_rules {
            cmd.arg("-r")
                .arg(format!("{} -> {}", rule.pattern, rule.replacement));
        }
        // gofmt reads stdin when no path args are given.
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

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
        // Native path: return None so the runner's per-file `check_file` path
        // formats each file once. A prior `native_list` pre-pass would format
        // the whole tree and then re-format every flagged file in `check_file`.
        if self.use_native() {
            return None;
        }
        // Subprocess prefilter (`GUFF_NATIVE_FMT=0`): system `gofmt -l`.
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
        // -s still uses the system binary (native simplify not ported).
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

    #[test]
    fn native_is_default_without_simplify() {
        // Ensure GUFF_NATIVE_FMT=0 is not set for this assertion.
        std::env::remove_var("GUFF_NATIVE_FMT");
        let fmt = Gofmt::new(GofmtOptions::default());
        assert!(fmt.use_native());
    }
}
