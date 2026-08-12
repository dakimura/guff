//! Port of `cmd/compile/internal/types2/scope.go`.
//!
//! Chunk-7 notes:
//! - **`pos`/`end`** are plain `u32`s; `0` means `nopos`.
//! - **`lazyObject`** isn't ported — used only by exporter/importer
//!   infrastructure which we haven't reached.
//! - **`Universe.Lookup` hijack for `any`** (gotypesalias legacy, D04) isn't
//!   ported; we always return whatever's stored. Go 1.22+ always enables
//!   aliases, so this hijack is obsolete for our supported language levels.
//! - **`WriteTo` / `Display`** are deferred.

use crate::hash::HashMap;

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectArena, ObjectId, ScopeArena, ScopeId};
use crate::object::is_exported;

/// A scope holds a set of named objects and links to its parent and
/// children. Objects may be inserted and looked up by name.
///
/// Equivalent to `types2.Scope`.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Scope {
    parent: Option<ScopeId>,
    children: Vec<ScopeId>,
    /// `parent.children[number - 1]` is this scope; `0` if root or detached.
    number: u32,
    elems: HashMap<String, ObjectId>,
    pos: u32,
    end: u32,
    comment: String,
    is_func: bool,
}

impl Scope {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.parent = r.scope_opt(self.parent);
        for c in &mut self.children {
            *c = r.scope(*c);
        }
        for obj in self.elems.values_mut() {
            *obj = r.obj(*obj);
        }
    }
}

impl Scope {
    pub fn parent(&self) -> Option<ScopeId> {
        self.parent
    }

    pub fn len(&self) -> usize {
        self.elems.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }

    pub fn num_children(&self) -> usize {
        self.children.len()
    }

    pub fn child(&self, i: usize) -> ScopeId {
        self.children[i]
    }

    pub fn pos(&self) -> u32 {
        self.pos
    }

    pub fn end(&self) -> u32 {
        self.end
    }

    pub fn comment(&self) -> &str {
        &self.comment
    }

    pub fn is_func(&self) -> bool {
        self.is_func
    }

    /// Mark this scope as a function-body scope.
    pub fn set_is_func(&mut self, v: bool) {
        self.is_func = v;
    }

    /// Clear FileSet-absolute `pos`/`end` to `nopos` (0). Used before
    /// persisting a seed overlay across process runs.
    pub fn clear_positions(&mut self) {
        self.pos = 0;
        self.end = 0;
    }

    /// Sorted list of element names (matches Go's `Scope.Names`).
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.elems.keys().cloned().collect();
        names.sort();
        names
    }

    /// Direct (non-recursive) lookup. Returns `None` if no element by that
    /// name lives in this scope.
    pub fn lookup_local(&self, name: &str) -> Option<ObjectId> {
        self.elems.get(name).copied()
    }
}

/// Construct a new scope contained in `parent` (if any).
///
/// Equivalent to `types2.NewScope`. The new scope is appended to the
/// parent's children list unless `parent` is `None` or the universe scope
/// (matching Go's exception that universe-children aren't tracked).
///
/// `universe_scope` should be the canonical universe `ScopeId`; pass `None`
/// when constructing the universe itself.
pub fn new_scope(
    arena: &mut ScopeArena,
    parent: Option<ScopeId>,
    universe_scope: Option<ScopeId>,
    pos: u32,
    end: u32,
    comment: impl Into<String>,
) -> ScopeId {
    let id = arena.alloc(Scope {
        parent,
        children: Vec::new(),
        number: 0,
        elems: HashMap::default(),
        pos,
        end,
        comment: comment.into(),
        is_func: false,
    });
    // Wire the parent's children list — but not under the universe.
    if let Some(p) = parent {
        let is_under_universe = universe_scope == Some(p);
        if !is_under_universe {
            let parent_scope = arena.get_mut(p);
            parent_scope.children.push(id);
            let number = parent_scope.children.len() as u32;
            arena.get_mut(id).number = number;
        }
    }
    id
}

/// Lookup `name` in `scope`. Does **not** walk to parent — call
/// [`lookup_chain`] for that.
///
/// Equivalent to `types2.Scope.Lookup` (minus the obsolete `any`-hijack, D04).
pub fn lookup(arena: &ScopeArena, scope: ScopeId, name: &str) -> Option<ObjectId> {
    arena.get(scope).lookup_local(name)
}

/// Returns objects in `scope` whose names match `name` ignoring case.
/// If `exported` is set, only exported names are returned.
///
/// Equivalent to `types2.Scope.lookupIgnoringCase` (D03).
pub fn lookup_ignoring_case(
    arena: &ScopeArena,
    scope: ScopeId,
    name: &str,
    exported: bool,
) -> Vec<ObjectId> {
    let mut matches = Vec::new();
    for n in arena.get(scope).names() {
        if (!exported || is_exported(&n)) && n.eq_ignore_ascii_case(name) {
            if let Some(obj) = lookup(arena, scope, &n) {
                matches.push(obj);
            }
        }
    }
    matches
}

/// Walk from `scope` toward the universe, returning the first matching
/// `ObjectId`. Returns `None` if no scope in the chain holds `name`.
pub fn lookup_chain(arena: &ScopeArena, mut scope: ScopeId, name: &str) -> Option<ObjectId> {
    loop {
        if let Some(obj) = arena.get(scope).lookup_local(name) {
            return Some(obj);
        }
        match arena.get(scope).parent {
            Some(p) => scope = p,
            None => return None,
        }
    }
}

/// [`lookup_chain`], but also returns the scope the name was found in — the
/// key half of Go's `dotImportKey{scope, name}`.
pub fn lookup_chain_scope(
    arena: &ScopeArena,
    mut scope: ScopeId,
    name: &str,
) -> Option<(ScopeId, ObjectId)> {
    loop {
        if let Some(obj) = arena.get(scope).lookup_local(name) {
            return Some((scope, obj));
        }
        match arena.get(scope).parent {
            Some(p) => scope = p,
            None => return None,
        }
    }
}

/// Insert `obj` into `scope`. If the scope already contains an object with
/// the same name, returns that alternative and leaves the scope unchanged.
/// Otherwise inserts, sets the inserted object's `parent` to `scope` if
/// not already set, and returns `None`.
///
/// Equivalent to `types2.Scope.Insert`.
pub fn insert(
    scope_arena: &mut ScopeArena,
    object_arena: &mut ObjectArena,
    scope: ScopeId,
    obj: ObjectId,
) -> Option<ObjectId> {
    let name = obj.name(object_arena).to_string();
    if let Some(alt) = scope_arena.get(scope).lookup_local(&name) {
        return Some(alt);
    }
    scope_arena.get_mut(scope).elems.insert(name, obj);
    // Back-fill the object's parent scope if not yet set.
    if obj.parent(object_arena).is_none() {
        obj.set_parent(object_arena, scope);
    }
    None
}

/// Insert `obj` into `scope` under `name` without modifying `obj` — in
/// particular without setting its parent. Used for dot-imports, where the
/// object belongs to another package and must not be reparented (Go inserts
/// straight into `fileScope.elems` for the same reason, see go.dev/issue/32154).
/// Returns the existing object on a name collision, leaving the scope unchanged.
pub fn insert_no_reparent(
    scope_arena: &mut ScopeArena,
    scope: ScopeId,
    name: &str,
    obj: ObjectId,
) -> Option<ObjectId> {
    if let Some(alt) = scope_arena.get(scope).lookup_local(name) {
        return Some(alt);
    }
    scope_arena
        .get_mut(scope)
        .elems
        .insert(name.to_string(), obj);
    None
}
