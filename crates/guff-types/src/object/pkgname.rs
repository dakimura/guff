//! Port of the `PkgName` parts of `cmd/compile/internal/types2/object.go`.
//!
//! A `PkgName` is the local name introduced by an `import` declaration; it
//! denotes the imported package within the importing file's scope. Its type is
//! always `Typ[Invalid]` (a package name can only appear in a qualified
//! identifier `pkg.X`, never as a value).
//!
//! We have no `Importer`, so the only package ever imported is the synthetic
//! `unsafe` package (created in the universe). `imported` holds that package.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageId, TypeId};
use crate::object::{HasMeta, ObjectMeta};

/// The local name bound by an `import` declaration.
///
/// Equivalent to `types2.PkgName`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgName {
    name: String,        // the local name (import alias, or the package's name)
    typ: TypeId,         // always Typ[Invalid]
    imported: PackageId, // the package this name refers to
    pub(crate) meta: ObjectMeta,
}

impl PkgName {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.typ = r.ty(self.typ);
        self.imported = r.pkg(self.imported);
        self.meta.remap_ids(r);
    }
}

impl PkgName {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn typ(&self) -> TypeId {
        self.typ
    }

    /// The package this name imports.
    ///
    /// Equivalent to `PkgName.Imported`.
    pub fn imported(&self) -> PackageId {
        self.imported
    }
}

impl HasMeta for PkgName {
    fn meta(&self) -> &ObjectMeta {
        &self.meta
    }
    fn meta_mut(&mut self) -> &mut ObjectMeta {
        &mut self.meta
    }
}

/// Construct a new [`PkgName`]. `invalid_typ` must be the predeclared
/// `Typ[Invalid]` from the universe.
///
/// Equivalent to `types2.NewPkgName`.
pub fn new_pkg_name(
    arena: &mut ObjectArena,
    name: impl Into<String>,
    imported: PackageId,
    invalid_typ: TypeId,
) -> ObjectId {
    arena.alloc(ObjectData::PkgName(PkgName {
        name: name.into(),
        typ: invalid_typ,
        imported,
        meta: ObjectMeta::default(),
    }))
}

impl ObjectId {
    /// If this object is a [`PkgName`], returns the package it imports.
    pub fn imported_pkg(self, arena: &ObjectArena) -> Option<PackageId> {
        match arena.get(self) {
            ObjectData::PkgName(p) => Some(p.imported()),
            _ => None,
        }
    }
}
