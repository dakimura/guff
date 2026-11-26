//! Index-addressed AST handles and a package-local arena (C-1).
//!
//! # Why
//!
//! The live AST is still an owned `Box`/`Vec`/`String` tree ([`crate::ast`]).
//! Cross-references (typecheck `Info`, and as of C-1 Phase 2 `DeclInfo`) key on
//! stable [`NodeId`] values that share the process-wide stamp space from
//! [`crate::ast::next_node_id`]. Phase 3 (parser-direct arena alloc) was
//! cancelled under the C-1 conditional-GO gate; this module still provides the
//! handle type and a small arena used by round-trip tests so the storage shape
//! is ready if that work is ever resumed.

use std::num::NonZeroU32;

use crate::ast::Ident;
use crate::Pos;

/// Stable handle for an AST node.
///
/// Same id space as stamped [`crate::ast::Ident::id`] / expression ids
/// (`0` is reserved for synthetic / unstamped nodes and is not a valid
/// [`NodeId`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// Wrap a stamped nonzero id. Returns `None` for `0`.
    #[inline]
    pub fn from_u32(id: u32) -> Option<Self> {
        NonZeroU32::new(id).map(NodeId)
    }

    /// The underlying stamp id.
    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0.get()
    }
}

/// A contiguous slice of [`NodeId`]s stored in [`AstArena::lists`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeListId {
    pub start: u32,
    pub len: u32,
}

impl NodeListId {
    pub const EMPTY: Self = Self { start: 0, len: 0 };
}

/// Flattened AST node. Children are [`NodeId`] / [`NodeListId`].
///
/// Phase 1 ships the subset needed to round-trip a tiny `package` + `func`
/// file in tests. Remaining kinds stay on the owned tree until/unless Phase 3
/// resumes.
#[derive(Debug, Clone)]
pub enum AstNode {
    File {
        name: NodeId,
        decls: NodeListId,
        package: Pos,
        file_start: Pos,
        file_end: Pos,
        go_version: String,
        id: u32,
    },
    Ident {
        name: String,
        name_pos: Pos,
        id: u32,
    },
    FuncDecl {
        name: NodeId,
        ty: NodeId,
        body: Option<NodeId>,
    },
    FuncType {
        func: Pos,
        params: Option<NodeId>,
        results: Option<NodeId>,
        id: u32,
    },
    FieldList {
        opening: Pos,
        closing: Pos,
    },
    BlockStmt {
        lbrace: Pos,
        rbrace: Pos,
        id: u32,
    },
}

/// Package-local arena: `Vec` storage + typed ids (no Layered CoW — hybrid drops
/// dep ASTs early, so sharing frozen dep arenas is not required for the MVP).
#[derive(Debug, Default, Clone)]
pub struct AstArena {
    /// 1-indexed; slot 0 is unused so [`NodeId`] can be `NonZeroU32`.
    nodes: Vec<AstNode>,
    lists: Vec<NodeId>,
}

impl AstArena {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            lists: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn alloc(&mut self, node: AstNode) -> NodeId {
        self.nodes.push(node);
        let idx = self.nodes.len(); // 1-based
        NodeId(NonZeroU32::new(idx as u32).expect("arena index fits NonZeroU32"))
    }

    pub fn alloc_list(&mut self, ids: &[NodeId]) -> NodeListId {
        if ids.is_empty() {
            return NodeListId::EMPTY;
        }
        let start = self.lists.len() as u32;
        self.lists.extend_from_slice(ids);
        NodeListId {
            start,
            len: ids.len() as u32,
        }
    }

    pub fn get(&self, id: NodeId) -> &AstNode {
        &self.nodes[id.as_u32() as usize - 1]
    }

    pub fn get_mut(&mut self, id: NodeId) -> &mut AstNode {
        &mut self.nodes[id.as_u32() as usize - 1]
    }

    pub fn list(&self, id: NodeListId) -> &[NodeId] {
        if id.len == 0 {
            return &[];
        }
        let start = id.start as usize;
        let end = start + id.len as usize;
        &self.lists[start..end]
    }
}

/// Convert a minimal owned [`crate::ast::File`] into an [`AstArena`].
///
/// Supports: package clause, `func` decls with empty recv, empty params/results,
/// and an optional empty block body. Used by the Phase 1 round-trip test — not
/// the production typecheck path.
pub fn file_to_arena(file: &crate::ast::File) -> (AstArena, NodeId) {
    use crate::ast::Decl;

    let mut arena = AstArena::new();

    fn alloc_ident(arena: &mut AstArena, id: &Ident) -> NodeId {
        arena.alloc(AstNode::Ident {
            name: id.name.clone(),
            name_pos: id.name_pos,
            id: id.id,
        })
    }

    fn alloc_field_list(arena: &mut AstArena, fl: &crate::ast::FieldList) -> NodeId {
        assert!(
            fl.list.is_empty(),
            "Phase 1 file_to_arena only supports empty FieldLists"
        );
        arena.alloc(AstNode::FieldList {
            opening: fl.opening,
            closing: fl.closing,
        })
    }

    fn alloc_func_type(arena: &mut AstArena, ty: &crate::ast::FuncType) -> NodeId {
        assert!(
            ty.type_params.is_none(),
            "Phase 1 file_to_arena does not support type params"
        );
        let params = ty.params.as_ref().map(|fl| alloc_field_list(arena, fl));
        let results = ty.results.as_ref().map(|fl| alloc_field_list(arena, fl));
        arena.alloc(AstNode::FuncType {
            func: ty.func,
            params,
            results,
            id: ty.id,
        })
    }

    fn alloc_block(arena: &mut AstArena, body: &crate::ast::BlockStmt) -> NodeId {
        assert!(
            body.list.is_empty(),
            "Phase 1 file_to_arena only supports empty function bodies"
        );
        arena.alloc(AstNode::BlockStmt {
            lbrace: body.lbrace,
            rbrace: body.rbrace,
            id: body.id,
        })
    }

    fn alloc_func_decl(arena: &mut AstArena, fd: &crate::ast::FuncDecl) -> NodeId {
        assert!(fd.recv.is_none(), "Phase 1: methods not supported");
        let name = alloc_ident(arena, &fd.name);
        let ty = alloc_func_type(arena, &fd.ty);
        let body = fd.body.as_ref().map(|b| alloc_block(arena, b));
        arena.alloc(AstNode::FuncDecl { name, ty, body })
    }

    let name = alloc_ident(&mut arena, &file.name);
    let mut decl_ids = Vec::new();
    for d in &file.decls {
        match d {
            Decl::FuncDecl(fd) => decl_ids.push(alloc_func_decl(&mut arena, fd)),
            _ => panic!("Phase 1 file_to_arena only supports FuncDecl"),
        }
    }
    let decls = arena.alloc_list(&decl_ids);
    let root = arena.alloc(AstNode::File {
        name,
        decls,
        package: file.package,
        file_start: file.file_start,
        file_end: file.file_end,
        go_version: file.go_version.clone(),
        id: file.id,
    });
    (arena, root)
}

/// Reconstruct an owned [`crate::ast::File`] from a Phase 1 arena encoding.
pub fn arena_to_file(arena: &AstArena, root: NodeId) -> crate::ast::File {
    use crate::ast::{BlockStmt, Decl, FieldList, File, FuncDecl, FuncType};

    fn ident(arena: &AstArena, id: NodeId) -> Ident {
        match arena.get(id) {
            AstNode::Ident {
                name,
                name_pos,
                id,
            } => Ident {
                name_pos: *name_pos,
                name: name.clone(),
                obj: Default::default(),
                id: *id,
            },
            _ => panic!("expected Ident"),
        }
    }

    fn field_list(arena: &AstArena, id: NodeId) -> FieldList {
        match arena.get(id) {
            AstNode::FieldList { opening, closing } => FieldList {
                opening: *opening,
                list: Vec::new(),
                closing: *closing,
            },
            _ => panic!("expected FieldList"),
        }
    }

    fn func_type(arena: &AstArena, id: NodeId) -> FuncType {
        match arena.get(id) {
            AstNode::FuncType {
                func,
                params,
                results,
                id,
            } => FuncType {
                func: *func,
                type_params: None,
                params: params.map(|fl| field_list(arena, fl)),
                results: results.map(|fl| field_list(arena, fl)),
                id: *id,
            },
            _ => panic!("expected FuncType"),
        }
    }

    fn block(arena: &AstArena, id: NodeId) -> BlockStmt {
        match arena.get(id) {
            AstNode::BlockStmt {
                lbrace,
                rbrace,
                id,
            } => BlockStmt {
                lbrace: *lbrace,
                list: Vec::new(),
                rbrace: *rbrace,
                id: *id,
            },
            _ => panic!("expected BlockStmt"),
        }
    }

    fn func_decl(arena: &AstArena, id: NodeId) -> FuncDecl {
        match arena.get(id) {
            AstNode::FuncDecl { name, ty, body } => FuncDecl {
                doc: None,
                recv: None,
                name: ident(arena, *name),
                ty: func_type(arena, *ty),
                body: body.map(|b| block(arena, b)),
            },
            _ => panic!("expected FuncDecl"),
        }
    }

    match arena.get(root) {
        AstNode::File {
            name,
            decls,
            package,
            file_start,
            file_end,
            go_version,
            id,
        } => {
            let decls = arena
                .list(*decls)
                .iter()
                .map(|d| Decl::FuncDecl(func_decl(arena, *d)))
                .collect();
            File {
                doc: None,
                package: *package,
                name: ident(arena, *name),
                decls,
                file_start: *file_start,
                file_end: *file_end,
                scope: None,
                imports: Vec::new(),
                unresolved: Vec::new(),
                comments: Vec::new(),
                go_version: go_version.clone(),
                id: *id,
            }
        }
        _ => panic!("expected File root"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{parse_file, Mode};
    use crate::position::FileSet;

    #[test]
    fn roundtrip_package_and_empty_func() {
        let src = b"package p\n\nfunc f() {}\n";
        let fset = FileSet::new();
        let file = parse_file(&fset, "t.go", src, Mode::NONE).expect("parse");
        let (arena, root) = file_to_arena(&file);
        let back = arena_to_file(&arena, root);
        assert_eq!(back.name.name, "p");
        assert_eq!(back.decls.len(), 1);
        match &back.decls[0] {
            crate::ast::Decl::FuncDecl(fd) => {
                assert_eq!(fd.name.name, "f");
                assert!(fd.recv.is_none());
                assert!(fd.body.is_some());
                let orig_ty = match &file.decls[0] {
                    crate::ast::Decl::FuncDecl(o) => o.ty.id,
                    _ => 0,
                };
                assert_eq!(fd.ty.id, orig_ty);
            }
            _ => panic!("expected FuncDecl"),
        }
    }
}
