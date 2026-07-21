//! `gci` formatter — shells out to the `gci` binary (`github.com/daixiang0/gci`).
//!
//! Matches golangci-lint `pkg/goformatters/gci` / `formatters.settings.gci`:
//! - `sections` → repeated `-s` / `--section`
//! - `custom-order` → `--custom-order`
//! - `no-lex-order` → `--no-lex-order`
//! - `no-inline-comments` / `no-prefix-comments` → post-processing on the
//!   formatted import block (the gci 0.14 `print` CLI does not expose these, so
//!   we strip the comments ourselves to match the gci library behavior).
//!
//! Default sections are `standard` / `default` (golangci default).
//!
//! `gci print` requires a file path (no stdin), so we stage `src` into a temp
//! `.go` file. When `filename` is a real path, the temp file is created in the
//! same directory so `localmodule` can resolve `go.mod`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::runner::FormatError;
use crate::Formatter;

pub const NAME: &str = "gci";

/// Options for [`Gci`] (`formatters.settings.gci`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GciOptions {
    /// Section list (`-s`). Empty → golangci/gci default `["standard", "default"]`.
    pub sections: Vec<String>,
    /// Pass `--custom-order` (section order follows `sections`).
    pub custom_order: bool,
    /// Pass `--no-lex-order`.
    pub no_lex_order: bool,
    /// Strip inline comments in the import block (post-processed; CLI gap).
    pub no_inline_comments: bool,
    /// Strip standalone prefix comments in the import block (post-processed).
    pub no_prefix_comments: bool,
}

impl Default for GciOptions {
    fn default() -> Self {
        Self {
            sections: vec!["standard".into(), "default".into()],
            custom_order: false,
            no_lex_order: false,
            no_inline_comments: false,
            no_prefix_comments: false,
        }
    }
}

/// Formatter that invokes the system `gci` binary.
#[derive(Debug, Clone, Default)]
pub struct Gci {
    options: GciOptions,
    /// Override binary path (tests / non-standard installs).
    binary: Option<String>,
}

impl Gci {
    pub fn new(options: GciOptions) -> Self {
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

impl Formatter for Gci {
    fn name(&self) -> &str {
        NAME
    }

    fn format(&self, filename: &str, src: &[u8]) -> Result<Vec<u8>, FormatError> {
        let bin = self.binary.as_deref().unwrap_or("gci");
        let temp = stage_temp(filename, src)?;
        let _guard = TempGuard(temp.clone());

        let mut cmd = Command::new(bin);
        cmd.arg("print");
        let sections = if self.options.sections.is_empty() {
            &["standard".to_string(), "default".to_string()][..]
        } else {
            &self.options.sections[..]
        };
        for section in sections {
            if !section.is_empty() {
                cmd.arg("-s").arg(section);
            }
        }
        if self.options.custom_order {
            cmd.arg("--custom-order");
        }
        if self.options.no_lex_order {
            cmd.arg("--no-lex-order");
        }
        // no-inline-comments / no-prefix-comments are not on the `print` CLI;
        // post-processed below.
        cmd.arg(&temp);
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
                message: format!("gci failed: {}", stderr.trim()),
            });
        }

        if self.options.no_inline_comments || self.options.no_prefix_comments {
            Ok(strip_import_comments(
                &output.stdout,
                self.options.no_inline_comments,
                self.options.no_prefix_comments,
            ))
        } else {
            Ok(output.stdout)
        }
    }

    fn list_unformatted(&self, files: &[&Path]) -> Option<Vec<PathBuf>> {
        // The `no-inline` / `no-prefix` comment stripping is post-processing on
        // top of `gci print`, so it can flip the "differs" decision in ways the
        // `list` subcommand can't see. Fall back to per-file in that case.
        if self.options.no_inline_comments || self.options.no_prefix_comments {
            return None;
        }
        let bin = self.binary.as_deref().unwrap_or("gci");
        let sections: Vec<String> = if self.options.sections.is_empty() {
            vec!["standard".to_string(), "default".to_string()]
        } else {
            self.options.sections.clone()
        };
        crate::runner::batch_list(files, || {
            let mut c = Command::new(bin);
            c.arg("list");
            for section in &sections {
                if !section.is_empty() {
                    c.arg("-s").arg(section);
                }
            }
            if self.options.custom_order {
                c.arg("--custom-order");
            }
            if self.options.no_lex_order {
                c.arg("--no-lex-order");
            }
            c
        })
    }
}

/// Remove inline and/or standalone prefix comments inside the `import ( … )`
/// block, mirroring the gci `NoInlineComments` / `NoPrefixComments` options.
fn strip_import_comments(src: &[u8], no_inline: bool, no_prefix: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(src);
    let trailing_newline = text.ends_with('\n');
    let mut out: Vec<String> = Vec::new();
    let mut in_block = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if !in_block {
            if trimmed.starts_with("import (") {
                in_block = true;
            }
            out.push(line.to_string());
            continue;
        }

        // Inside `import ( … )`.
        if trimmed.starts_with(')') {
            in_block = false;
            out.push(line.to_string());
            continue;
        }

        let is_comment_only = trimmed.starts_with("//") || trimmed.starts_with("/*");
        if is_comment_only {
            if no_prefix {
                continue; // drop standalone prefix comment line
            }
            out.push(line.to_string());
            continue;
        }

        if no_inline {
            if let Some(stripped) = strip_inline_comment(line) {
                out.push(stripped);
                continue;
            }
        }
        out.push(line.to_string());
    }

    let mut joined = out.join("\n");
    if trailing_newline {
        joined.push('\n');
    }
    joined.into_bytes()
}

/// Strip a trailing `// …` comment from an import line, keeping the import spec.
/// Returns `None` when there is no inline comment to remove.
fn strip_inline_comment(line: &str) -> Option<String> {
    // Only strip after the closing quote of the import path so `//` inside the
    // string literal is preserved.
    let close = line.rfind('"')?;
    let after = &line[close + 1..];
    let rel = after.find("//")?;
    let cut = close + 1 + rel;
    Some(line[..cut].trim_end().to_string())
}

/// Write `src` next to `filename` when possible; otherwise system temp.
fn stage_temp(filename: &str, src: &[u8]) -> Result<PathBuf, FormatError> {
    let dir = preferred_temp_dir(filename);
    let path = unique_go_path(&dir)?;
    fs::write(&path, src).map_err(|e| FormatError::Io {
        formatter: NAME.to_string(),
        path: filename.to_string(),
        source: e,
    })?;
    Ok(path)
}

fn preferred_temp_dir(filename: &str) -> PathBuf {
    if filename != "<standard input>" && !filename.is_empty() {
        let p = Path::new(filename);
        if let Some(parent) = p.parent() {
            if !parent.as_os_str().is_empty() && parent.is_dir() {
                return parent.to_path_buf();
            }
        }
    }
    std::env::temp_dir()
}

fn unique_go_path(dir: &Path) -> Result<PathBuf, FormatError> {
    // Avoid depending on the `tempfile` crate in the library path.
    let pid = std::process::id();
    for n in 0..1000 {
        let name = format!(".guff-gci-{pid}-{n}.go");
        let path = dir.join(name);
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                // File created empty; caller overwrites with write().
                let _ = f.write_all(b"");
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                return Err(FormatError::Io {
                    formatter: NAME.to_string(),
                    path: path.display().to_string(),
                    source: e,
                });
            }
        }
    }
    Err(FormatError::Message {
        formatter: NAME.to_string(),
        path: dir.display().to_string(),
        message: "failed to create temp file for gci".into(),
    })
}

struct TempGuard(PathBuf);

impl Drop for TempGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_inline_comments_in_import_block() {
        let src = b"package p\n\nimport (\n\t\"fmt\" // std\n\t\"os\"\n)\n\nfunc f() {}\n";
        let out = strip_import_comments(src, true, false);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("\t\"fmt\"\n"), "inline comment kept:\n{s}");
        assert!(!s.contains("// std"), "inline comment not removed:\n{s}");
    }

    #[test]
    fn strip_prefix_comments_in_import_block() {
        let src =
            b"package p\n\nimport (\n\t// standard\n\t\"fmt\"\n\t\"os\"\n)\n\nfunc f() {}\n";
        let out = strip_import_comments(src, false, true);
        let s = String::from_utf8(out).unwrap();
        assert!(!s.contains("// standard"), "prefix comment not removed:\n{s}");
        assert!(s.contains("\t\"fmt\"\n"), "import spec lost:\n{s}");
    }

    #[test]
    fn keeps_comments_outside_import_block() {
        let src = b"package p\n\n// keep me\nfunc f() {} // and me\n";
        let out = strip_import_comments(src, true, true);
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("// keep me"), "doc comment removed:\n{s}");
        assert!(s.contains("// and me"), "code comment removed:\n{s}");
    }

    #[test]
    fn preserves_double_slash_inside_import_path() {
        let line = "\t\"example.com/a//b\"";
        assert_eq!(strip_inline_comment(line), None);
    }

    fn gci_available() -> bool {
        Command::new("gci")
            .arg("print")
            .arg("--help")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn sorts_stdlib_before_third_party() {
        if !gci_available() {
            eprintln!("skip: gci not on PATH");
            return;
        }
        let fmt = Gci::new(GciOptions::default());
        let src = b"package p\n\nimport (\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n}\n";
        let out = fmt.format("p.go", src).expect("gci");
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").expect("fmt import");
        let bar_pos = s.find("\"github.com/foo/bar\"").expect("bar import");
        assert!(
            fmt_pos < bar_pos,
            "expected stdlib before third-party, got:\n{s}"
        );
        assert!(
            s.contains("\"fmt\"\n\n\t\"github.com/foo/bar\""),
            "expected blank line between sections, got:\n{s}"
        );
    }

    #[test]
    fn custom_order_prefix_section() {
        if !gci_available() {
            eprintln!("skip: gci not on PATH");
            return;
        }
        let fmt = Gci::new(GciOptions {
            sections: vec![
                "standard".into(),
                "default".into(),
                "prefix(github.com/org/project)".into(),
            ],
            custom_order: true,
            ..Default::default()
        });
        let src = b"package p\n\nimport (\n\t\"github.com/org/project/pkg\"\n\t\"github.com/foo/bar\"\n\t\"fmt\"\n)\n\nfunc f() {\n\tfmt.Println()\n\t_ = bar.X\n\t_ = pkg.Y\n}\n";
        let out = fmt.format("p.go", src).expect("gci custom-order");
        let s = String::from_utf8(out).unwrap();
        let fmt_pos = s.find("\"fmt\"").expect("fmt");
        let bar_pos = s.find("\"github.com/foo/bar\"").expect("bar");
        let pkg_pos = s.find("\"github.com/org/project/pkg\"").expect("pkg");
        assert!(
            fmt_pos < bar_pos && bar_pos < pkg_pos,
            "expected standard < default < prefix, got:\n{s}"
        );
    }
}
