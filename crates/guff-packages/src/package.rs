//! [`Package`], [`Module`], and related types.
//!
//! Port of `packages.Package`, `packages.Module`, and `packages.Error` from `packages.go`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use guff::ast::File;
use guff::position::FileSet;
use guff_types::api::Info;
use guff_types::arena::{ObjectArena, PackageId, ScopeArena, TypeArena};
use guff_types::arena::PackageArena;
use guff_types::sizes::Sizes;

/// Type-checker arenas and results for a loaded package.
///
/// Holds the ownership of `guff-types` state produced when a package is
/// type-checked from source. Consumers such as `guff-ssa` take this bundle to
/// build SSA without re-running the checker.
#[derive(Clone)]
pub struct TypecheckArtifacts {
    pub type_pkg: PackageId,
    pub types: TypeArena,
    pub objects: ObjectArena,
    pub scopes: ScopeArena,
    pub packages: PackageArena,
    pub info: Info,
}

impl TypecheckArtifacts {
    /// Deep copy for SSA construction without consuming the package's artifacts.
    pub fn snapshot(&self) -> Self {
        self.clone()
    }
}

/// A loaded Go package.
///
/// Equivalent to `packages.Package`.
#[derive(Default)]
pub struct Package {
    /// Unique identifier from the build system (typically the import path).
    pub id: String,
    /// Package name from source `package` declarations.
    pub name: String,
    /// Import path used by `go/types`.
    pub pkg_path: String,
    /// Directory containing package sources.
    pub dir: PathBuf,
    /// Errors from metadata lookup, parsing, or type checking.
    pub errors: Vec<Error>,
    /// Absolute paths of Go source files.
    pub go_files: Vec<PathBuf>,
    /// Go files suitable for type checking.
    pub compiled_go_files: Vec<PathBuf>,
    /// Non-Go source files (assembly, C, etc.).
    pub other_files: Vec<PathBuf>,
    /// Files embedded with `go:embed`.
    pub embed_files: Vec<PathBuf>,
    /// `go:embed` patterns.
    pub embed_patterns: Vec<PathBuf>,
    /// Source files excluded by the current build configuration.
    pub ignored_files: Vec<PathBuf>,
    /// Path to export data for the package.
    pub export_file: PathBuf,
    /// Install path of the `.a` file or binary.
    pub target: PathBuf,
    /// Import path → imported package.
    pub imports: HashMap<String, Arc<Package>>,
    /// Transitive dependency import paths from `go list`.
    pub deps: Vec<String>,
    /// Module metadata when available.
    pub module: Option<Module>,
    /// Package under test, if any (`ForTest` in go list JSON).
    pub for_test: String,

    // -- Filled in Phase 4; left empty in Phase 2 --

    /// Type-checked package (arena id).
    pub types: Option<PackageId>,
    /// Owned type-checker state (`Types`, `TypesInfo`, arenas).
    pub type_artifacts: Option<TypecheckArtifacts>,
    /// Shared file set for syntax and types.
    pub fset: Option<Arc<FileSet>>,
    /// Whether the package or a dependency has type errors.
    pub ill_typed: bool,
    /// Parsed syntax trees for `compiled_go_files`.
    pub syntax: Vec<File>,
    /// Type information for syntax trees.
    pub types_info: Option<Info>,
    /// Effective sizes for `types_info`.
    pub types_sizes: Option<Sizes>,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("pkg_path", &self.pkg_path)
            .field("dir", &self.dir)
            .field("errors", &self.errors)
            .field("go_files", &self.go_files)
            .field("compiled_go_files", &self.compiled_go_files)
            .field("imports", &self.imports.keys().collect::<Vec<_>>())
            .field("deps", &self.deps)
            .field("module", &self.module)
            .field("for_test", &self.for_test)
            .field("ill_typed", &self.ill_typed)
            .finish_non_exhaustive()
    }
}

impl Clone for Package {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            name: self.name.clone(),
            pkg_path: self.pkg_path.clone(),
            dir: self.dir.clone(),
            errors: self.errors.clone(),
            go_files: self.go_files.clone(),
            compiled_go_files: self.compiled_go_files.clone(),
            other_files: self.other_files.clone(),
            embed_files: self.embed_files.clone(),
            embed_patterns: self.embed_patterns.clone(),
            ignored_files: self.ignored_files.clone(),
            export_file: self.export_file.clone(),
            target: self.target.clone(),
            imports: self.imports.clone(),
            deps: self.deps.clone(),
            module: self.module.clone(),
            for_test: self.for_test.clone(),
            types: self.types,
            type_artifacts: None,
            fset: self.fset.clone(),
            ill_typed: self.ill_typed,
            syntax: self.syntax.clone(),
            types_info: self.types_info.clone(),
            types_sizes: self.types_sizes,
        }
    }
}

/// Module metadata for a package.
///
/// Equivalent to `packages.Module`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    pub path: String,
    pub version: String,
    pub replace: Option<Box<Module>>,
    pub main: bool,
    pub indirect: bool,
    pub dir: PathBuf,
    pub go_mod: PathBuf,
    pub go_version: String,
    pub error: Option<ModuleError>,
}

/// Error loading a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleError {
    pub err: String,
}

/// A package metadata, syntax, or type error.
///
/// Equivalent to `packages.Error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub pos: String,
    pub msg: String,
    pub kind: ErrorKind,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pos = if self.pos.is_empty() { "-" } else { &self.pos };
        write!(f, "{pos}: {}", self.msg)
    }
}

impl std::error::Error for Error {}

/// Source of a [`Error`].
///
/// Equivalent to `packages.ErrorKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ErrorKind {
    #[default]
    Unknown,
    List,
    Parse,
    Type,
}

/// Response from a package driver (`go list` or external tool).
///
/// Equivalent to `packages.DriverResponse`.
#[derive(Debug, Clone, Default)]
pub struct DriverResponse {
    pub not_handled: bool,
    pub compiler: String,
    pub arch: String,
    pub roots: Vec<String>,
    pub packages: Vec<Arc<Package>>,
    pub go_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_default_is_empty() {
        let pkg = Package::default();
        assert!(pkg.id.is_empty());
        assert!(pkg.imports.is_empty());
        assert!(pkg.types.is_none());
        assert!(!pkg.ill_typed);
    }

    #[test]
    fn error_display_uses_dash_for_empty_pos() {
        let err = Error {
            pos: String::new(),
            msg: "boom".into(),
            kind: ErrorKind::List,
        };
        assert_eq!(err.to_string(), "-: boom");
    }
}
