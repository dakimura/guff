//! Arena-based storage for Go types and objects.
//!
//! Types and objects form a cyclic, mutable graph in `go/types` — Named's
//! underlying may be a Struct whose fields are Vars whose `typ` may point back
//! to that Named. We model this with two arenas (`TypeArena`, `ObjectArena`)
//! and `TypeId` / `ObjectId` indices in place of Go's `*T` pointers.
//!
//! - `TypeId` and `ObjectId` use `NonZeroU32` so `Option<TypeId>` stays 4
//!   bytes (matching the size of a bare ID and avoiding tag overhead).
//! - IDs are 1-indexed internally; index 0 is reserved as the niche.

use std::num::NonZeroU32;

use crate::alias::Alias;
use crate::array::Array;
use crate::basic::Basic;
use crate::chan::Chan;
use crate::interface::Interface;
use crate::map::Map;
use crate::named::Named;
use crate::object::builtin::Builtin;
use crate::object::const_::Const;
use crate::object::func::Func;
use crate::object::nil_::Nil;
use crate::object::pkgname::PkgName;
use crate::object::type_name::TypeName;
use crate::object::var::Var;
use crate::package::Package;
use crate::pointer::Pointer;
use crate::r#struct::Struct;
use crate::scope::Scope;
use crate::signature::Signature;
use crate::slice::Slice;
use crate::tuple::Tuple;
use crate::typeparam::TypeParam;
use crate::union::Union;

/// Handle to a type stored in a [`TypeArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct TypeId(NonZeroU32);

impl TypeId {
    /// Construct a `TypeId` from a 1-based arena index. Crate-internal helper
    /// for the few places that iterate an arena by position (e.g.
    /// [`crate::basic::lookup_basic`]).
    ///
    /// # Panics
    /// Panics if `index` is 0.
    pub(crate) fn from_index(index: usize) -> Self {
        TypeId(NonZeroU32::new(index as u32).expect("arena index never 0"))
    }
}

/// Handle to an object stored in an [`ObjectArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ObjectId(NonZeroU32);

/// Handle to a [`Scope`] stored in a [`ScopeArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct ScopeId(NonZeroU32);

/// Handle to a [`Package`] stored in a [`PackageArena`].
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct PackageId(NonZeroU32);

/// Storage for all Go types in a type-checking session. Types reference each
/// other by [`TypeId`]; the arena owns the underlying data.
///
/// Mutation is performed via [`TypeArena::get_mut`]. Concurrent access is not
/// supported (matching `go/types`' single-threaded `Checker`).
#[derive(Debug, Default, Clone)]
pub struct TypeArena {
    types: Vec<TypeData>,
}

/// Storage for all Go objects (variables, functions, type names, etc.).
#[derive(Debug, Default, Clone)]
pub struct ObjectArena {
    objects: Vec<ObjectData>,
}

/// Storage for all [`Scope`]s in a type-checking session.
#[derive(Debug, Default, Clone)]
pub struct ScopeArena {
    scopes: Vec<Scope>,
}

/// Storage for all [`Package`]s in a type-checking session.
#[derive(Debug, Default, Clone)]
pub struct PackageArena {
    packages: Vec<Package>,
}

/// Backing data for each [`TypeId`]. One variant per Go type kind.
///
/// Chunks 1–3 cover every type kind. The Checker proper (which animates them)
/// is still to come.
#[derive(Debug, Clone)]
pub enum TypeData {
    Basic(Basic),
    Array(Array),
    Slice(Slice),
    Pointer(Pointer),
    Map(Map),
    Chan(Chan),
    Tuple(Tuple),
    Struct(Struct),
    Signature(Signature),
    Interface(Interface),
    Union(Union),
    Named(Named),
    Alias(Alias),
    TypeParam(TypeParam),
}

/// Backing data for each [`ObjectId`].
///
/// Chunks 1–6 cover `Var`, `Func`, `TypeName`, `Const`, `Nil`, `Builtin`.
/// `PkgName` arrives with imports (D16). `Label` is still deferred.
#[derive(Debug, Clone)]
pub enum ObjectData {
    Var(Var),
    Func(Func),
    TypeName(TypeName),
    Const(Const),
    Nil(Nil),
    Builtin(Builtin),
    PkgName(PkgName),
}

impl TypeArena {
    /// Create an empty arena. To get the predeclared basic types as well, use
    /// [`crate::basic::init_universe`] which returns a populated arena plus
    /// the lookup table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert `data` and return a stable [`TypeId`] pointing to it.
    pub fn alloc(&mut self, data: TypeData) -> TypeId {
        self.types.push(data);
        // Index is 1-based so Option<TypeId> can use 0 as the niche.
        let raw = self.types.len() as u32;
        TypeId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: TypeId) -> &TypeData {
        let idx = (id.0.get() - 1) as usize;
        &self.types[idx]
    }

    pub fn get_mut(&mut self, id: TypeId) -> &mut TypeData {
        let idx = (id.0.get() - 1) as usize;
        &mut self.types[idx]
    }

    pub fn len(&self) -> usize {
        self.types.len()
    }

    pub fn is_empty(&self) -> bool {
        self.types.is_empty()
    }
}

impl ObjectArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: ObjectData) -> ObjectId {
        self.objects.push(data);
        let raw = self.objects.len() as u32;
        ObjectId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: ObjectId) -> &ObjectData {
        let idx = (id.0.get() - 1) as usize;
        &self.objects[idx]
    }

    pub fn get_mut(&mut self, id: ObjectId) -> &mut ObjectData {
        let idx = (id.0.get() - 1) as usize;
        &mut self.objects[idx]
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// Iterates all allocated object ids in creation order.
    pub fn ids(&self) -> impl Iterator<Item = ObjectId> + '_ {
        (0..self.len()).map(|i| ObjectId(NonZeroU32::new((i + 1) as u32).expect("object id")))
    }
}

impl ScopeArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: Scope) -> ScopeId {
        self.scopes.push(data);
        let raw = self.scopes.len() as u32;
        ScopeId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: ScopeId) -> &Scope {
        &self.scopes[(id.0.get() - 1) as usize]
    }

    pub fn get_mut(&mut self, id: ScopeId) -> &mut Scope {
        &mut self.scopes[(id.0.get() - 1) as usize]
    }

    pub fn len(&self) -> usize {
        self.scopes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

impl PackageArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn alloc(&mut self, data: Package) -> PackageId {
        self.packages.push(data);
        let raw = self.packages.len() as u32;
        PackageId(NonZeroU32::new(raw).expect("arena index never 0"))
    }

    pub fn get(&self, id: PackageId) -> &Package {
        &self.packages[(id.0.get() - 1) as usize]
    }

    pub fn get_mut(&mut self, id: PackageId) -> &mut Package {
        &mut self.packages[(id.0.get() - 1) as usize]
    }

    pub fn len(&self) -> usize {
        self.packages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Return the package id at `index` (0-based arena position).
    pub fn id_at(&self, index: usize) -> PackageId {
        PackageId(NonZeroU32::new((index + 1) as u32).expect("arena index never 0"))
    }

    /// Look up a package by its import path.
    pub fn find_by_path(&self, path: &str) -> Option<PackageId> {
        (0..self.len()).find_map(|i| {
            let id = self.id_at(i);
            (self.get(id).path() == path).then_some(id)
        })
    }
}
