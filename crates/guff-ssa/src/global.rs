//! SSA Globals.

use crate::ids::PackageId;
use guff_types::{ObjectId, TypeId};

/// Global represents a package-level variable.
/// (Go: `Global`)
pub struct Global {
    pub name: String,
    pub pkg: PackageId,
    pub typ: TypeId, // pointer to underlying type
    /// the type-checker `Var` object this global was created from; `None` for
    /// synthetic globals (e.g. the init guard). (Go: `Global.object`)
    pub object: Option<ObjectId>,
    // DEFERRED: pos
}

impl Global {
    pub fn new(name: String, pkg: PackageId, typ: TypeId) -> Self {
        Self { name, pkg, typ, object: None }
    }
}
