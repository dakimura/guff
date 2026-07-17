//! `gofumpt` formatter — shells out to the `gofumpt` binary (`mvdan.cc/gofumpt`).
//!
//! Matches golangci-lint `pkg/goformatters/gofumpt` settings:
//! - `extra-rules` → `-extra`
//! - `module-path` → `-modpath`
//! - target Go version → `-lang` (golangci sources this from `run.go`; the CLI
//!   wires it from config `run.go`, else gofumpt reads `go.mod`).

use std::io::Write;
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "gofumpt";

/// Options for [`Gofumpt`] (`formatters.settings.gofumpt`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GofumptOptions {
    /// Pass `-extra` (stricter rules that need human review).
    pub extra_rules: bool,
    /// Pass `-modpath` (module path containing the source).
    pub module_path: Option<String>,
    /// Pass `-lang` (target Go version, e.g. `go1.22` / `1.22`).
    /// `None` → gofumpt reads the version from `go.mod`.
    pub lang: Option<String>,
}

/// Formatter that invokes the system `gofumpt` binary.
#[derive(Debug, Clone, Default)]
pub struct Gofumpt {
    options: GofumptOptions,
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Gofumpt {
    pub fn new(options: GofumptOptions) -> Self {
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

impl Formatter for Gofumpt {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("gofumpt");
        let mut cmd = Command::new(bin);
        if self.options.extra_rules {
            cmd.arg("-extra");
        }
        if let Some(modpath) = &self.options.module_path {
            if !modpath.is_empty() {
                cmd.arg("-modpath").arg(modpath);
            }
        }
        if let Some(lang) = &self.options.lang {
            if !lang.is_empty() {
                cmd.arg("-lang").arg(normalize_lang(lang));
            }
        }
        // gofumpt reads stdin when no path args are given.
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
                message: "failed to open gofumpt stdin".into(),
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
                message: format!("gofumpt failed: {}", stderr.trim()),
            });
        }

        Ok(output.stdout)
    }
}

/// gofumpt expects `-lang go1.X`; accept bare `1.22` / `go1.22` / `1` forms.
fn normalize_lang(lang: &str) -> String {
    let t = lang.trim();
    if t.starts_with("go") {
        t.to_string()
    } else {
        format!("go{t}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lang_prefixes_go() {
        assert_eq!(normalize_lang("1.22"), "go1.22");
        assert_eq!(normalize_lang("go1.21"), "go1.21");
        assert_eq!(normalize_lang(" 1.20 "), "go1.20");
    }

    fn gofumpt_available() -> bool {
        Command::new("gofumpt")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn removes_empty_line_at_block_start() {
        if !gofumpt_available() {
            eprintln!("skip: gofumpt not on PATH");
            return;
        }
        let fmt = Gofumpt::new(GofumptOptions::default());
        let src = b"package p\n\nfunc f() {\n\n\tx := 1\n\tprintln(x)\n}\n";
        let out = fmt.format("p.go", src).expect("gofumpt");
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("func f() {\n\tx := 1"),
            "expected empty line removed, got:\n{s}"
        );
    }

    #[test]
    fn extra_rules_clothes_naked_return() {
        if !gofumpt_available() {
            eprintln!("skip: gofumpt not on PATH");
            return;
        }
        let fmt = Gofumpt::new(GofumptOptions {
            extra_rules: true,
            ..Default::default()
        });
        let src = b"package p\n\nfunc f() (x int) {\n\tx = 1\n\treturn\n}\n";
        let out = fmt.format("p.go", src).expect("gofumpt -extra");
        let s = String::from_utf8(out).unwrap();
        assert!(
            s.contains("return x"),
            "expected clothed return, got:\n{s}"
        );
    }
}
