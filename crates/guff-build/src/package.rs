//! `Package` and related types from `go/build`.

use std::path::PathBuf;

/// Controls the behavior of [`crate::import_path::import`].
///
/// Equivalent to `build.ImportMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ImportMode(pub u32);

impl ImportMode {
    pub const NONE: ImportMode = ImportMode(0);
    /// Stop after locating the package directory.
    pub const FIND_ONLY: ImportMode = ImportMode(1);
}

/// A Go package located on disk.
///
/// Equivalent to `build.Package`. Only fields needed for Phase 1 are populated;
/// import lists, embed patterns, and cgo directives are deferred.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Package {
    /// Directory containing package sources.
    pub dir: PathBuf,
    /// Package name from `package` declarations.
    pub name: String,
    /// Import path (`""` if unknown).
    pub import_path: String,
    /// Module root directory when resolved via `go.mod`.
    pub root: String,
    /// Package found in `GOROOT`.
    pub goroot: bool,

    /// `.go` source files (excluding cgo, test, xtest).
    pub go_files: Vec<String>,
    /// `.go` files that import `"C"` (cgo preprocessing deferred).
    pub cgo_files: Vec<String>,
    /// `.go` files ignored for this build (build tags, etc.).
    pub ignored_go_files: Vec<String>,
    /// `.go` files with detected problems.
    pub invalid_go_files: Vec<String>,
    /// `_test.go` files in the package.
    pub test_go_files: Vec<String>,
    /// `_test.go` files with `package foo_test`.
    pub xtest_go_files: Vec<String>,

    /// Build tags consulted while classifying files in this directory.
    pub all_tags: Vec<String>,
}

impl Package {
    /// Reports whether the package is a command (`package main`).
    pub fn is_command(&self) -> bool {
        self.name == "main"
    }
}

/// The directory contains no buildable Go source files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoGoError {
    pub dir: PathBuf,
}

impl std::fmt::Display for NoGoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "no buildable Go source files in {}", self.dir.display())
    }
}

impl std::error::Error for NoGoError {}

/// Multiple package names found in one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultiplePackageError {
    pub dir: PathBuf,
    pub packages: [String; 2],
    pub files: [String; 2],
}

impl std::fmt::Display for MultiplePackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "found packages {} ({}) and {} ({}) in {}",
            self.packages[0],
            self.files[0],
            self.packages[1],
            self.files[1],
            self.dir.display()
        )
    }
}

impl std::error::Error for MultiplePackageError {}

/// Errors returned while loading a package directory.
#[derive(Debug)]
pub enum BuildError {
    Io(std::io::Error),
    NoGo(NoGoError),
    MultiplePackages(MultiplePackageError),
    Match(crate::match_file::MatchError),
    Import(String),
}

impl std::fmt::Display for BuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildError::Io(e) => write!(f, "{e}"),
            BuildError::NoGo(e) => write!(f, "{e}"),
            BuildError::MultiplePackages(e) => write!(f, "{e}"),
            BuildError::Match(e) => write!(f, "{e}"),
            BuildError::Import(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for BuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            BuildError::Io(e) => Some(e),
            BuildError::NoGo(e) => Some(e),
            BuildError::MultiplePackages(e) => Some(e),
            BuildError::Match(e) => Some(e),
            BuildError::Import(_) => None,
        }
    }
}

impl From<std::io::Error> for BuildError {
    fn from(value: std::io::Error) -> Self {
        BuildError::Io(value)
    }
}

impl From<crate::match_file::MatchError> for BuildError {
    fn from(value: crate::match_file::MatchError) -> Self {
        BuildError::Match(value)
    }
}
