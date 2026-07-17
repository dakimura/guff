//! `swaggo` formatter — formats swaggo/swag annotation comments.
//!
//! golangci-lint embeds a hard fork of the swag formatter
//! (`github.com/golangci/swaggoswag`) which reformats swag comments in memory.
//! guff shells out to the swaggo CLI `swag fmt`, which uses the same formatter.
//!
//! `swag fmt` rewrites `.go` files in a directory in place, so we stage `src`
//! into a private temp directory, run `swag fmt -d <dir>`, and read the file
//! back. No settings are exposed (golangci: "No settings available.").

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "swaggo";

/// Formatter that invokes the system `swag` binary (`swag fmt`).
#[derive(Debug, Clone, Default)]
pub struct Swaggo {
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Swaggo {
    pub fn new() -> Self {
        Self { binary: None }
    }

    pub fn with_binary(mut self, path: impl Into<String>) -> Self {
        self.binary = Some(path.into());
        self
    }
}

impl Formatter for Swaggo {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("swag");
        let dir = stage_dir(filename, src)?;
        let _guard = DirGuard(dir.0.clone());
        let staged = dir.1;

        let mut cmd = Command::new(bin);
        cmd.arg("fmt").arg("-d").arg(&dir.0);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(FormatError::Message {
                formatter: NAME.to_string(),
                path: filename.to_string(),
                message: format!("swag fmt failed: {}", stderr.trim()),
            });
        }

        fs::read(&staged).map_err(|e| FormatError::Io {
            formatter: NAME.to_string(),
            path: filename.to_string(),
            source: e,
        })
    }
}

/// Create a private temp dir containing the source as a `.go` file.
/// Returns `(dir, staged_file_path)`.
fn stage_dir(filename: &str, src: &[u8]) -> Result<(PathBuf, PathBuf), FormatError> {
    let base = std::env::temp_dir();
    let pid = std::process::id();
    for n in 0..1000 {
        let dir = base.join(format!(".guff-swaggo-{pid}-{n}"));
        match fs::create_dir(&dir) {
            Ok(()) => {
                let name = go_file_name(filename);
                let staged = dir.join(name);
                fs::write(&staged, src).map_err(|e| FormatError::Io {
                    formatter: NAME.to_string(),
                    path: filename.to_string(),
                    source: e,
                })?;
                return Ok((dir, staged));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(FormatError::Io {
                    formatter: NAME.to_string(),
                    path: filename.to_string(),
                    source: e,
                })
            }
        }
    }
    Err(FormatError::Message {
        formatter: NAME.to_string(),
        path: filename.to_string(),
        message: "failed to create temp dir for swag fmt".into(),
    })
}

fn go_file_name(filename: &str) -> String {
    if filename != "<standard input>" && !filename.is_empty() {
        if let Some(name) = std::path::Path::new(filename).file_name() {
            let n = name.to_string_lossy();
            if n.ends_with(".go") {
                return n.to_string();
            }
        }
    }
    "input.go".to_string()
}

struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn swag_available() -> bool {
        Command::new("swag")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn go_file_name_defaults() {
        assert_eq!(go_file_name("<standard input>"), "input.go");
        assert_eq!(go_file_name("/a/b/api.go"), "api.go");
        assert_eq!(go_file_name(""), "input.go");
    }

    #[test]
    fn formats_swag_comments() {
        if !swag_available() {
            eprintln!("skip: swag not on PATH");
            return;
        }
        // Misaligned swag annotations should be realigned by `swag fmt`.
        let src = b"package main\n\n// @Summary   Add a new pet\n// @Description  add\n// @Success 200\nfunc handler() {}\n";
        let out = Swaggo::new().format("api.go", src).expect("swag fmt");
        assert!(!out.is_empty());
        assert!(String::from_utf8_lossy(&out).contains("@Summary"));
    }
}
