// Port of Go's go/ast/commentmap.go to Rust.
//
// Original: Copyright 2012 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// `CommentMap` associates each AST node with the comment groups that
// belong to it, applying the same line-proximity rules Go uses. In Go
// the map key is `Node` (an interface — i.e. a pointer), so equality is
// by identity. This port uses [`NodeRef<'a>`] from `walk.rs`, comparing
// keys by the *pointer* embedded in each variant via [`node_ptr`].
//
// The CommentMap carries a lifetime tying it to the AST it indexes —
// dropping the AST while the map is alive is rejected at compile time.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{CommentGroup, Decl, Spec, Stmt};
use crate::position::{FileSet, Position};
use crate::walk::{inspect, NodeRef};

// ====================================================================
// Identity helpers
// ====================================================================

/// Stable pointer-as-usize identity for a [`NodeRef`]. Two `NodeRef`
/// values referring to the same node share the same `node_ptr`.
pub fn node_ptr(n: NodeRef<'_>) -> usize {
    match n {
        NodeRef::Comment(p) => p as *const _ as usize,
        NodeRef::CommentGroup(p) => p as *const _ as usize,
        NodeRef::Field(p) => p as *const _ as usize,
        NodeRef::FieldList(p) => p as *const _ as usize,
        NodeRef::BadExpr(p) => p as *const _ as usize,
        NodeRef::Ident(p) => p as *const _ as usize,
        NodeRef::BasicLit(p) => p as *const _ as usize,
        NodeRef::Ellipsis(p) => p as *const _ as usize,
        NodeRef::FuncLit(p) => p as *const _ as usize,
        NodeRef::CompositeLit(p) => p as *const _ as usize,
        NodeRef::ParenExpr(p) => p as *const _ as usize,
        NodeRef::SelectorExpr(p) => p as *const _ as usize,
        NodeRef::IndexExpr(p) => p as *const _ as usize,
        NodeRef::IndexListExpr(p) => p as *const _ as usize,
        NodeRef::SliceExpr(p) => p as *const _ as usize,
        NodeRef::TypeAssertExpr(p) => p as *const _ as usize,
        NodeRef::CallExpr(p) => p as *const _ as usize,
        NodeRef::StarExpr(p) => p as *const _ as usize,
        NodeRef::UnaryExpr(p) => p as *const _ as usize,
        NodeRef::BinaryExpr(p) => p as *const _ as usize,
        NodeRef::KeyValueExpr(p) => p as *const _ as usize,
        NodeRef::ArrayType(p) => p as *const _ as usize,
        NodeRef::StructType(p) => p as *const _ as usize,
        NodeRef::FuncType(p) => p as *const _ as usize,
        NodeRef::InterfaceType(p) => p as *const _ as usize,
        NodeRef::MapType(p) => p as *const _ as usize,
        NodeRef::ChanType(p) => p as *const _ as usize,
        NodeRef::BadStmt(p) => p as *const _ as usize,
        NodeRef::DeclStmt(p) => p as *const _ as usize,
        NodeRef::EmptyStmt(p) => p as *const _ as usize,
        NodeRef::LabeledStmt(p) => p as *const _ as usize,
        NodeRef::ExprStmt(p) => p as *const _ as usize,
        NodeRef::SendStmt(p) => p as *const _ as usize,
        NodeRef::IncDecStmt(p) => p as *const _ as usize,
        NodeRef::AssignStmt(p) => p as *const _ as usize,
        NodeRef::GoStmt(p) => p as *const _ as usize,
        NodeRef::DeferStmt(p) => p as *const _ as usize,
        NodeRef::ReturnStmt(p) => p as *const _ as usize,
        NodeRef::BranchStmt(p) => p as *const _ as usize,
        NodeRef::BlockStmt(p) => p as *const _ as usize,
        NodeRef::IfStmt(p) => p as *const _ as usize,
        NodeRef::CaseClause(p) => p as *const _ as usize,
        NodeRef::SwitchStmt(p) => p as *const _ as usize,
        NodeRef::TypeSwitchStmt(p) => p as *const _ as usize,
        NodeRef::CommClause(p) => p as *const _ as usize,
        NodeRef::SelectStmt(p) => p as *const _ as usize,
        NodeRef::ForStmt(p) => p as *const _ as usize,
        NodeRef::RangeStmt(p) => p as *const _ as usize,
        NodeRef::ImportSpec(p) => p as *const _ as usize,
        NodeRef::ValueSpec(p) => p as *const _ as usize,
        NodeRef::TypeSpec(p) => p as *const _ as usize,
        NodeRef::BadDecl(p) => p as *const _ as usize,
        NodeRef::GenDecl(p) => p as *const _ as usize,
        NodeRef::FuncDecl(p) => p as *const _ as usize,
        NodeRef::File(p) => p as *const _ as usize,
        NodeRef::Package(p) => p as *const _ as usize,
    }
}

/// Source-order pos/end helpers — `NodeRef::pos`/`NodeRef::end` aren't
/// defined on the enum yet, so we compute them by variant here.
///
/// This is Go's `ast.Node.Pos()`: the first byte of the node's own source
/// text. Diagnostics need it because upstream reports the node, so a checker
/// that reports an inner token (the operator, the `(`) lands in the wrong
/// column.
pub fn node_pos(n: NodeRef<'_>) -> crate::position::Pos {
    match n {
        NodeRef::Comment(c) => c.pos(),
        NodeRef::CommentGroup(c) => c.pos(),
        NodeRef::Field(f) => {
            if let Some(d) = &f.doc {
                d.pos()
            } else if let Some(first) = f.names.first() {
                first.pos()
            } else if let Some(t) = &f.ty {
                t.pos()
            } else {
                crate::position::Pos::default()
            }
        }
        NodeRef::FieldList(fl) => fl.pos(),
        NodeRef::BadExpr(x) => x.from,
        NodeRef::Ident(x) => x.pos(),
        NodeRef::BasicLit(x) => x.pos(),
        NodeRef::Ellipsis(x) => x.ellipsis,
        NodeRef::FuncLit(x) => x.ty.pos(),
        NodeRef::CompositeLit(x) => {
            if let Some(t) = &x.ty {
                t.pos()
            } else {
                x.lbrace
            }
        }
        NodeRef::ParenExpr(x) => x.lparen,
        NodeRef::SelectorExpr(x) => x.x.pos(),
        NodeRef::IndexExpr(x) => x.x.pos(),
        NodeRef::IndexListExpr(x) => x.x.pos(),
        NodeRef::SliceExpr(x) => x.x.pos(),
        NodeRef::TypeAssertExpr(x) => x.x.pos(),
        NodeRef::CallExpr(x) => x.pos(),
        NodeRef::StarExpr(x) => x.star,
        NodeRef::UnaryExpr(x) => x.op_pos,
        NodeRef::BinaryExpr(x) => x.x.pos(),
        NodeRef::KeyValueExpr(x) => x.key.pos(),
        NodeRef::ArrayType(x) => x.lbrack,
        NodeRef::StructType(x) => x.struct_,
        NodeRef::FuncType(x) => x.pos(),
        NodeRef::InterfaceType(x) => x.interface_,
        NodeRef::MapType(x) => x.map_,
        NodeRef::ChanType(x) => x.begin,
        NodeRef::BadStmt(x) => x.from,
        NodeRef::DeclStmt(x) => Decl_pos(&x.decl),
        NodeRef::EmptyStmt(x) => x.semicolon,
        NodeRef::LabeledStmt(x) => x.label.pos(),
        NodeRef::ExprStmt(x) => x.x.pos(),
        NodeRef::SendStmt(x) => x.chan_.pos(),
        NodeRef::IncDecStmt(x) => x.x.pos(),
        NodeRef::AssignStmt(x) => x.lhs.first().map(|e| e.pos()).unwrap_or_default(),
        NodeRef::GoStmt(x) => x.go_,
        NodeRef::DeferStmt(x) => x.defer_,
        NodeRef::ReturnStmt(x) => x.return_,
        NodeRef::BranchStmt(x) => x.tok_pos,
        NodeRef::BlockStmt(x) => x.lbrace,
        NodeRef::IfStmt(x) => x.if_,
        NodeRef::CaseClause(x) => x.case,
        NodeRef::SwitchStmt(x) => x.switch,
        NodeRef::TypeSwitchStmt(x) => x.switch,
        NodeRef::CommClause(x) => x.case,
        NodeRef::SelectStmt(x) => x.select_,
        NodeRef::ForStmt(x) => x.for_,
        NodeRef::RangeStmt(x) => x.for_,
        NodeRef::ImportSpec(x) => Spec_pos(&Spec::ImportSpec((*x).clone())),
        NodeRef::ValueSpec(x) => Spec_pos(&Spec::ValueSpec((*x).clone())),
        NodeRef::TypeSpec(x) => x.name.pos(),
        NodeRef::BadDecl(x) => x.from,
        NodeRef::GenDecl(x) => x.tok_pos,
        NodeRef::FuncDecl(x) => x.ty.pos(),
        NodeRef::File(x) => x.pos(),
        NodeRef::Package(x) => x.pos(),
    }
}

/// Go's `ast.Node.End()`: one past the last byte of the node's own source text.
pub fn node_end(n: NodeRef<'_>) -> crate::position::Pos {
    match n {
        NodeRef::Comment(c) => c.end(),
        NodeRef::CommentGroup(c) => c.end(),
        NodeRef::Field(f) => {
            if let Some(c) = &f.comment {
                c.end()
            } else if let Some(t) = &f.tag {
                t.end()
            } else if let Some(t) = &f.ty {
                t.end()
            } else if let Some(last) = f.names.last() {
                last.end()
            } else {
                crate::position::Pos::default()
            }
        }
        NodeRef::FieldList(fl) => fl.end(),
        NodeRef::BadExpr(x) => x.to,
        NodeRef::Ident(x) => x.end(),
        NodeRef::BasicLit(x) => x.end(),
        NodeRef::Ellipsis(x) => {
            if let Some(elt) = &x.elt {
                elt.end()
            } else {
                crate::position::Pos(x.ellipsis.0 + 3)
            }
        }
        NodeRef::FuncLit(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::CompositeLit(x) => crate::position::Pos(x.rbrace.0 + 1),
        NodeRef::ParenExpr(x) => crate::position::Pos(x.rparen.0 + 1),
        NodeRef::SelectorExpr(x) => x.sel.end(),
        NodeRef::IndexExpr(x) => crate::position::Pos(x.rbrack.0 + 1),
        NodeRef::IndexListExpr(x) => crate::position::Pos(x.rbrack.0 + 1),
        NodeRef::SliceExpr(x) => crate::position::Pos(x.rbrack.0 + 1),
        NodeRef::TypeAssertExpr(x) => crate::position::Pos(x.rparen.0 + 1),
        NodeRef::CallExpr(x) => x.end(),
        NodeRef::StarExpr(x) => x.x.end(),
        NodeRef::UnaryExpr(x) => x.x.end(),
        NodeRef::BinaryExpr(x) => x.y.end(),
        NodeRef::KeyValueExpr(x) => x.value.end(),
        NodeRef::ArrayType(x) => x.elt.end(),
        NodeRef::StructType(x) => crate::position::Pos(x.fields.closing.0 + 1),
        NodeRef::FuncType(x) => x.end(),
        NodeRef::InterfaceType(x) => crate::position::Pos(x.methods.closing.0 + 1),
        NodeRef::MapType(x) => x.value.end(),
        NodeRef::ChanType(x) => x.value.end(),
        NodeRef::BadStmt(x) => x.to,
        NodeRef::DeclStmt(x) => Decl_end(&x.decl),
        NodeRef::EmptyStmt(x) => crate::position::Pos(x.semicolon.0 + 1),
        NodeRef::LabeledStmt(x) => Stmt_end(&x.stmt),
        NodeRef::ExprStmt(x) => x.x.end(),
        NodeRef::SendStmt(x) => x.value.end(),
        NodeRef::IncDecStmt(x) => crate::position::Pos(x.tok_pos.0 + 2),
        NodeRef::AssignStmt(x) => x.rhs.last().map(|e| e.end()).unwrap_or_default(),
        NodeRef::GoStmt(x) => x.call.end(),
        NodeRef::DeferStmt(x) => x.call.end(),
        NodeRef::ReturnStmt(x) => x
            .results
            .last()
            .map(|e| e.end())
            .unwrap_or(crate::position::Pos(x.return_.0 + 6)),
        NodeRef::BranchStmt(x) => {
            if let Some(l) = &x.label {
                l.end()
            } else {
                crate::position::Pos(x.tok_pos.0 + 1)
            }
        }
        NodeRef::BlockStmt(x) => crate::position::Pos(x.rbrace.0 + 1),
        NodeRef::IfStmt(x) => {
            if let Some(e) = &x.else_ {
                Stmt_end(e)
            } else {
                crate::position::Pos(x.body.rbrace.0 + 1)
            }
        }
        NodeRef::CaseClause(x) => crate::position::Pos(x.colon.0 + 1),
        NodeRef::SwitchStmt(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::TypeSwitchStmt(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::CommClause(x) => crate::position::Pos(x.colon.0 + 1),
        NodeRef::SelectStmt(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::ForStmt(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::RangeStmt(x) => crate::position::Pos(x.body.rbrace.0 + 1),
        NodeRef::ImportSpec(x) => Spec_end(&Spec::ImportSpec((*x).clone())),
        NodeRef::ValueSpec(x) => Spec_end(&Spec::ValueSpec((*x).clone())),
        NodeRef::TypeSpec(x) => x.ty.end(),
        NodeRef::BadDecl(x) => x.to,
        NodeRef::GenDecl(x) => {
            if x.rparen.is_valid() {
                crate::position::Pos(x.rparen.0 + 1)
            } else {
                x.specs.last().map(|s| s.end()).unwrap_or(x.tok_pos)
            }
        }
        NodeRef::FuncDecl(x) => {
            if let Some(b) = &x.body {
                crate::position::Pos(b.rbrace.0 + 1)
            } else {
                x.ty.end()
            }
        }
        NodeRef::File(x) => x.end(),
        NodeRef::Package(x) => x.end(),
    }
}

#[allow(non_snake_case)]
fn Spec_pos(s: &Spec) -> crate::position::Pos {
    s.pos()
}
#[allow(non_snake_case)]
fn Spec_end(s: &Spec) -> crate::position::Pos {
    s.end()
}
#[allow(non_snake_case)]
fn Decl_pos(d: &Decl) -> crate::position::Pos {
    d.pos()
}
#[allow(non_snake_case)]
fn Decl_end(d: &Decl) -> crate::position::Pos {
    d.end()
}
#[allow(non_snake_case)]
fn Stmt_end(s: &Stmt) -> crate::position::Pos {
    s.end()
}

// ====================================================================
// CommentMap
// ====================================================================

/// Comment-to-node association map. Lifetime `'a` ties the map to the
/// AST it indexes.
pub struct CommentMap<'a> {
    /// Map from node-identity (pointer-as-usize) to associated groups.
    entries: HashMap<usize, Vec<CommentGroup>>,
    /// Parallel ordering vector so iteration is deterministic.
    nodes: Vec<NodeRef<'a>>,
}

impl<'a> CommentMap<'a> {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            nodes: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Add `c` to the list associated with `n`.
    pub fn add_comment(&mut self, n: NodeRef<'a>, c: CommentGroup) {
        let key = node_ptr(n);
        let entry = self.entries.entry(key).or_insert_with(|| {
            self.nodes.push(n);
            Vec::new()
        });
        entry.push(c);
        // The closure above pushes the node only when entry was just
        // created (or_insert_with runs only on miss).
    }

    /// Comments associated with `n`, if any.
    pub fn get(&self, n: NodeRef<'a>) -> Option<&[CommentGroup]> {
        self.entries.get(&node_ptr(n)).map(|v| v.as_slice())
    }

    /// Replace `old` with `new` in the map. Comments associated with
    /// `old` are reassigned to `new`. Returns `new`.
    pub fn update(&mut self, old: NodeRef<'a>, new: NodeRef<'a>) -> NodeRef<'a> {
        if let Some(list) = self.entries.remove(&node_ptr(old)) {
            // Drop old from `nodes` to keep ordering map clean.
            self.nodes.retain(|n| node_ptr(*n) != node_ptr(old));
            let key = node_ptr(new);
            self.entries
                .entry(key)
                .or_insert_with(|| {
                    self.nodes.push(new);
                    Vec::new()
                })
                .extend(list);
        }
        new
    }

    /// Keep only entries whose nodes are reachable from `root`.
    pub fn filter(&self, root: NodeRef<'a>) -> CommentMap<'a> {
        let mut reachable: std::collections::HashSet<usize> = std::collections::HashSet::new();
        inspect(root, |n| {
            if let Some(node) = n {
                reachable.insert(node_ptr(node));
            }
            true
        });
        let mut out = CommentMap::new();
        for n in &self.nodes {
            if reachable.contains(&node_ptr(*n)) {
                if let Some(list) = self.entries.get(&node_ptr(*n)) {
                    out.entries.insert(node_ptr(*n), list.clone());
                    out.nodes.push(*n);
                }
            }
        }
        out
    }

    /// Flat list of all comment groups in the map, sorted in source
    /// order.
    pub fn comments(&self) -> Vec<CommentGroup> {
        let mut all: Vec<CommentGroup> = Vec::new();
        for v in self.entries.values() {
            for g in v {
                all.push(g.clone());
            }
        }
        all.sort_by(|a, b| a.pos().cmp(&b.pos()));
        all
    }
}

impl<'a> Default for CommentMap<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> std::fmt::Display for CommentMap<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut nodes: Vec<NodeRef<'a>> = self.nodes.clone();
        nodes.sort_by(|a, b| {
            let r = node_pos(*a).cmp(&node_pos(*b));
            if r != std::cmp::Ordering::Equal {
                return r;
            }
            node_end(*a).cmp(&node_end(*b))
        });
        writeln!(f, "CommentMap {{")?;
        for n in nodes {
            let list = self.entries.get(&node_ptr(n)).cloned().unwrap_or_default();
            let label = if let NodeRef::Ident(id) = n {
                id.name.clone()
            } else {
                n.kind_name().to_string()
            };
            writeln!(
                f,
                "\t0x{:x}  {:>20}:  {}",
                node_ptr(n),
                label,
                summary(&list)
            )?;
        }
        writeln!(f, "}}")?;
        Ok(())
    }
}

fn summary(list: &[CommentGroup]) -> String {
    const MAX_LEN: usize = 40;
    let mut buf = String::new();
    'outer: for g in list {
        for c in &g.list {
            if buf.len() >= MAX_LEN {
                break 'outer;
            }
            buf.push_str(&c.text);
        }
    }
    if buf.len() > MAX_LEN {
        buf.truncate(MAX_LEN - 3);
        buf.push_str("...");
    }
    let bytes: Vec<u8> = buf
        .bytes()
        .map(|b| match b {
            b'\t' | b'\n' | b'\r' => b' ',
            _ => b,
        })
        .collect();
    String::from_utf8(bytes).unwrap_or_default()
}

// ====================================================================
// Construction: new_comment_map
// ====================================================================

/// Build a [`CommentMap`] from `comments` against the AST rooted at
/// `node`. Mirrors Go's `NewCommentMap` association rules.
pub fn new_comment_map<'a>(
    fset: &Arc<FileSet>,
    root: NodeRef<'a>,
    comments: &'a [CommentGroup],
) -> CommentMap<'a> {
    let mut cmap = CommentMap::new();
    if comments.is_empty() {
        return cmap;
    }

    // Sort a copy of `comments` in source order. We'll keep our own
    // indices into the slice for the reader state.
    let mut sorted_indices: Vec<usize> = (0..comments.len()).collect();
    sorted_indices.sort_by(|&a, &b| comments[a].pos().cmp(&comments[b].pos()));

    // Collect AST nodes in source order, dropping CommentGroup/Comment
    // (their associations are recorded via owning nodes, not by
    // visiting them directly).
    let mut nodes: Vec<NodeRef<'a>> = Vec::new();
    inspect(root, |n| {
        if let Some(node) = n {
            match node {
                NodeRef::Comment(_) | NodeRef::CommentGroup(_) => return false,
                _ => nodes.push(node),
            }
        }
        true
    });

    let mut r = CommentListReader {
        fset,
        list: comments,
        order: &sorted_indices,
        index: 0,
        comment: None,
        pos: Position::default(),
        end: Position::default(),
    };
    r.next();

    let mut p: Option<NodeRef<'a>> = None;
    let mut pend = Position::default();
    let mut pg: Option<NodeRef<'a>> = None;
    let mut pgend = Position::default();
    let mut stack: Vec<NodeRef<'a>> = Vec::new();

    let q_iter = nodes.iter().copied().map(Some).chain(std::iter::once(None));
    for q in q_iter {
        let qpos = if let Some(q) = q {
            fset.position(node_pos(q))
        } else {
            // Sentinel "infinity" position to flush remaining comments.
            Position {
                filename: String::new(),
                offset: 1 << 30,
                line: 1 << 30,
                column: 0,
            }
        };

        // Process comments before the current node.
        while r.end.offset <= qpos.offset {
            // Pop the stack of "important" nodes whose extent ended.
            if let Some(top) = nodestack_pop(&mut stack, r.comment.unwrap().pos()) {
                pg = Some(top);
                pgend = fset.position(node_end(top));
            }

            // Decide which node to associate with.
            let assoc: NodeRef<'a> = if pg.is_some()
                && (pgend.line == r.pos.line
                    || (pgend.line + 1 == r.pos.line && r.end.line + 1 < qpos.line))
            {
                pg.unwrap()
            } else if p.is_some()
                && (pend.line == r.pos.line
                    || (pend.line + 1 == r.pos.line && r.end.line + 1 < qpos.line)
                    || q.is_none())
            {
                p.unwrap()
            } else {
                match q {
                    Some(q) => q,
                    None => panic!("internal error: comment with no node to attach to"),
                }
            };
            cmap.add_comment(assoc, r.comment.unwrap().clone());
            if r.eol() {
                return cmap;
            }
            r.next();
        }

        if let Some(q) = q {
            p = Some(q);
            pend = fset.position(node_end(q));
            if is_important(q) {
                nodestack_push(&mut stack, q);
            }
        }
    }
    cmap
}

fn is_important(n: NodeRef<'_>) -> bool {
    matches!(
        n,
        NodeRef::File(_)
            | NodeRef::Field(_)
            | NodeRef::BadDecl(_)
            | NodeRef::GenDecl(_)
            | NodeRef::FuncDecl(_)
            | NodeRef::ImportSpec(_)
            | NodeRef::ValueSpec(_)
            | NodeRef::TypeSpec(_)
            | NodeRef::BadStmt(_)
            | NodeRef::DeclStmt(_)
            | NodeRef::EmptyStmt(_)
            | NodeRef::LabeledStmt(_)
            | NodeRef::ExprStmt(_)
            | NodeRef::SendStmt(_)
            | NodeRef::IncDecStmt(_)
            | NodeRef::AssignStmt(_)
            | NodeRef::GoStmt(_)
            | NodeRef::DeferStmt(_)
            | NodeRef::ReturnStmt(_)
            | NodeRef::BranchStmt(_)
            | NodeRef::BlockStmt(_)
            | NodeRef::IfStmt(_)
            | NodeRef::CaseClause(_)
            | NodeRef::SwitchStmt(_)
            | NodeRef::TypeSwitchStmt(_)
            | NodeRef::CommClause(_)
            | NodeRef::SelectStmt(_)
            | NodeRef::ForStmt(_)
            | NodeRef::RangeStmt(_)
    )
}

fn nodestack_push<'a>(stack: &mut Vec<NodeRef<'a>>, n: NodeRef<'a>) {
    nodestack_pop(stack, node_pos(n));
    stack.push(n);
}

fn nodestack_pop<'a>(
    stack: &mut Vec<NodeRef<'a>>,
    pos: crate::position::Pos,
) -> Option<NodeRef<'a>> {
    let mut top: Option<NodeRef<'a>> = None;
    while let Some(last) = stack.last() {
        if node_end(*last) <= pos {
            top = Some(*last);
            stack.pop();
        } else {
            break;
        }
    }
    top
}

struct CommentListReader<'a, 's> {
    fset: &'s Arc<FileSet>,
    list: &'a [CommentGroup],
    order: &'s [usize],
    index: usize,
    comment: Option<&'a CommentGroup>,
    pos: Position,
    end: Position,
}

impl<'a, 's> CommentListReader<'a, 's> {
    fn eol(&self) -> bool {
        self.index >= self.order.len()
    }
    fn next(&mut self) {
        if !self.eol() {
            let idx = self.order[self.index];
            let c = &self.list[idx];
            self.comment = Some(c);
            self.pos = self.fset.position(c.pos());
            self.end = self.fset.position(c.end());
            self.index += 1;
        }
    }
}

// ====================================================================
// Tests — small hand-built ASTs (no parser).
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Comment, CommentGroup, ExprStmt, Ident, Stmt};
    use crate::position::FileSet;
    use crate::walk::stmt_ref;

    /// Build a single-line FileSet position helper.
    fn make_file(lines: &[i64]) -> (Arc<FileSet>, Arc<crate::position::File>) {
        let fset = FileSet::new();
        let size = lines.iter().copied().max().unwrap_or(0) + 100;
        let f = fset.add_file("t.go", fset.base(), size);
        for &offset in lines {
            f.add_line(offset);
        }
        (fset, f)
    }

    #[test]
    fn empty_comment_list_returns_empty_map() {
        let (fset, _) = make_file(&[]);
        let stmt = Stmt::ExprStmt(ExprStmt {
            x: crate::ast::Expr::Ident(Ident::new_ident("x")),
        });
        let cmap = new_comment_map(&fset, stmt_ref(&stmt), &[]);
        assert!(cmap.is_empty());
    }

    #[test]
    fn comments_iter_sorted_in_source_order() {
        // Manually build two comments + an ExprStmt, then assert that
        // comments() returns them in pos order regardless of insertion.
        let mut cmap = CommentMap::new();
        let stmt = Stmt::ExprStmt(ExprStmt {
            x: crate::ast::Expr::Ident(Ident::new_ident("x")),
        });
        let later = CommentGroup {
            list: vec![Comment {
                slash: crate::position::Pos(10),
                text: "// later".to_string(),
            }],
        };
        let earlier = CommentGroup {
            list: vec![Comment {
                slash: crate::position::Pos(2),
                text: "// earlier".to_string(),
            }],
        };
        cmap.add_comment(stmt_ref(&stmt), later);
        cmap.add_comment(stmt_ref(&stmt), earlier);
        let got = cmap.comments();
        let texts: Vec<String> = got.iter().map(|g| g.list[0].text.clone()).collect();
        assert_eq!(texts, vec!["// earlier", "// later"]);
    }

    #[test]
    fn update_moves_associations_to_new_node() {
        let mut cmap = CommentMap::new();
        let a = Ident::new_ident("a");
        let b = Ident::new_ident("b");
        cmap.add_comment(
            NodeRef::Ident(&a),
            CommentGroup {
                list: vec![Comment {
                    slash: crate::position::Pos(1),
                    text: "// for a".to_string(),
                }],
            },
        );
        cmap.update(NodeRef::Ident(&a), NodeRef::Ident(&b));
        assert!(cmap.get(NodeRef::Ident(&a)).is_none());
        let bs = cmap.get(NodeRef::Ident(&b)).expect("attached to b");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].list[0].text, "// for a");
    }

    #[test]
    fn filter_keeps_only_reachable_entries() {
        // Build an ExprStmt wrapping ident `kept`. Attach a comment to
        // `kept` and to a *separate* free-standing ident `gone`.
        let kept = Ident::new_ident("kept");
        let gone = Ident::new_ident("gone");
        let stmt = Stmt::ExprStmt(ExprStmt {
            x: crate::ast::Expr::Ident(kept.clone()),
        });
        // The ident inside `stmt.x` is a DIFFERENT instance from `kept`
        // (since we cloned). We need to walk-find that instance to use
        // as map keys; but since these are toy tests, build the map
        // directly against the in-stmt instance via a fresh ref:
        let inside = match &stmt {
            Stmt::ExprStmt(e) => match &e.x {
                crate::ast::Expr::Ident(id) => id,
                _ => unreachable!(),
            },
            _ => unreachable!(),
        };

        let mut cmap = CommentMap::new();
        cmap.add_comment(
            NodeRef::Ident(inside),
            CommentGroup {
                list: vec![Comment {
                    slash: crate::position::Pos(1),
                    text: "// in".to_string(),
                }],
            },
        );
        cmap.add_comment(
            NodeRef::Ident(&gone),
            CommentGroup {
                list: vec![Comment {
                    slash: crate::position::Pos(2),
                    text: "// out".to_string(),
                }],
            },
        );

        // Filter to subtree reachable from the stmt → only `inside` is
        // visible.
        let filtered = cmap.filter(stmt_ref(&stmt));
        assert_eq!(filtered.len(), 1);
        assert!(filtered.get(NodeRef::Ident(inside)).is_some());
        assert!(filtered.get(NodeRef::Ident(&gone)).is_none());
    }
}
