//! Package importing — resolving an `import "path"` to a [`Package`].
//!
//! Port of the role played by `types2.Importer` / `Config.Importer`. Go's
//! `Importer` returns a `*Package` whose objects are independently allocated and
//! referenced by pointer. This port stores every type / object / scope in the
//! checker's arenas and refers to them by index, so an imported package's
//! contents must be allocated **into those same arenas**. The [`Importer`] trait
//! is therefore handed an [`ImportCtx`] granting mutable access to the arenas.
//!
//! The checker owns the importer (set via
//! [`Checker::set_importer`](crate::Checker::set_importer)) rather than reading
//! it from [`Config`](crate::Config): `Config` stays a plain, `Clone`-able data
//! struct, and a boxed trait object is neither `Clone` nor `Debug`.
//!
//! The synthetic `unsafe` package is resolved by the checker itself and never
//! reaches the importer.

use crate::arena::{ObjectArena, PackageArena, PackageId, ScopeArena, ScopeId, TypeArena};

/// Mutable access to the checker's arenas, handed to an [`Importer`] so it can
/// allocate the imported package (its scope, and the exported objects/types
/// within it). Use [`crate::package::new_package`] to create the package and the
/// object constructors (`new_const`, `new_type_name`, …) plus
/// [`crate::scope::insert`] to populate its scope.
pub struct ImportCtx<'a> {
    pub types: &'a mut TypeArena,
    pub objects: &'a mut ObjectArena,
    pub scopes: &'a mut ScopeArena,
    pub packages: &'a mut PackageArena,
    /// The predeclared universe scope; parent of every package scope.
    pub universe_scope: ScopeId,
}

/// Resolves an import path to an already-loaded (or freshly built) package.
///
/// Equivalent to `types2.Importer`. `import` is called at most once per path per
/// checker run (the checker caches the result), and must return a package whose
/// [`scope`](crate::package::Package::scope) holds its exported objects so that
/// `pkg.X` selectors resolve. Returning `None` means the path can't be resolved;
/// the checker then leaves the import unbound (matching the pre-importer
/// behaviour for unknown paths).
pub trait Importer {
    fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<PackageId>;
}
