//! Port of `cmd/compile/internal/types2/object.go`.
//!
//! Chunk 1 only contains the minimum needed by `Tuple` — a stub `Var`. The
//! full `Object` API (Parent/Pos/Pkg/Exported/Id, the seven object kinds,
//! `objset`, etc.) arrives with the Scope/Package chunk.

pub mod builtin;
pub mod const_;
pub mod func;
pub mod nil_;
pub mod pkgname;
pub mod type_name;
pub mod var;

use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, ScopeId};

/// Common metadata shared by all `Object` kinds — the equivalent of Go's
/// embedded `object` struct's housekeeping fields.
///
/// Positions are `u32` for now (chunk 7 keeps it simple); `0` means
/// `nopos`. Full `syntax.Pos` integration arrives when the Checker is
/// wired up.
#[derive(Debug, Clone, Default)]
pub struct ObjectMeta {
    pub parent: Option<ScopeId>,
    pub pkg: Option<PackageId>,
    pub pos: u32,
    pub order: u32,
    pub scope_pos: u32,
}

/// Internal trait so the `ObjectId` dispatch can grab `&ObjectMeta` /
/// `&mut ObjectMeta` from any variant without per-variant boilerplate at
/// every call site.
pub(crate) trait HasMeta {
    fn meta(&self) -> &ObjectMeta;
    fn meta_mut(&mut self) -> &mut ObjectMeta;
}

/// Returns `true` if `name` begins with an uppercase letter — Go's
/// definition of "exported".
///
/// Equivalent to `types2.isExported`.
pub fn is_exported(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_uppercase())
}

/// Returns `name` if exported, otherwise `path.name` where `path` is the
/// owning package's import path (`"_"` if `pkg` is `None`).
///
/// Equivalent to `types2.Id`.
pub fn id(pkg_arena: &PackageArena, pkg: Option<PackageId>, name: &str) -> String {
    if is_exported(name) {
        return name.to_string();
    }
    let path = match pkg {
        Some(p) if !pkg_arena.get(p).path().is_empty() => pkg_arena.get(p).path().to_string(),
        _ => "_".to_string(),
    };
    format!("{}.{}", path, name)
}

impl ObjectId {
    /// Returns the object's source-declared name.
    pub fn name<'a>(self, arena: &'a ObjectArena) -> &'a str {
        match arena.get(self) {
            ObjectData::Var(v) => v.name(),
            ObjectData::Func(f) => f.name(),
            ObjectData::TypeName(tn) => tn.name(),
            ObjectData::Const(c) => c.name(),
            ObjectData::Nil(n) => n.name(),
            ObjectData::Builtin(b) => b.name(),
            ObjectData::PkgName(p) => p.name(),
        }
    }

    /// Returns the object's type. For [`func::Func`] and
    /// [`type_name::TypeName`], this may be `None` during two-phase
    /// construction (Go's `NewFunc(.., nil)` / `NewTypeName(.., nil)`); for
    /// other object kinds the type is always set.
    pub fn typ(self, arena: &ObjectArena) -> Option<crate::arena::TypeId> {
        match arena.get(self) {
            ObjectData::Var(v) => Some(v.typ()),
            ObjectData::Func(f) => f.typ(),
            ObjectData::TypeName(tn) => tn.typ(),
            ObjectData::Const(c) => Some(c.typ()),
            ObjectData::Nil(n) => Some(n.typ()),
            ObjectData::Builtin(b) => Some(b.typ()),
            ObjectData::PkgName(p) => Some(p.typ()),
        }
    }

    fn meta<'a>(self, arena: &'a ObjectArena) -> &'a ObjectMeta {
        match arena.get(self) {
            ObjectData::Var(v) => v.meta(),
            ObjectData::Func(f) => f.meta(),
            ObjectData::TypeName(tn) => tn.meta(),
            ObjectData::Const(c) => c.meta(),
            ObjectData::Nil(n) => n.meta(),
            ObjectData::Builtin(b) => b.meta(),
            ObjectData::PkgName(p) => p.meta(),
        }
    }

    fn meta_mut<'a>(self, arena: &'a mut ObjectArena) -> &'a mut ObjectMeta {
        match arena.get_mut(self) {
            ObjectData::Var(v) => v.meta_mut(),
            ObjectData::Func(f) => f.meta_mut(),
            ObjectData::TypeName(tn) => tn.meta_mut(),
            ObjectData::Const(c) => c.meta_mut(),
            ObjectData::Nil(n) => n.meta_mut(),
            ObjectData::Builtin(b) => b.meta_mut(),
            ObjectData::PkgName(p) => p.meta_mut(),
        }
    }

    pub fn parent(self, arena: &ObjectArena) -> Option<ScopeId> {
        self.meta(arena).parent
    }

    pub fn set_parent(self, arena: &mut ObjectArena, parent: ScopeId) {
        self.meta_mut(arena).parent = Some(parent);
    }

    pub fn pkg(self, arena: &ObjectArena) -> Option<PackageId> {
        self.meta(arena).pkg
    }

    pub fn set_pkg(self, arena: &mut ObjectArena, pkg: PackageId) {
        self.meta_mut(arena).pkg = Some(pkg);
    }

    pub fn pos(self, arena: &ObjectArena) -> u32 {
        self.meta(arena).pos
    }

    /// Sets the object's source declaration position (a byte offset into the
    /// `FileSet`; `0` means `nopos`).
    ///
    /// Go passes the position to the object constructor (`NewVar(pos, ..)` and
    /// friends); our constructors default it to `nopos`, so the checker calls
    /// `set_pos` at each declaration site once the declaring identifier is in
    /// hand. Part of the D07 position-integration work.
    pub fn set_pos(self, arena: &mut ObjectArena, pos: u32) {
        self.meta_mut(arena).pos = pos;
    }

    pub fn order(self, arena: &ObjectArena) -> u32 {
        self.meta(arena).order
    }

    pub fn set_order(self, arena: &mut ObjectArena, order: u32) {
        assert!(order > 0, "order must be > 0");
        self.meta_mut(arena).order = order;
    }

    pub fn scope_pos(self, arena: &ObjectArena) -> u32 {
        self.meta(arena).scope_pos
    }

    pub fn set_scope_pos(self, arena: &mut ObjectArena, pos: u32) {
        self.meta_mut(arena).scope_pos = pos;
    }

    /// Reports whether the object's name starts with an uppercase letter.
    pub fn exported(self, arena: &ObjectArena) -> bool {
        is_exported(self.name(arena))
    }

    /// Returns the object's identifier — `name` if exported, otherwise
    /// `pkg_path.name`. Matches Go's `Object.Id()`.
    pub fn id(self, arena: &ObjectArena, pkg_arena: &PackageArena) -> String {
        let nm = self.name(arena).to_string();
        let pkg = self.pkg(arena);
        id(pkg_arena, pkg, &nm)
    }

    /// Reports whether `self.id()` equals `id(pkg, name)`. If `fold_case`
    /// is true, case-insensitive name comparison ignores packages.
    ///
    /// Equivalent to `Object.sameId` — used by the spec's "two identifiers
    /// are different if they ... appear in different packages and are not
    /// exported" rule.
    pub fn same_id(
        self,
        arena: &ObjectArena,
        pkg_arena: &PackageArena,
        pkg: Option<PackageId>,
        name: &str,
        fold_case: bool,
    ) -> bool {
        let self_name = self.name(arena);
        if fold_case && self_name.eq_ignore_ascii_case(name) {
            return true;
        }
        if self_name != name {
            return false;
        }
        if is_exported(self_name) {
            return true;
        }
        same_pkg_helper(pkg_arena, self.pkg(arena), pkg)
    }
}

/// Compare two objects in the canonical Go order: exported before
/// unexported, then by name, then (for unexported) by package path.
///
/// Equivalent to `types2.object.cmp`. Used to sort method lists for
/// stable identity comparisons (see `Interface.typeSet`).
pub fn cmp(
    arena: &ObjectArena,
    pkg_arena: &PackageArena,
    a: ObjectId,
    b: ObjectId,
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a == b {
        return Ordering::Equal;
    }
    let a_name = arena.get(a);
    let b_name = arena.get(b);
    let a_name = match a_name {
        ObjectData::Var(v) => v.name(),
        ObjectData::Func(f) => f.name(),
        ObjectData::TypeName(t) => t.name(),
        ObjectData::Const(c) => c.name(),
        ObjectData::Nil(n) => n.name(),
        ObjectData::Builtin(bi) => bi.name(),
        ObjectData::PkgName(p) => p.name(),
    };
    let b_name = match b_name {
        ObjectData::Var(v) => v.name(),
        ObjectData::Func(f) => f.name(),
        ObjectData::TypeName(t) => t.name(),
        ObjectData::Const(c) => c.name(),
        ObjectData::Nil(n) => n.name(),
        ObjectData::Builtin(bi) => bi.name(),
        ObjectData::PkgName(p) => p.name(),
    };
    let ea = is_exported(a_name);
    let eb = is_exported(b_name);
    if ea != eb {
        return if ea {
            Ordering::Less
        } else {
            Ordering::Greater
        };
    }
    match a_name.cmp(b_name) {
        Ordering::Equal => {}
        ord => return ord,
    }
    if !ea {
        // Compare package paths. Empty / missing package treated as the
        // empty string — keeps the ordering stable.
        let a_path = a
            .pkg(arena)
            .map(|p| pkg_arena.get(p).path().to_string())
            .unwrap_or_default();
        let b_path = b
            .pkg(arena)
            .map(|p| pkg_arena.get(p).path().to_string())
            .unwrap_or_default();
        return a_path.cmp(&b_path);
    }
    Ordering::Equal
}

/// Internal: package-equality with Go semantics (both `None` is "same",
/// otherwise paths must match).
fn same_pkg_helper(pkg_arena: &PackageArena, a: Option<PackageId>, b: Option<PackageId>) -> bool {
    match (a, b) {
        (None, None) => true,
        (None, _) | (_, None) => false,
        (Some(x), Some(y)) => {
            if x == y {
                return true;
            }
            pkg_arena.get(x).path() == pkg_arena.get(y).path()
        }
    }
}
