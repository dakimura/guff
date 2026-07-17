//! `goimports` formatter — shells out to the `goimports` binary (`golang.org/x/tools/cmd/goimports`).
//!
//! Matches golangci-lint `pkg/goformatters/goimports` settings:
//! - `local-prefixes` → `-local` (comma-separated)

use std::io::Write;
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "goimports";

/// Options for [`Goimports`] (`formatters.settings.goimports`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoimportsOptions {
    /// Pass `-local` (comma-joined prefixes for the third import group).
    pub local_prefixes: Vec<String>,
}

/// Formatter that invokes the system `goimports` binary.
#[derive(Debug, Clone, Default)]
pub struct Goimports {
    options: GoimportsOptions,
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Goimports {
    pub fn new(options: GoimportsOptions) -> Self {
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

impl Formatter for Goimports {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("goimports");
        let mut cmd = Command::new(bin);
        let locals: Vec<&str> = self
            .options
            .local_prefixes
            .iter()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        if !locals.is_empty() {
            cmd.arg("-local").arg(locals.join(","));
        }
        // When a real path is known, pass `-srcdir` so goimports can resolve
        // the module context for import fixes (golangci library does the same).
        if filename != "<standard input>" && !filename.is_empty() {
            cmd.arg("-srcdir").arg(filename);
        }
        // goimports reads stdin when no path args are given.
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
                message: "failed to open goimports stdin".into(),
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
                message: format!("goimports failed: {}", stderr.trim()),
            });
        }

        Ok(output.stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goimports_available() -> bool {
        Command::new("goimports")
            .arg("-h")
            .output()
            .map(|o| o.status.success() || !o.stderr.is_empty())
            .unwrap_or(false)
    }

    #[test]
    fn sorts_stdlib_before_third_party() {
        if !goimports_available() {
            eprintln!("skip: goimports not on PATH");
            return;
        }
        let fmt = Goimports::new(GoimportsOptions::default());
        let src = b"package p\n\nimport (\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n}\n";
        let out = fmt.format("p.go", src).expect("goimports");
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").expect("fmt import");
        let bar_pos = s.find("\"github.com/foo/bar\"").expect("bar import");
        assert!(
            fmt_pos < bar_pos,
            "expected stdlib before third-party, got:\n{s}"
        );
    }

    #[test]
    fn local_prefixes_groups_project_imports() {
        if !goimports_available() {
            eprintln!("skip: goimports not on PATH");
            return;
        }
        let fmt = Goimports::new(GoimportsOptions {
            local_prefixes: vec!["github.com/org/project".into()],
        });
        let src = b"package p\n\nimport (\n\t\"github.com/org/project/pkg\"\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n\t_ = pkg.Y\n}\n";
        let out = fmt.format("p.go", src).expect("goimports -local");
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").expect("fmt");
        let bar_pos = s.find("\"github.com/foo/bar\"").expect("bar");
        let pkg_pos = s.find("\"github.com/org/project/pkg\"").expect("pkg");
        assert!(
            fmt_pos < bar_pos && bar_pos < pkg_pos,
            "expected stdlib < third-party < local, got:\n{s}"
        );
    }
}
