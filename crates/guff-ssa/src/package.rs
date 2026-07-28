//! SSA Package.

use crate::hash::HashMap;
use guff_types::{PackageId as TypePackageId, ObjectId};
use crate::member::MemberData;
use crate::ids::{FuncId, GlobalId};
use crate::program::Program;
use crate::value::Value;

/// A Package is a single analyzed Go package.
/// (Go: `Package`)
pub struct Package {
    /// the corresponding go/types.Package
    pub pkg: TypePackageId,
    /// all package members keyed by name
    pub members: HashMap<String, MemberData>,
    /// maps each type-checker object to its SSA value
    pub objects: HashMap<ObjectId, Value>,
    /// include full debug info (DebugRef pseudo-instructions) when building
    /// functions of this package. Set via [`Package::set_debug_mode`] or by
    /// the `GLOBAL_DEBUG` builder mode. (Go: `Package.debug`)
    pub debug: bool,
    /// number of explicit `init` functions seen so far, used to give them
    /// unique member names `init#1`, `init#2`, ... (Go: `Package.ninit`)
    pub ninit: u32,
    /// the synthesized package initializer function `init`, built by
    /// [`crate::builder::build_package_init`]. (Go: `Package.init`)
    pub init: Option<FuncId>,
    /// the synthesized `init$guard` boolean variable that makes the initializer
    /// idempotent. `None` when built with `BARE_INITS`. (Go: the anonymous
    /// `init$guard` Global created in `CreatePackage`.)
    pub init_guard: Option<GlobalId>,
    /// `true` when the package was created from source syntax (`len(files) > 0`
    /// in go/ssa's `CreatePackage`). Import-only shells keep this `false`.
    /// (Go: `Package.syntax`)
    pub has_syntax: bool,
}

impl Package {
    pub fn new(pkg: TypePackageId) -> Self {
        Self {
            pkg,
            members: HashMap::default(),
            objects: HashMap::default(),
            debug: false,
            ninit: 0,
            init: None,
            init_guard: None,
            has_syntax: false,
        }
    }

    /// Returns the package member named `name` if it is a function. (Go:
    /// `(*Package).Func`.)
    pub fn func(&self, name: &str) -> Option<FuncId> {
        match self.members.get(name) {
            Some(MemberData::Function(fid)) => Some(*fid),
            _ => None,
        }
    }

    /// Reports whether this package was loaded from source syntax. (Go:
    /// `isSyntactic`.)
    pub fn is_syntactic(&self) -> bool {
        self.has_syntax
    }

    /// Returns the type-checker package id. (Go: `Package.Pkg`.)
    pub fn type_pkg(&self) -> TypePackageId {
        self.pkg
    }

    /// Returns the package's short name (`main`, `fmt`, …). (Go: `Pkg.Name`.)
    pub fn name(&self, prog: &Program) -> String {
        prog.package_arena.get(self.pkg).name().to_string()
    }

    /// set_debug_mode enables or disables the generation of debug information
    /// for functions of this package. (Go: `Package.SetDebugMode`)
    pub fn set_debug_mode(&mut self, debug: bool) {
        self.debug = debug;
    }
}
