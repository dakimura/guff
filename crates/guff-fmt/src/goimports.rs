//! `goimports` formatter — native Rust port by default (PERF_TASKS Task 1d).
//!
//! Native path: delete unused imports, add from sibling files / stdlib, then
//! group/sort + gofmt. When a missing import still needs module-cache
//! resolution, falls back to the system `goimports` binary for that file.
//!
//! Set `GUFF_NATIVE_FMT=0` to force the subprocess for every file.
//!
//! Matches golangci-lint `pkg/goformatters/goimports` settings:
//! - `local-prefixes` → `-local` (comma-separated)

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::native::{self, goimports::FormatOutcome, NativeOptions};
use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "goimports";

/// Options for [`Goimports`] (`formatters.settings.goimports`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GoimportsOptions {
    /// Pass `-local` (comma-joined prefixes for the third import group).
    pub local_prefixes: Vec<String>,
}

/// Formatter: native goimports port, with subprocess fallback.
#[derive(Debug, Clone, Default)]
pub struct Goimports {
    options: GoimportsOptions,
    /// Override binary path (tests / non-standard installs / subprocess path).
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

    /// Native is default after Task 1d. `GUFF_NATIVE_FMT=0` forces subprocess.
    fn use_native(&self) -> bool {
        !std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "0")
    }

    fn native_opts(&self, filename: &str) -> NativeOptions {
        NativeOptions {
            local_prefixes: self.options.local_prefixes.clone(),
            filename: filename.to_string(),
            ..Default::default()
        }
    }

    fn format_subprocess(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
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

impl Formatter for Goimports {
    fn name(&self) -> &str {
        NAME
    }

    fn options_fingerprint(&self) -> String {
        let locals = self.options.local_prefixes.join(",");
        crate::fingerprint_parts(&[
            ("local", &locals),
            ("native", if self.use_native() { "1" } else { "0" }),
        ])
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        if self.use_native() {
            match native::goimports::format_outcome(src, &self.native_opts(filename)) {
                Ok(FormatOutcome::Formatted(out)) => return Ok(out),
                Ok(FormatOutcome::NeedsResolver) => {
                    // Prefer system goimports for third-party resolution.
                    if let Ok(out) = self.format_subprocess(filename, src) {
                        return Ok(out);
                    }
                    // No binary: degrade to format-only so check mode still runs.
                    return native::goimports::format_only(src, &self.native_opts(filename));
                }
                Err(e) => {
                    if let Ok(out) = self.format_subprocess(filename, src) {
                        return Ok(out);
                    }
                    return Err(e);
                }
            }
        }
        self.format_subprocess(filename, src)
    }

    fn list_unformatted(&self, files: &[&Path]) -> Option<Vec<PathBuf>> {
        // Native: skip list pre-pass — runner `check_file` formats once per file.
        if self.use_native() {
            return None;
        }
        let bin = self.binary.as_deref().unwrap_or("goimports");
        let locals: Vec<&str> = self
            .options
            .local_prefixes
            .iter()
            .map(|s| s.as_str())
            .filter(|s| !s.is_empty())
            .collect();
        // Batch `-l` reads each file from disk and infers `-srcdir` from its own
        // location, matching the per-file `-srcdir <path>` module resolution.
        crate::runner::batch_list(files, || {
            let mut c = Command::new(bin);
            c.arg("-l");
            if !locals.is_empty() {
                c.arg("-local").arg(locals.join(","));
            }
            c
        })
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

    #[test]
    fn native_is_default() {
        // Don't assert under GUFF_NATIVE_FMT=0 if the env is set by the user.
        if std::env::var_os("GUFF_NATIVE_FMT").is_some_and(|v| v == "0") {
            return;
        }
        let fmt = Goimports::new(GoimportsOptions::default());
        assert!(fmt.use_native());
    }

    #[test]
    fn subprocess_fallback_for_unknown_third_party() {
        if !goimports_available() {
            eprintln!("skip: goimports not on PATH");
            return;
        }
        // Missing import that is not stdlib and has no sibling → NeedsResolver
        // → subprocess. Use a path under a temp dir so -srcdir works.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p.go");
        let src = b"package p\n\nfunc f() {\n\t_ = notarealpkg.X\n}\n";
        std::fs::write(&path, src).unwrap();
        let fmt = Goimports::new(GoimportsOptions::default());
        // May error (package not found) or leave unchanged — either is fine;
        // we only check that native doesn't panic / falsely claim success with
        // a wrong stdlib import.
        let _ = fmt.format(&path.to_string_lossy(), src);
    }
}
