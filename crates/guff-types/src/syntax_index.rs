//! Pointer index from stamped [`guff::NodeId`] into `Checker::files` (C-1 Phase 2).
//!
//! DeclInfo stores [`NodeId`] handles instead of owned AST clones. Lookups go
//! through this index, which holds `NonNull` pointers into the package's
//! `files` vector. The AST is not mutated for the duration of typechecking, so
//! the pointers remain valid until `files` is replaced or dropped.

use std::collections::HashMap;
use std::ptr::NonNull;

use guff::ast::{BlockStmt, Expr, FuncDecl, TypeSpec};
use guff::NodeId;

/// Maps stamped node ids to locations inside `Checker::files`.
#[derive(Debug, Default)]
pub struct SyntaxIndex {
    exprs: HashMap<NodeId, NonNull<Expr>>,
    type_specs: HashMap<NodeId, NonNull<TypeSpec>>,
    func_decls: HashMap<NodeId, NonNull<FuncDecl>>,
    blocks: HashMap<NodeId, NonNull<BlockStmt>>,
}

// SAFETY: SyntaxIndex is only used from a single Checker on one thread during
// typecheck; the HashMap itself is not shared across threads.
unsafe impl Send for SyntaxIndex {}
unsafe impl Sync for SyntaxIndex {}

impl SyntaxIndex {
    pub fn clear(&mut self) {
        self.exprs.clear();
        self.type_specs.clear();
        self.func_decls.clear();
        self.blocks.clear();
    }

    pub fn insert_expr(&mut self, e: &Expr) {
        if let Some(id) = NodeId::from_u32(e.id()) {
            self.exprs.insert(id, NonNull::from(e));
        }
    }

    pub fn insert_type_spec(&mut self, ts: &TypeSpec) {
        if let Some(id) = NodeId::from_u32(ts.id) {
            self.type_specs.insert(id, NonNull::from(ts));
        }
        self.insert_expr(&ts.ty);
    }

    pub fn insert_func_decl(&mut self, fd: &FuncDecl) {
        // Key funcs by FuncType id (always stamped; FuncDecl itself has no id).
        if let Some(id) = NodeId::from_u32(fd.ty.id) {
            self.func_decls.insert(id, NonNull::from(fd));
        }
        if let Some(body) = &fd.body {
            if let Some(id) = NodeId::from_u32(body.id) {
                self.blocks.insert(id, NonNull::from(body));
            }
        }
    }

    pub fn expr(&self, id: NodeId) -> Option<&Expr> {
        self.exprs.get(&id).map(|p| unsafe { p.as_ref() })
    }

    pub fn type_spec(&self, id: NodeId) -> Option<&TypeSpec> {
        self.type_specs.get(&id).map(|p| unsafe { p.as_ref() })
    }

    pub fn func_decl(&self, id: NodeId) -> Option<&FuncDecl> {
        self.func_decls.get(&id).map(|p| unsafe { p.as_ref() })
    }

    pub fn block(&self, id: NodeId) -> Option<&BlockStmt> {
        self.blocks.get(&id).map(|p| unsafe { p.as_ref() })
    }
}
