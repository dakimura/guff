// Port of Go's go/parser/interface.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// User-facing entry points layered on top of [`crate::parser`]:
//
// * [`parse_file`] — reads source from bytes-in-memory or a filesystem
//   path, builds an [`File`](crate::ast::File).
// * [`parse_dir`] — parses every `.go` file in a directory, grouped by
//   package name.
// * [`parse_expr_from`] / [`parse_expr`] — re-exports of the same-name
//   helpers in `parser.rs`, kept here for API ergonomics.
//
// Differences from Go:
//
// * Source is `Option<&[u8]>` rather than Go's untyped `any`. Callers
//   that hold a `String`/`Vec<u8>` pass `Some(s.as_bytes())`; readers
//   should `read_to_end` into a `Vec<u8>` first.
// * Errors are returned as a typed [`ParseError`] (`Io` for filesystem
//   failures, `Syntax` for parser errors). Go conflates them under
//   `error`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ast::{Expr, File, Ident, Package};
use crate::errors::ErrorList;
use crate::parser::{self, Mode};
use crate::position::FileSet;

// ====================================================================
// ParseError
// ====================================================================

/// Combined error type for the high-level parser entry points.
#[derive(Debug)]
pub enum ParseError {
    /// I/O failure (file not found, unreadable, etc.).
    Io(io::Error),
    /// Parser produced one or more syntax errors.
    Syntax(ErrorList),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::Io(e) => write!(f, "io error: {}", e),
            ParseError::Syntax(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::Io(e) => Some(e),
            ParseError::Syntax(_) => None,
        }
    }
}

impl From<io::Error> for ParseError {
    fn from(e: io::Error) -> Self {
        ParseError::Io(e)
    }
}

impl From<ErrorList> for ParseError {
    fn from(e: ErrorList) -> Self {
        ParseError::Syntax(e)
    }
}

// ====================================================================
// read_source
// ====================================================================

/// Read source either from caller-provided bytes (`src.is_some()`) or
/// from the filesystem (`src.is_none()`).
pub fn read_source(filename: &str, src: Option<&[u8]>) -> io::Result<Vec<u8>> {
    match src {
        Some(b) => Ok(b.to_vec()),
        None => fs::read(filename),
    }
}

// ====================================================================
// ParseFile
// ====================================================================

/// Parse a single Go source file.
///
/// * `fset` records position info — must outlive the returned `File`.
/// * `filename` is used both for resolving on-disk sources (when
///   `src` is `None`) and for diagnostic positions.
/// * `src` provides the bytes directly; pass `None` to read from
///   `filename`.
/// * `mode` controls optional parser features. Pass [`Mode::NONE`] for
///   defaults, or combine [`PACKAGE_CLAUSE_ONLY`] / [`PARSE_COMMENTS`]
///   / [`SKIP_OBJECT_RESOLUTION`] etc.
///
/// On syntax errors, returns [`ParseError::Syntax`] with a sorted
/// [`ErrorList`]. The partial AST is *not* returned, matching Go's
/// behavior of "either valid or error-only" for the public API. (Use
/// [`crate::parser::parse_file`] directly if you want raw access.)
pub fn parse_file(
    fset: &Arc<FileSet>,
    filename: &str,
    src: Option<&[u8]>,
    mode: Mode,
) -> Result<File, ParseError> {
    let text = read_source(filename, src)?;
    parser::parse_file(fset, filename, &text, mode).map_err(ParseError::Syntax)
}

// ====================================================================
// ParseDir
// ====================================================================

/// Parse every `.go` file in the directory at `path`. Returns a map
/// keyed by package name.
///
/// `filter` (optional) receives each candidate entry; only entries
/// passing the filter (in addition to having a `.go` suffix) are
/// parsed.
///
/// On parse errors, parsing continues for the rest of the files —
/// the first error encountered is returned alongside the partial map.
///
/// Deprecated like Go's upstream `ParseDir`: it does not honor
/// build-tags and is mostly suitable for trivial multi-file packages.
/// Real-world tools should use the equivalent of
/// `golang.org/x/tools/go/packages` instead.
pub fn parse_dir<F>(
    fset: &Arc<FileSet>,
    path: &Path,
    filter: Option<F>,
    mode: Mode,
) -> Result<(BTreeMap<String, Package>, Option<ParseError>), io::Error>
where
    F: Fn(&fs::DirEntry) -> bool,
{
    let read = fs::read_dir(path)?;
    let mut pkgs: BTreeMap<String, Package> = BTreeMap::new();
    let mut first_err: Option<ParseError> = None;

    // Collect + sort filenames for deterministic iteration (Go's
    // os.ReadDir is unsorted; we sort here so tests don't flake).
    let mut entries: Vec<fs::DirEntry> = read.filter_map(|r| r.ok()).collect();
    entries.sort_by_key(|d| d.file_name());

    for d in entries {
        let file_type = match d.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            continue;
        }
        let name_os = d.file_name();
        let name = match name_os.to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.ends_with(".go") {
            continue;
        }
        if let Some(f) = filter.as_ref() {
            if !f(&d) {
                continue;
            }
        }
        let filename_path: PathBuf = path.join(&name);
        let filename = match filename_path.to_str() {
            Some(s) => s.to_string(),
            None => continue,
        };
        match parse_file(fset, &filename, None, mode) {
            Ok(src) => {
                let pkg_name = src.name.name.clone();
                let pkg = pkgs.entry(pkg_name.clone()).or_insert_with(|| Package {
                    name: pkg_name,
                    files: BTreeMap::new(),
                    ..Default::default()
                });
                pkg.files.insert(filename, src);
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    Ok((pkgs, first_err))
}

// ====================================================================
// ParseExprFrom / ParseExpr
// ====================================================================

/// Convenience wrapper around [`crate::parser::parse_expr_from`] that
/// accepts the same `Option<&[u8]>` source convention as
/// [`parse_file`]. With `src = None`, reads from `filename`.
pub fn parse_expr_from(
    fset: &Arc<FileSet>,
    filename: &str,
    src: Option<&[u8]>,
    mode: Mode,
) -> Result<Expr, ParseError> {
    let text = read_source(filename, src)?;
    parser::parse_expr_from(fset, filename, &text, mode).map_err(ParseError::Syntax)
}

/// Parse a single Go expression. Position info is recorded into a
/// throwaway [`FileSet`]; the filename used in errors is the empty
/// string.
pub fn parse_expr(x: &str) -> Result<Expr, ErrorList> {
    let fset = FileSet::new();
    parser::parse_expr_from(&fset, "", x.as_bytes(), Mode::NONE)
}

// ====================================================================
// Helpers
// ====================================================================

/// Suppress dead-code warnings for the `Ident::default()` helper that
/// some downstream re-exports may rely on without referencing here.
#[allow(dead_code)]
fn _ident_marker() -> Ident {
    Ident::default()
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Drop guard that deletes a temp file when it goes out of scope.
    struct TempFile {
        path: PathBuf,
    }
    impl TempFile {
        fn new(name: &str, contents: &str) -> Self {
            let mut path = std::env::temp_dir();
            // Use both process id and a nanosecond timestamp so tests in
            // the same process don't collide.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            path.push(format!(
                "guff_pi_{}_{}_{}.go",
                std::process::id(),
                stamp,
                name
            ));
            let mut f = fs::File::create(&path).expect("create");
            f.write_all(contents.as_bytes()).expect("write");
            TempFile { path }
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.path);
        }
    }
    /// Drop guard that recursively deletes a temp directory.
    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn new(label: &str) -> Self {
            let mut path = std::env::temp_dir();
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            path.push(format!(
                "guff_pi_dir_{}_{}_{}",
                std::process::id(),
                stamp,
                label
            ));
            fs::create_dir_all(&path).expect("mkdir");
            TempDir { path }
        }
        fn write(&self, name: &str, contents: &str) {
            let p = self.path.join(name);
            let mut f = fs::File::create(&p).expect("create");
            f.write_all(contents.as_bytes()).expect("write");
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn read_source_with_inline_bytes() {
        let bytes = b"package p\n";
        let got = read_source("ignored.go", Some(bytes)).unwrap();
        assert_eq!(got, bytes);
    }

    #[test]
    fn parse_file_from_inline_bytes() {
        let fset = FileSet::new();
        let f = parse_file(&fset, "t.go", Some(b"package q\n"), Mode::NONE).unwrap();
        assert_eq!(f.name.name, "q");
    }

    #[test]
    fn parse_file_io_error_is_typed() {
        let fset = FileSet::new();
        let err = parse_file(
            &fset,
            "/nonexistent/path/should_not_exist.go",
            None,
            Mode::NONE,
        )
        .expect_err("expected io error");
        assert!(matches!(err, ParseError::Io(_)));
    }

    #[test]
    fn parse_expr_simple() {
        let e = parse_expr("a + b").unwrap();
        assert!(matches!(e, Expr::BinaryExpr(_)));
    }

    #[test]
    fn parse_expr_from_inline() {
        let fset = FileSet::new();
        let e = parse_expr_from(&fset, "expr", Some(b"x.Y"), Mode::NONE).unwrap();
        assert!(matches!(e, Expr::SelectorExpr(_)));
    }

    #[test]
    fn parse_file_reads_from_disk_when_src_is_none() {
        let tmp = TempFile::new("disk_read", "package fromdisk\n");
        let fset = FileSet::new();
        let path_str = tmp.path.to_str().unwrap();
        let f = parse_file(&fset, path_str, None, Mode::NONE).unwrap();
        assert_eq!(f.name.name, "fromdisk");
    }

    #[test]
    fn parse_dir_groups_files_by_package_name() {
        let dir = TempDir::new("group");
        dir.write("a.go", "package alpha\n");
        dir.write("b.go", "package alpha\nfunc B() {}\n");
        dir.write("c.go", "package beta\n");
        // Non-go file is ignored.
        dir.write("README.txt", "ignored\n");

        let fset = FileSet::new();
        let no_filter: Option<fn(&fs::DirEntry) -> bool> = None;
        let (pkgs, err) = parse_dir(&fset, &dir.path, no_filter, Mode::NONE).unwrap();
        assert!(err.is_none(), "expected no parse errors, got {:?}", err);
        assert_eq!(pkgs.len(), 2);
        let alpha = pkgs.get("alpha").expect("alpha pkg");
        assert_eq!(alpha.files.len(), 2);
        let beta = pkgs.get("beta").expect("beta pkg");
        assert_eq!(beta.files.len(), 1);
    }

    #[test]
    fn parse_dir_filter_skips_files() {
        let dir = TempDir::new("filter");
        dir.write("keep.go", "package k\n");
        dir.write("skip_me.go", "package s\n");
        let fset = FileSet::new();
        let (pkgs, err) = parse_dir(
            &fset,
            &dir.path,
            Some(|d: &fs::DirEntry| !d.file_name().to_string_lossy().starts_with("skip_")),
            Mode::NONE,
        )
        .unwrap();
        assert!(err.is_none());
        assert!(pkgs.contains_key("k"));
        assert!(!pkgs.contains_key("s"));
    }

    #[test]
    fn parse_dir_reports_first_parse_error_but_keeps_going() {
        let dir = TempDir::new("err");
        dir.write("good.go", "package good\n");
        // Bad: no package clause.
        dir.write("bad.go", "func()\n");

        let fset = FileSet::new();
        let no_filter: Option<fn(&fs::DirEntry) -> bool> = None;
        let (pkgs, err) = parse_dir(&fset, &dir.path, no_filter, Mode::NONE).unwrap();
        assert!(err.is_some(), "should have surfaced a parse error");
        // The valid file still made it through.
        assert!(pkgs.get("good").is_some());
    }
}
