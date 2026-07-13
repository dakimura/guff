// Port of Go's go/ast/scope.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// As in Go, this module is deprecated — `go/types` provides the
// supported object-resolution machinery. It's ported for parity with
// the upstream surface and so that `resolve.rs` has something to build
// on.
//
// Notable shape changes for Rust:
//
// * `*Object` becomes `Arc<Object>`; identity comparisons use
//   `Arc::ptr_eq`.
// * Go's `Decl any` becomes a closed [`ObjDecl`] enum that owns a
//   *clone* of the originating AST node (`Field`, `ImportSpec`, etc.).
//   Cloning avoids cyclic references between an `Ident.obj` pointer and
//   the AST that declared it; the trade-off is a little memory.
// * `Data any` becomes [`ObjData`] — `Scope` (used by `Pkg` objects)
//   or `Int` (used by `Con` objects for the iota value).
// * Scope mutation goes through a `Mutex<HashMap<…>>` so an
//   `Arc<Scope>` is safe to share.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::ast::{AssignStmt, Expr, Field, FuncDecl, ImportSpec, LabeledStmt, TypeSpec, ValueSpec};
use crate::position::{Pos, NO_POS};

// ====================================================================
// ObjKind
// ====================================================================

/// What an [`Object`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjKind {
    /// For error handling.
    Bad,
    /// Package.
    Pkg,
    /// Constant.
    Con,
    /// Type.
    Typ,
    /// Variable.
    Var,
    /// Function or method.
    Fun,
    /// Label.
    Lbl,
}

impl ObjKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ObjKind::Bad => "bad",
            ObjKind::Pkg => "package",
            ObjKind::Con => "const",
            ObjKind::Typ => "type",
            ObjKind::Var => "var",
            ObjKind::Fun => "func",
            ObjKind::Lbl => "label",
        }
    }
}

impl fmt::Display for ObjKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ====================================================================
// Object / ObjDecl / ObjData
// ====================================================================

/// The kinds of declarations an [`Object`] can point at. Each variant
/// owns a clone of the original AST node so that walking back via
/// `obj.pos()` is self-contained.
#[derive(Debug, Clone, Default)]
pub enum ObjDecl {
    /// No declaration recorded.
    #[default]
    None,
    Field(Box<Field>),
    ImportSpec(Box<ImportSpec>),
    ValueSpec(Box<ValueSpec>),
    TypeSpec(Box<TypeSpec>),
    FuncDecl(Box<FuncDecl>),
    LabeledStmt(Box<LabeledStmt>),
    AssignStmt(Box<AssignStmt>),
    /// Predeclared scope (universe). [`Object::pos`] yields [`NO_POS`].
    Scope(Arc<Scope>),
}

/// Object-specific data field. Matches the small set of types Go's
/// `ast` package actually stores under `Object.Data`.
#[derive(Debug, Clone, Default)]
pub enum ObjData {
    #[default]
    None,
    /// Package scope (set for [`ObjKind::Pkg`] objects).
    Scope(Arc<Scope>),
    /// `iota` value (set for [`ObjKind::Con`] objects).
    Int(i64),
}

/// A named language entity: package, const, type, var, func, or label.
///
/// Deprecated like Go's `ast.Object` — see [`crate::scope`] module docs.
#[derive(Debug, Clone)]
pub struct Object {
    pub kind: ObjKind,
    pub name: String,
    pub decl: ObjDecl,
    pub data: ObjData,
}

impl Object {
    /// Equivalent of Go's `ast.NewObj`.
    pub fn new(kind: ObjKind, name: impl Into<String>) -> Arc<Self> {
        Arc::new(Object {
            kind,
            name: name.into(),
            decl: ObjDecl::None,
            data: ObjData::None,
        })
    }

    /// Position of the declaration of this object's name (best-effort).
    /// Returns [`NO_POS`] when no source position can be derived.
    pub fn pos(&self) -> Pos {
        let name = self.name.as_str();
        match &self.decl {
            ObjDecl::None => NO_POS,
            ObjDecl::Field(d) => {
                for n in &d.names {
                    if n.name == name {
                        return n.pos();
                    }
                }
                NO_POS
            }
            ObjDecl::ImportSpec(d) => {
                if let Some(n) = &d.name {
                    if n.name == name {
                        return n.pos();
                    }
                }
                d.path.pos()
            }
            ObjDecl::ValueSpec(d) => {
                for n in &d.names {
                    if n.name == name {
                        return n.pos();
                    }
                }
                NO_POS
            }
            ObjDecl::TypeSpec(d) => {
                if d.name.name == name {
                    return d.name.pos();
                }
                NO_POS
            }
            ObjDecl::FuncDecl(d) => {
                if d.name.name == name {
                    return d.name.pos();
                }
                NO_POS
            }
            ObjDecl::LabeledStmt(d) => {
                if d.label.name == name {
                    return d.label.pos();
                }
                NO_POS
            }
            ObjDecl::AssignStmt(d) => {
                for x in &d.lhs {
                    if let Expr::Ident(id) = x {
                        if id.name == name {
                            return id.pos();
                        }
                    }
                }
                NO_POS
            }
            ObjDecl::Scope(_) => NO_POS,
        }
    }
}

// ====================================================================
// Scope
// ====================================================================

/// Set of named entities, with a link to the surrounding scope.
///
/// Deprecated like Go's `ast.Scope`.
#[derive(Debug)]
pub struct Scope {
    /// Immediately surrounding scope, if any.
    pub outer: Mutex<Option<Arc<Scope>>>,
    objects: Mutex<HashMap<String, Arc<Object>>>,
}

impl Scope {
    /// Create a new scope nested in `outer`.
    pub fn new(outer: Option<Arc<Scope>>) -> Arc<Self> {
        Arc::new(Scope {
            outer: Mutex::new(outer),
            objects: Mutex::new(HashMap::with_capacity(4)),
        })
    }

    /// Return the object with the given name, if present in *this*
    /// scope (outer scopes are not consulted).
    pub fn lookup(&self, name: &str) -> Option<Arc<Object>> {
        self.objects.lock().unwrap().get(name).cloned()
    }

    /// Insert `obj`. If an object with the same name already exists,
    /// the scope is left unchanged and that existing object is returned.
    pub fn insert(&self, obj: Arc<Object>) -> Option<Arc<Object>> {
        let mut m = self.objects.lock().unwrap();
        if let Some(alt) = m.get(&obj.name) {
            return Some(Arc::clone(alt));
        }
        m.insert(obj.name.clone(), obj);
        None
    }

    /// Snapshot of all objects currently in the scope, name+object.
    pub fn objects(&self) -> Vec<(String, Arc<Object>)> {
        self.objects
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), Arc::clone(v)))
            .collect()
    }

    /// Number of objects currently in the scope.
    pub fn len(&self) -> usize {
        self.objects.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Swap the outer-scope link. Mirrors Go's `scope.Outer = …`
    /// assignments in `resolve.go`.
    pub fn set_outer(&self, outer: Option<Arc<Scope>>) {
        *self.outer.lock().unwrap() = outer;
    }

    /// Borrow of the current outer-scope link (clones the `Arc`).
    pub fn outer(&self) -> Option<Arc<Scope>> {
        self.outer.lock().unwrap().clone()
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Mirror Go's "scope %p {…}\n" form. The address gives test
        // output a stable look (only the literal varies between runs).
        let ptr = self as *const Scope;
        write!(f, "scope {:p} {{", ptr)?;
        let objs = self.objects.lock().unwrap();
        if !objs.is_empty() {
            writeln!(f)?;
            // Iteration order isn't fixed by HashMap; Go's `for _, obj
            // := range s.Objects` is similarly unordered. Sort here to
            // make output predictable for tests.
            let mut items: Vec<(&String, &Arc<Object>)> = objs.iter().collect();
            items.sort_by(|a, b| a.0.cmp(b.0));
            for (_, obj) in items {
                writeln!(f, "\t{} {}", obj.kind, obj.name)?;
            }
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BasicLit, Ident};
    use crate::position::Pos;
    use crate::token::Token;

    #[test]
    fn objkind_display() {
        assert_eq!(ObjKind::Bad.to_string(), "bad");
        assert_eq!(ObjKind::Pkg.to_string(), "package");
        assert_eq!(ObjKind::Con.to_string(), "const");
        assert_eq!(ObjKind::Typ.to_string(), "type");
        assert_eq!(ObjKind::Var.to_string(), "var");
        assert_eq!(ObjKind::Fun.to_string(), "func");
        assert_eq!(ObjKind::Lbl.to_string(), "label");
    }

    #[test]
    fn lookup_returns_none_for_missing() {
        let scope = Scope::new(None);
        assert!(scope.lookup("x").is_none());
    }

    #[test]
    fn insert_then_lookup_returns_same_arc() {
        let scope = Scope::new(None);
        let obj = Object::new(ObjKind::Var, "x");
        let alt = scope.insert(Arc::clone(&obj));
        assert!(alt.is_none(), "fresh insert returns no conflict");
        let got = scope.lookup("x").unwrap();
        assert!(Arc::ptr_eq(&got, &obj), "same object instance returned");
    }

    #[test]
    fn duplicate_insert_returns_existing_and_leaves_scope_unchanged() {
        let scope = Scope::new(None);
        let first = Object::new(ObjKind::Var, "x");
        let second = Object::new(ObjKind::Var, "x");
        assert!(scope.insert(Arc::clone(&first)).is_none());
        let alt = scope.insert(Arc::clone(&second)).expect("conflict");
        assert!(Arc::ptr_eq(&alt, &first), "alt is the original object");
        let after = scope.lookup("x").unwrap();
        assert!(Arc::ptr_eq(&after, &first), "scope unchanged");
    }

    #[test]
    fn outer_scopes_are_not_searched_by_lookup() {
        let outer = Scope::new(None);
        outer.insert(Object::new(ObjKind::Var, "x"));
        let inner = Scope::new(Some(Arc::clone(&outer)));
        // Inner doesn't see outer's objects via lookup.
        assert!(inner.lookup("x").is_none());
        // But outer link is preserved.
        let outer_link = inner.outer().expect("link present");
        assert!(Arc::ptr_eq(&outer_link, &outer));
    }

    #[test]
    fn set_outer_replaces_link() {
        let a = Scope::new(None);
        let b = Scope::new(None);
        let inner = Scope::new(Some(Arc::clone(&a)));
        inner.set_outer(Some(Arc::clone(&b)));
        let now = inner.outer().unwrap();
        assert!(Arc::ptr_eq(&now, &b));
    }

    #[test]
    fn object_pos_field_matches_name() {
        let field = Field {
            doc: None,
            names: vec![Ident {
                name_pos: Pos(42),
                name: "x".to_string(),
                ..Default::default()
            }],
            ty: None,
            tag: None,
            comment: None,
            id: 0,
        };
        let mut obj = (*Object::new(ObjKind::Var, "x")).clone();
        obj.decl = ObjDecl::Field(Box::new(field));
        assert_eq!(obj.pos(), Pos(42));
    }

    #[test]
    fn object_pos_import_spec_path_when_no_name() {
        let spec = ImportSpec {
            doc: None,
            name: None,
            path: BasicLit {
                id: 0,
                value_pos: Pos(7),
                value_end: Pos(0),
                kind: Some(Token::STRING),
                value: "\"x\"".to_string(),
            },
            comment: None,
            end_pos: Pos(0),
            id: 0,
        };
        let mut obj = (*Object::new(ObjKind::Pkg, "x")).clone();
        obj.decl = ObjDecl::ImportSpec(Box::new(spec));
        assert_eq!(obj.pos(), Pos(7));
    }

    #[test]
    fn object_pos_scope_decl_is_no_pos() {
        let scope = Scope::new(None);
        let mut obj = (*Object::new(ObjKind::Bad, "universe")).clone();
        obj.decl = ObjDecl::Scope(scope);
        assert_eq!(obj.pos(), NO_POS);
    }

    #[test]
    fn display_format_matches_go_shape() {
        let scope = Scope::new(None);
        scope.insert(Object::new(ObjKind::Var, "y"));
        scope.insert(Object::new(ObjKind::Fun, "x"));
        let s = scope.to_string();
        // Sorted by name: "x" then "y".
        assert!(s.starts_with("scope 0x"));
        assert!(s.contains("\tfunc x\n"));
        assert!(s.contains("\tvar y\n"));
        assert!(s.ends_with("}\n"));
        // Empty scope: no inner lines, single `{}`.
        let empty = Scope::new(None);
        let es = empty.to_string();
        assert!(es.ends_with("{}\n"));
    }
}
