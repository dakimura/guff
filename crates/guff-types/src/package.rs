//! Port of `cmd/compile/internal/types2/package.go`.
//!
//! Chunk-7 deferrals: `cgo` / `fake` flags (internal compiler use) are
//! omitted until they become load-bearing.

use serde::{Deserialize, Serialize};

use crate::arena::{PackageArena, PackageId, ScopeArena, ScopeId};
use crate::scope::new_scope;

/// A Go package — a name, an import path, a top-level [`Scope`], and a
/// list of imported packages.
///
/// Equivalent to `types2.Package`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    path: String,
    name: String,
    scope: ScopeId,
    imports: Vec<PackageId>,
    complete: bool,
    go_version: String,
}

impl Package {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.scope = r.scope(self.scope);
        for imp in &mut self.imports {
            *imp = r.pkg(*imp);
        }
    }
}

impl Package {
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Sets the package's import path (Go: `Package.path` is normally fixed at
    /// [`new_package`], but source checkers allocate with `""` and fill it in
    /// from the loader's known path before checking).
    pub fn set_path(&mut self, path: impl Into<String>) {
        self.path = path.into();
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    pub fn scope(&self) -> ScopeId {
        self.scope
    }

    pub fn imports(&self) -> &[PackageId] {
        &self.imports
    }

    pub fn set_imports(&mut self, list: Vec<PackageId>) {
        self.imports = list;
    }

    pub fn complete(&self) -> bool {
        self.complete
    }

    pub fn mark_complete(&mut self) {
        self.complete = true;
    }

    pub fn go_version(&self) -> &str {
        &self.go_version
    }

    pub fn set_go_version(&mut self, v: impl Into<String>) {
        self.go_version = v.into();
    }
}

/// Construct a new package with the given path and name. Creates a new
/// package-level scope parented at `universe_scope`.
///
/// Equivalent to `types2.NewPackage`.
pub fn new_package(
    package_arena: &mut PackageArena,
    scope_arena: &mut ScopeArena,
    universe_scope: ScopeId,
    path: impl Into<String>,
    name: impl Into<String>,
) -> PackageId {
    let path_str = path.into();
    let comment = format!("package \"{}\"", path_str);
    let scope = new_scope(
        scope_arena,
        Some(universe_scope),
        Some(universe_scope),
        0,
        0,
        comment,
    );
    package_arena.alloc(Package {
        path: path_str,
        name: name.into(),
        scope,
        imports: Vec::new(),
        complete: false,
        go_version: String::new(),
    })
}
