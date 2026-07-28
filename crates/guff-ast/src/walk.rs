// Port of Go's go/ast/walk.go to Rust.
//
// Original: Copyright 2009/2024 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Walking the AST in Rust
// -----------------------
//
// Go's `ast.Walk` switches on the concrete type of a `Node` interface
// and invokes `v.Visit(child)` for each child. We can't recover the
// concrete type from an `&Expr` (or `&Stmt`) in the same way Go does
// from an interface value — but our AST is closed enums, so we can
// dispatch with `match` instead. The result is `NodeRef<'a>`, an enum
// with one variant per concrete AST struct (56 in total). Walking
// always promotes an `&Expr`/`&Stmt`/`&Decl`/`&Spec` to its narrow
// `NodeRef::Xxx` variant before invoking the visitor, so a callback
// sees the same concrete type Go's switch would have surfaced.
//
// Two traversal APIs are exposed:
//
// * Trait-based: implement [`Visitor`] and call [`walk`].
// * Closure-based: [`inspect`] (Go's `Inspect`), [`preorder`] (Go's
//   `Preorder`), and [`preorder_stack`] (Go's `PreorderStack`).
//
// Visitor mirrors Go's "return nil from Visit to skip subtree"
// pattern using `enter -> bool` (true → descend) plus an optional
// `leave` hook for the trailing `v.Visit(nil)` call.
//
// Children are visited via direct recursion through [`for_each_child`]
// — no per-node child-list allocation.

use crate::ast::*;

// ============================================================
// NodeRef — concrete-type-narrow reference into the AST.
// ============================================================

/// Reference to any concrete AST node. The variant tells you the
/// exact Go type that would have been seen at `case` arm in
/// `ast.Walk`'s switch.
#[derive(Debug, Clone, Copy)]
pub enum NodeRef<'a> {
    // Comments and fields
    Comment(&'a Comment),
    CommentGroup(&'a CommentGroup),
    Field(&'a Field),
    FieldList(&'a FieldList),
    // Expressions
    BadExpr(&'a BadExpr),
    Ident(&'a Ident),
    BasicLit(&'a BasicLit),
    Ellipsis(&'a Ellipsis),
    FuncLit(&'a FuncLit),
    CompositeLit(&'a CompositeLit),
    ParenExpr(&'a ParenExpr),
    SelectorExpr(&'a SelectorExpr),
    IndexExpr(&'a IndexExpr),
    IndexListExpr(&'a IndexListExpr),
    SliceExpr(&'a SliceExpr),
    TypeAssertExpr(&'a TypeAssertExpr),
    CallExpr(&'a CallExpr),
    StarExpr(&'a StarExpr),
    UnaryExpr(&'a UnaryExpr),
    BinaryExpr(&'a BinaryExpr),
    KeyValueExpr(&'a KeyValueExpr),
    // Types
    ArrayType(&'a ArrayType),
    StructType(&'a StructType),
    FuncType(&'a FuncType),
    InterfaceType(&'a InterfaceType),
    MapType(&'a MapType),
    ChanType(&'a ChanType),
    // Statements
    BadStmt(&'a BadStmt),
    DeclStmt(&'a DeclStmt),
    EmptyStmt(&'a EmptyStmt),
    LabeledStmt(&'a LabeledStmt),
    ExprStmt(&'a ExprStmt),
    SendStmt(&'a SendStmt),
    IncDecStmt(&'a IncDecStmt),
    AssignStmt(&'a AssignStmt),
    GoStmt(&'a GoStmt),
    DeferStmt(&'a DeferStmt),
    ReturnStmt(&'a ReturnStmt),
    BranchStmt(&'a BranchStmt),
    BlockStmt(&'a BlockStmt),
    IfStmt(&'a IfStmt),
    CaseClause(&'a CaseClause),
    SwitchStmt(&'a SwitchStmt),
    TypeSwitchStmt(&'a TypeSwitchStmt),
    CommClause(&'a CommClause),
    SelectStmt(&'a SelectStmt),
    ForStmt(&'a ForStmt),
    RangeStmt(&'a RangeStmt),
    // Specs
    ImportSpec(&'a ImportSpec),
    ValueSpec(&'a ValueSpec),
    TypeSpec(&'a TypeSpec),
    // Declarations
    BadDecl(&'a BadDecl),
    GenDecl(&'a GenDecl),
    FuncDecl(&'a FuncDecl),
    // Files and packages
    File(&'a File),
    Package(&'a Package),
}

// ============================================================
// Node kinds — flat discriminant and type-erased round trip.
// ============================================================

/// Declares everything that has to list all 56 `NodeRef` variants exactly once.
///
/// `kind_name` used to spell the list out by hand; the flat inspector added two
/// more such lists (discriminant, erasure), and three hand-kept copies of the
/// same 56 names is how a variant silently goes missing. Every variant holds a
/// single `&T` whose type is named by the variant, so one name per row is enough.
macro_rules! node_variants {
    ($($variant:ident),* $(,)?) => {
        /// Flat discriminant of a [`NodeRef`] variant, in declaration order.
        ///
        /// Paired with an erased pointer this reproduces a `NodeRef` without
        /// carrying the enum's payload, which is what lets the inspector's event
        /// array be a `Copy` POD (Go's `inspector` stores `[]ast.Node` directly;
        /// `NodeRef<'a>` borrows, so it cannot live in a `'static` result).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum NodeKind { $($variant),* }

        impl NodeKind {
            /// How many kinds exist. A [`NodeMask`] is a `u64`, so this is
            /// checked against 64 below rather than left to a runtime shift
            /// overflow the day a 65th variant is added.
            pub const COUNT: usize = [$(stringify!($variant)),*].len();

            /// This kind's single-bit mask.
            #[inline]
            pub const fn bit(self) -> u64 {
                1u64 << (self as u8)
            }

            /// Inverse of [`NodeRef::kind_name`]. `None` for anything that is
            /// not one of the 56 variant names.
            ///
            /// Callers that build a mask from names must treat `None` as "I
            /// don't know what this is" and widen to [`NodeMask::ALL`] — a name
            /// silently dropped from a mask silently drops findings.
            pub fn from_name(name: &str) -> Option<NodeKind> {
                match name {
                    $(stringify!($variant) => Some(NodeKind::$variant),)*
                    _ => None,
                }
            }
        }

        const _: () = assert!(
            NodeKind::COUNT <= 64,
            "NodeMask is a u64 bitset; widen it before adding a 65th NodeRef variant",
        );

        impl NodeMask {
            /// Every kind — what an unfiltered traversal asks for.
            pub const ALL: NodeMask = NodeMask(0 $(| NodeKind::$variant.bit())*);
        }

        impl<'a> NodeRef<'a> {
            /// Short type name (e.g. `"Ident"`, `"FuncDecl"`). Equivalent of
            /// Go's `reflect.TypeOf(n).Elem().Name()` for AST values.
            pub fn kind_name(self) -> &'static str {
                match self { $(NodeRef::$variant(_) => stringify!($variant),)* }
            }

            /// This node's [`NodeKind`].
            pub fn kind(self) -> NodeKind {
                match self { $(NodeRef::$variant(_) => NodeKind::$variant,)* }
            }

            /// The referenced node as a type-erased thin pointer.
            ///
            /// Lossless when kept together with [`kind`](NodeRef::kind):
            /// [`from_erased`](NodeRef::from_erased) inverts the pair.
            pub fn erased_ptr(self) -> *const () {
                match self { $(NodeRef::$variant(x) => (x as *const $variant).cast(),)* }
            }

            /// Inverse of [`kind`](NodeRef::kind) + [`erased_ptr`](NodeRef::erased_ptr).
            ///
            /// # Safety
            ///
            /// `ptr` must be what `erased_ptr` returned for a `NodeRef` whose
            /// `kind` was `kind` — pairing a pointer with the wrong kind
            /// reinterprets the node as an unrelated type. The node must also
            /// still be alive, and not have moved, for all of `'a`.
            pub unsafe fn from_erased(kind: NodeKind, ptr: *const ()) -> NodeRef<'a> {
                match kind {
                    $(NodeKind::$variant => {
                        NodeRef::$variant(unsafe { &*ptr.cast::<$variant>() })
                    })*
                }
            }
        }
    };
}

node_variants![
    // Comments and fields
    Comment, CommentGroup, Field, FieldList,
    // Expressions
    BadExpr, Ident, BasicLit, Ellipsis, FuncLit, CompositeLit, ParenExpr, SelectorExpr,
    IndexExpr, IndexListExpr, SliceExpr, TypeAssertExpr, CallExpr, StarExpr, UnaryExpr,
    BinaryExpr, KeyValueExpr,
    // Types
    ArrayType, StructType, FuncType, InterfaceType, MapType, ChanType,
    // Statements
    BadStmt, DeclStmt, EmptyStmt, LabeledStmt, ExprStmt, SendStmt, IncDecStmt, AssignStmt,
    GoStmt, DeferStmt, ReturnStmt, BranchStmt, BlockStmt, IfStmt, CaseClause, SwitchStmt,
    TypeSwitchStmt, CommClause, SelectStmt, ForStmt, RangeStmt,
    // Specs
    ImportSpec, ValueSpec, TypeSpec,
    // Declarations
    BadDecl, GenDecl, FuncDecl,
    // Files and packages
    File, Package,
];

/// A set of [`NodeKind`]s, as the `u64` bitset Go's `inspector` calls a
/// "type mask".
///
/// The point of a mask is that a traversal can decide *without touching the
/// node* whether a caller wants it. Most analyzers care about one or two of the
/// 56 kinds, so the mask is what turns "walk the tree and `let else { return }`
/// 99% of it away" into "scan only what was asked for".
///
/// Build one with [`node_mask!`](crate::node_mask):
///
/// ```
/// # use guff::{node_mask, walk::{NodeKind, NodeMask}};
/// const CALLS: NodeMask = node_mask!(CallExpr, GoStmt);
/// assert!(CALLS.contains(NodeKind::CallExpr));
/// assert!(!CALLS.contains(NodeKind::Ident));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeMask(u64);

impl NodeMask {
    /// The empty mask. Matches nothing; `node_mask!()` of no kinds.
    pub const NONE: NodeMask = NodeMask(0);

    /// This mask plus `kind`.
    #[inline]
    pub const fn with(self, kind: NodeKind) -> NodeMask {
        NodeMask(self.0 | kind.bit())
    }

    /// The union of two masks.
    #[inline]
    pub const fn union(self, other: NodeMask) -> NodeMask {
        NodeMask(self.0 | other.0)
    }

    /// Whether `kind` is in this mask.
    #[inline]
    pub const fn contains(self, kind: NodeKind) -> bool {
        self.0 & kind.bit() != 0
    }

    /// Whether the two masks share any kind. This is the subtree-skip test:
    /// a subtree whose kinds don't intersect the wanted mask can be jumped over.
    #[inline]
    pub const fn intersects(self, other: NodeMask) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether this mask matches nothing.
    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits.
    #[inline]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

/// Build a [`NodeMask`] from variant names: `node_mask!(AssignStmt, CallExpr)`.
///
/// Const-evaluable, so callers can hold it in a `const` and pay nothing at run
/// time. Names are [`NodeKind`] variants, i.e. [`NodeRef`] variants.
#[macro_export]
macro_rules! node_mask {
    ($($variant:ident),* $(,)?) => {
        $crate::walk::NodeMask::NONE
            $(.with($crate::walk::NodeKind::$variant))*
    };
}

/// Promote an `&Expr` to the narrow `NodeRef` variant matching its
/// concrete type. Walk dispatch never sees `NodeRef::Expr` because
/// the type doesn't exist — concrete variants only.
pub fn expr_ref<'a>(e: &'a Expr) -> NodeRef<'a> {
    match e {
        Expr::BadExpr(x) => NodeRef::BadExpr(x),
        Expr::Ident(x) => NodeRef::Ident(x),
        Expr::Ellipsis(x) => NodeRef::Ellipsis(x),
        Expr::BasicLit(x) => NodeRef::BasicLit(x),
        Expr::FuncLit(x) => NodeRef::FuncLit(x),
        Expr::CompositeLit(x) => NodeRef::CompositeLit(x),
        Expr::ParenExpr(x) => NodeRef::ParenExpr(x),
        Expr::SelectorExpr(x) => NodeRef::SelectorExpr(x),
        Expr::IndexExpr(x) => NodeRef::IndexExpr(x),
        Expr::IndexListExpr(x) => NodeRef::IndexListExpr(x),
        Expr::SliceExpr(x) => NodeRef::SliceExpr(x),
        Expr::TypeAssertExpr(x) => NodeRef::TypeAssertExpr(x),
        Expr::CallExpr(x) => NodeRef::CallExpr(x),
        Expr::StarExpr(x) => NodeRef::StarExpr(x),
        Expr::UnaryExpr(x) => NodeRef::UnaryExpr(x),
        Expr::BinaryExpr(x) => NodeRef::BinaryExpr(x),
        Expr::KeyValueExpr(x) => NodeRef::KeyValueExpr(x),
        Expr::ArrayType(x) => NodeRef::ArrayType(x),
        Expr::StructType(x) => NodeRef::StructType(x),
        Expr::FuncType(x) => NodeRef::FuncType(x),
        Expr::InterfaceType(x) => NodeRef::InterfaceType(x),
        Expr::MapType(x) => NodeRef::MapType(x),
        Expr::ChanType(x) => NodeRef::ChanType(x),
    }
}

pub fn stmt_ref<'a>(s: &'a Stmt) -> NodeRef<'a> {
    match s {
        Stmt::BadStmt(x) => NodeRef::BadStmt(x),
        Stmt::DeclStmt(x) => NodeRef::DeclStmt(x),
        Stmt::EmptyStmt(x) => NodeRef::EmptyStmt(x),
        Stmt::LabeledStmt(x) => NodeRef::LabeledStmt(x),
        Stmt::ExprStmt(x) => NodeRef::ExprStmt(x),
        Stmt::SendStmt(x) => NodeRef::SendStmt(x),
        Stmt::IncDecStmt(x) => NodeRef::IncDecStmt(x),
        Stmt::AssignStmt(x) => NodeRef::AssignStmt(x),
        Stmt::GoStmt(x) => NodeRef::GoStmt(x),
        Stmt::DeferStmt(x) => NodeRef::DeferStmt(x),
        Stmt::ReturnStmt(x) => NodeRef::ReturnStmt(x),
        Stmt::BranchStmt(x) => NodeRef::BranchStmt(x),
        Stmt::BlockStmt(x) => NodeRef::BlockStmt(x),
        Stmt::IfStmt(x) => NodeRef::IfStmt(x),
        Stmt::CaseClause(x) => NodeRef::CaseClause(x),
        Stmt::SwitchStmt(x) => NodeRef::SwitchStmt(x),
        Stmt::TypeSwitchStmt(x) => NodeRef::TypeSwitchStmt(x),
        Stmt::CommClause(x) => NodeRef::CommClause(x),
        Stmt::SelectStmt(x) => NodeRef::SelectStmt(x),
        Stmt::ForStmt(x) => NodeRef::ForStmt(x),
        Stmt::RangeStmt(x) => NodeRef::RangeStmt(x),
    }
}

pub fn decl_ref<'a>(d: &'a Decl) -> NodeRef<'a> {
    match d {
        Decl::BadDecl(x) => NodeRef::BadDecl(x),
        Decl::GenDecl(x) => NodeRef::GenDecl(x),
        Decl::FuncDecl(x) => NodeRef::FuncDecl(x),
    }
}

pub fn spec_ref<'a>(s: &'a Spec) -> NodeRef<'a> {
    match s {
        Spec::ImportSpec(x) => NodeRef::ImportSpec(x),
        Spec::ValueSpec(x) => NodeRef::ValueSpec(x),
        Spec::TypeSpec(x) => NodeRef::TypeSpec(x),
    }
}

// ============================================================
// Visitor trait
// ============================================================

/// AST visitor.
///
/// `enter` is called before descending into a node's children;
/// returning `false` causes Walk to skip this subtree and not invoke
/// `leave`. `leave` (default = no-op) is called after children have
/// been visited — it corresponds to Go's `v.Visit(nil)` callback.
pub trait Visitor<'a> {
    fn enter(&mut self, node: NodeRef<'a>) -> bool;
    fn leave(&mut self, _node: NodeRef<'a>) {}
}

// ============================================================
// Walk — trait-based traversal
// ============================================================

/// Depth-first traversal: invoke `v.enter(node)`, then recurse into
/// each non-empty child (in the order Go's `Walk` uses), then invoke
/// `v.leave(node)`. If `enter` returns false, the children and
/// `leave` are skipped.
pub fn walk<'a, V: Visitor<'a>>(v: &mut V, node: NodeRef<'a>) {
    if !v.enter(node) {
        return;
    }
    // Descend in place — no child-list allocation. Same pattern as
    // `parser_resolver` (`for_each_child` → recursive call on `&mut self`).
    for_each_child(node, |c| walk(v, c));
    v.leave(node);
}

/// Invoke `visit` once per direct child of `node`, in Go's `Walk`
/// order. Does *not* invoke `visit` on `node` itself.
pub fn for_each_child<'a, F: FnMut(NodeRef<'a>)>(node: NodeRef<'a>, mut visit: F) {
    // Order matches ast.go's Walk switch cases exactly.
    match node {
        // Comments and fields
        NodeRef::Comment(_) => {}

        NodeRef::CommentGroup(g) => {
            for c in &g.list {
                visit(NodeRef::Comment(c));
            }
        }

        NodeRef::Field(f) => {
            if let Some(d) = &f.doc {
                visit(NodeRef::CommentGroup(d));
            }
            for n in &f.names {
                visit(NodeRef::Ident(n));
            }
            if let Some(t) = &f.ty {
                visit(expr_ref(t));
            }
            if let Some(t) = &f.tag {
                visit(NodeRef::BasicLit(t));
            }
            if let Some(c) = &f.comment {
                visit(NodeRef::CommentGroup(c));
            }
        }

        NodeRef::FieldList(fl) => {
            for f in &fl.list {
                visit(NodeRef::Field(f));
            }
        }

        // Expressions: leaves
        NodeRef::BadExpr(_) | NodeRef::Ident(_) | NodeRef::BasicLit(_) => {}

        NodeRef::Ellipsis(x) => {
            if let Some(elt) = &x.elt {
                visit(expr_ref(elt));
            }
        }

        NodeRef::FuncLit(x) => {
            visit(NodeRef::FuncType(&x.ty));
            visit(NodeRef::BlockStmt(&x.body));
        }

        NodeRef::CompositeLit(x) => {
            if let Some(t) = &x.ty {
                visit(expr_ref(t));
            }
            for e in &x.elts {
                visit(expr_ref(e));
            }
        }

        NodeRef::ParenExpr(x) => visit(expr_ref(&x.x)),

        NodeRef::SelectorExpr(x) => {
            visit(expr_ref(&x.x));
            visit(NodeRef::Ident(&x.sel));
        }

        NodeRef::IndexExpr(x) => {
            visit(expr_ref(&x.x));
            visit(expr_ref(&x.index));
        }

        NodeRef::IndexListExpr(x) => {
            visit(expr_ref(&x.x));
            for i in &x.indices {
                visit(expr_ref(i));
            }
        }

        NodeRef::SliceExpr(x) => {
            visit(expr_ref(&x.x));
            if let Some(lo) = &x.low {
                visit(expr_ref(lo));
            }
            if let Some(hi) = &x.high {
                visit(expr_ref(hi));
            }
            if let Some(mx) = &x.max {
                visit(expr_ref(mx));
            }
        }

        NodeRef::TypeAssertExpr(x) => {
            visit(expr_ref(&x.x));
            if let Some(t) = &x.ty {
                visit(expr_ref(t));
            }
        }

        NodeRef::CallExpr(x) => {
            visit(expr_ref(&x.fun));
            for a in &x.args {
                visit(expr_ref(a));
            }
        }

        NodeRef::StarExpr(x) => visit(expr_ref(&x.x)),
        NodeRef::UnaryExpr(x) => visit(expr_ref(&x.x)),

        NodeRef::BinaryExpr(x) => {
            visit(expr_ref(&x.x));
            visit(expr_ref(&x.y));
        }

        NodeRef::KeyValueExpr(x) => {
            visit(expr_ref(&x.key));
            visit(expr_ref(&x.value));
        }

        // Types
        NodeRef::ArrayType(x) => {
            if let Some(l) = &x.len {
                visit(expr_ref(l));
            }
            visit(expr_ref(&x.elt));
        }

        NodeRef::StructType(x) => visit(NodeRef::FieldList(&x.fields)),

        NodeRef::FuncType(x) => {
            if let Some(tp) = &x.type_params {
                visit(NodeRef::FieldList(tp));
            }
            if let Some(p) = &x.params {
                visit(NodeRef::FieldList(p));
            }
            if let Some(r) = &x.results {
                visit(NodeRef::FieldList(r));
            }
        }

        NodeRef::InterfaceType(x) => visit(NodeRef::FieldList(&x.methods)),

        NodeRef::MapType(x) => {
            visit(expr_ref(&x.key));
            visit(expr_ref(&x.value));
        }

        NodeRef::ChanType(x) => visit(expr_ref(&x.value)),

        // Statements
        NodeRef::BadStmt(_) | NodeRef::EmptyStmt(_) => {}

        NodeRef::DeclStmt(s) => visit(decl_ref(&s.decl)),

        NodeRef::LabeledStmt(s) => {
            visit(NodeRef::Ident(&s.label));
            visit(stmt_ref(&s.stmt));
        }

        NodeRef::ExprStmt(s) => visit(expr_ref(&s.x)),

        NodeRef::SendStmt(s) => {
            visit(expr_ref(&s.chan_));
            visit(expr_ref(&s.value));
        }

        NodeRef::IncDecStmt(s) => visit(expr_ref(&s.x)),

        NodeRef::AssignStmt(s) => {
            for e in &s.lhs {
                visit(expr_ref(e));
            }
            for e in &s.rhs {
                visit(expr_ref(e));
            }
        }

        NodeRef::GoStmt(s) => visit(NodeRef::CallExpr(&s.call)),
        NodeRef::DeferStmt(s) => visit(NodeRef::CallExpr(&s.call)),

        NodeRef::ReturnStmt(s) => {
            for e in &s.results {
                visit(expr_ref(e));
            }
        }

        NodeRef::BranchStmt(s) => {
            if let Some(l) = &s.label {
                visit(NodeRef::Ident(l));
            }
        }

        NodeRef::BlockStmt(s) => {
            for st in &s.list {
                visit(stmt_ref(st));
            }
        }

        NodeRef::IfStmt(s) => {
            if let Some(init) = &s.init {
                visit(stmt_ref(init));
            }
            visit(expr_ref(&s.cond));
            visit(NodeRef::BlockStmt(&s.body));
            if let Some(e) = &s.else_ {
                visit(stmt_ref(e));
            }
        }

        NodeRef::CaseClause(s) => {
            for e in &s.list {
                visit(expr_ref(e));
            }
            for st in &s.body {
                visit(stmt_ref(st));
            }
        }

        NodeRef::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                visit(stmt_ref(init));
            }
            if let Some(t) = &s.tag {
                visit(expr_ref(t));
            }
            visit(NodeRef::BlockStmt(&s.body));
        }

        NodeRef::TypeSwitchStmt(s) => {
            if let Some(init) = &s.init {
                visit(stmt_ref(init));
            }
            visit(stmt_ref(&s.assign));
            visit(NodeRef::BlockStmt(&s.body));
        }

        NodeRef::CommClause(s) => {
            if let Some(c) = &s.comm {
                visit(stmt_ref(c));
            }
            for st in &s.body {
                visit(stmt_ref(st));
            }
        }

        NodeRef::SelectStmt(s) => visit(NodeRef::BlockStmt(&s.body)),

        NodeRef::ForStmt(s) => {
            if let Some(init) = &s.init {
                visit(stmt_ref(init));
            }
            if let Some(c) = &s.cond {
                visit(expr_ref(c));
            }
            if let Some(p) = &s.post {
                visit(stmt_ref(p));
            }
            visit(NodeRef::BlockStmt(&s.body));
        }

        NodeRef::RangeStmt(s) => {
            if let Some(k) = &s.key {
                visit(expr_ref(k));
            }
            if let Some(val) = &s.value {
                visit(expr_ref(val));
            }
            visit(expr_ref(&s.x));
            visit(NodeRef::BlockStmt(&s.body));
        }

        // Specs
        NodeRef::ImportSpec(sp) => {
            if let Some(d) = &sp.doc {
                visit(NodeRef::CommentGroup(d));
            }
            if let Some(n) = &sp.name {
                visit(NodeRef::Ident(n));
            }
            visit(NodeRef::BasicLit(&sp.path));
            if let Some(c) = &sp.comment {
                visit(NodeRef::CommentGroup(c));
            }
        }

        NodeRef::ValueSpec(sp) => {
            if let Some(d) = &sp.doc {
                visit(NodeRef::CommentGroup(d));
            }
            for n in &sp.names {
                visit(NodeRef::Ident(n));
            }
            if let Some(t) = &sp.ty {
                visit(expr_ref(t));
            }
            for val in &sp.values {
                visit(expr_ref(val));
            }
            if let Some(c) = &sp.comment {
                visit(NodeRef::CommentGroup(c));
            }
        }

        NodeRef::TypeSpec(sp) => {
            if let Some(d) = &sp.doc {
                visit(NodeRef::CommentGroup(d));
            }
            visit(NodeRef::Ident(&sp.name));
            if let Some(tp) = &sp.type_params {
                visit(NodeRef::FieldList(tp));
            }
            visit(expr_ref(&sp.ty));
            if let Some(c) = &sp.comment {
                visit(NodeRef::CommentGroup(c));
            }
        }

        // Declarations
        NodeRef::BadDecl(_) => {}

        NodeRef::GenDecl(d) => {
            if let Some(doc) = &d.doc {
                visit(NodeRef::CommentGroup(doc));
            }
            for sp in &d.specs {
                visit(spec_ref(sp));
            }
        }

        NodeRef::FuncDecl(d) => {
            if let Some(doc) = &d.doc {
                visit(NodeRef::CommentGroup(doc));
            }
            if let Some(recv) = &d.recv {
                visit(NodeRef::FieldList(recv));
            }
            visit(NodeRef::Ident(&d.name));
            visit(NodeRef::FuncType(&d.ty));
            if let Some(body) = &d.body {
                visit(NodeRef::BlockStmt(body));
            }
        }

        // Files and packages
        NodeRef::File(f) => {
            if let Some(doc) = &f.doc {
                visit(NodeRef::CommentGroup(doc));
            }
            visit(NodeRef::Ident(&f.name));
            for d in &f.decls {
                visit(decl_ref(d));
            }
            // Do not walk f.comments — they're visited indirectly
            // through the nodes that reference them.
        }

        NodeRef::Package(p) => {
            // BTreeMap iterates in key order, deterministic — different
            // from Go's randomized map order, but more useful in tests.
            for file in p.files.values() {
                visit(NodeRef::File(file));
            }
        }
    }
}

// ============================================================
// Closure-based convenience: inspect, preorder, preorder_stack
// ============================================================

/// `inspect(node, f)` mirrors Go's `ast.Inspect`. The callback `f` is
/// invoked twice per node:
///
/// * `f(Some(node))` before children — return `false` to skip the
///   subtree (no `leave` call).
/// * `f(None)` after children — the value returned here is ignored.
pub fn inspect<'a, F>(node: NodeRef<'a>, mut f: F)
where
    F: FnMut(Option<NodeRef<'a>>) -> bool,
{
    fn rec<'a, F>(node: NodeRef<'a>, f: &mut F)
    where
        F: FnMut(Option<NodeRef<'a>>) -> bool,
    {
        if !f(Some(node)) {
            return;
        }
        for_each_child(node, |c| rec(c, f));
        f(None);
    }
    rec(node, &mut f);
}

/// `preorder(node, f)` mirrors Go's `ast.Preorder`. The callback is
/// invoked once per node in depth-first preorder. Return `false` from
/// `f` to stop the traversal (no callbacks for any remaining nodes).
pub fn preorder<'a, F>(node: NodeRef<'a>, mut f: F)
where
    F: FnMut(NodeRef<'a>) -> bool,
{
    fn rec<'a, F>(node: NodeRef<'a>, f: &mut F) -> bool
    where
        F: FnMut(NodeRef<'a>) -> bool,
    {
        if !f(node) {
            return false;
        }
        let mut ok = true;
        for_each_child(node, |c| {
            if ok {
                ok = rec(c, f);
            }
        });
        ok
    }
    let _ = rec(node, &mut f);
}

/// `preorder_stack(root, stack, f)` mirrors Go's `ast.PreorderStack`.
/// The callback receives the current node and the stack of enclosing
/// nodes from `root` down to (but not including) the current node.
/// Returning `false` skips the subtree — there is no second call to
/// `f` (no `leave` semantics).
///
/// `stack` is taken by mutable reference so callers can pre-populate
/// it with outer context; on return it's restored to its initial state.
pub fn preorder_stack<'a, F>(root: NodeRef<'a>, stack: &mut Vec<NodeRef<'a>>, mut f: F)
where
    F: FnMut(NodeRef<'a>, &[NodeRef<'a>]) -> bool,
{
    let before = stack.len();
    fn rec<'a, F>(node: NodeRef<'a>, stack: &mut Vec<NodeRef<'a>>, f: &mut F)
    where
        F: FnMut(NodeRef<'a>, &[NodeRef<'a>]) -> bool,
    {
        if !f(node, stack) {
            return;
        }
        stack.push(node);
        for_each_child(node, |c| rec(c, stack, f));
        stack.pop();
    }
    rec(root, stack, &mut f);

    if stack.len() != before {
        panic!("preorder_stack: push/pop mismatch");
    }
}

// ============================================================
// Tests (mirror walk_test.go using hand-built ASTs).
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Token;
    use crate::Pos;

    /// Mirror of Go's `TestPreorder_Break`: confirm that returning
    /// false from the callback in the middle of a subtree doesn't
    /// cause sibling-visit corruption (no panic, no unexpected calls).
    ///
    /// Original Go source was:
    ///
    /// ```text
    /// package p
    /// type T struct {
    ///     F int `json:"f"` // a field
    /// }
    /// ```
    #[test]
    fn test_preorder_break() {
        // Build AST: file with one type decl with one field "F int".
        let file = File {
            package: Pos(1),
            name: Ident::new_ident("p"),
            decls: vec![Decl::GenDecl(GenDecl {
                doc: None,
                tok_pos: Pos(10),
                tok: Some(Token::TYPE),
                lparen: Pos(0),
                specs: vec![Spec::TypeSpec(TypeSpec {
                    doc: None,
                    name: Ident::new_ident("T"),
                    type_params: None,
                    assign: Pos(0),
                    ty: Expr::StructType(StructType {
                        id: 0,
                        struct_: Pos(20),
                        fields: FieldList {
                            opening: Pos(27),
                            list: vec![Field {
                                doc: None,
                                names: vec![Ident::new_ident("F")],
                                ty: Some(Expr::Ident(Ident::new_ident("int"))),
                                tag: Some(BasicLit {
                                    id: 0,
                                    value_pos: Pos(35),
                                    value_end: Pos(45),
                                    kind: Some(Token::STRING),
                                    value: "`json:\"f\"`".to_string(),
                                }),
                                comment: None,
                                id: 0,
                            }],
                            closing: Pos(50),
                        },
                        incomplete: false,
                    }),
                    comment: None,
                    id: 0,
                })],
                rparen: Pos(0),
            })],
            ..File::default()
        };

        // Run preorder, stop when we see the Ident "F". This must not panic.
        let mut visited = Vec::<String>::new();
        let mut stopped = false;
        preorder(NodeRef::File(&file), |n| {
            visited.push(n.kind_name().to_string());
            if let NodeRef::Ident(id) = n {
                if id.name == "F" {
                    stopped = true;
                    return false;
                }
            }
            true
        });
        assert!(stopped, "should have found ident F");
        // After stopping at F, no further nodes are visited.
        assert_eq!(visited.last().map(String::as_str), Some("Ident"));
    }

    /// Mirror of Go's `TestPreorderStack`. Builds the equivalent of:
    ///
    /// ```text
    /// package a
    /// func f() { print("hello") }
    /// func g() { print("goodbye"); panic("oops") }
    /// ```
    #[test]
    fn test_preorder_stack() {
        fn ident(n: &str) -> Ident {
            Ident::new_ident(n)
        }
        fn lit(s: &str) -> BasicLit {
            BasicLit {
                id: 0,
                value_pos: Pos::default(),
                value_end: Pos::default(),
                kind: Some(Token::STRING),
                value: s.to_string(),
            }
        }
        fn call(fname: &str, arg: &str) -> CallExpr {
            CallExpr {
                id: 0,
                fun: Box::new(Expr::Ident(ident(fname))),
                lparen: Pos::default(),
                args: vec![Expr::BasicLit(lit(arg))],
                ellipsis: Pos::default(),
                rparen: Pos::default(),
            }
        }
        fn expr_stmt(c: CallExpr) -> Stmt {
            Stmt::ExprStmt(ExprStmt {
                x: Expr::CallExpr(c),
            })
        }
        fn func_decl(name: &str, body: Vec<Stmt>) -> Decl {
            Decl::FuncDecl(FuncDecl {
                doc: None,
                recv: None,
                name: ident(name),
                ty: FuncType {
                    id: 0,
                    func: Pos(1),
                    type_params: None,
                    params: Some(FieldList::default()),
                    results: None,
                },
                body: Some(BlockStmt {
                    lbrace: Pos::default(),
                    list: body,
                    rbrace: Pos(1),
                    id: 0,
                }),
            })
        }

        let file = File {
            package: Pos(1),
            name: ident("a"),
            decls: vec![
                func_decl("f", vec![expr_stmt(call("print", "\"hello\""))]),
                func_decl(
                    "g",
                    vec![
                        expr_stmt(call("print", "\"goodbye\"")),
                        expr_stmt(call("panic", "\"oops\"")),
                    ],
                ),
            ],
            ..File::default()
        };

        let mut events: Vec<String> = Vec::new();
        let mut got_stack: Vec<String> = Vec::new();
        let mut stack: Vec<NodeRef> = Vec::new();
        preorder_stack(NodeRef::File(&file), &mut stack, |n, stk| {
            events.push(n.kind_name().to_string());
            if let NodeRef::FuncDecl(d) = n {
                if d.name.name == "f" {
                    return false; // prune f's subtree
                }
            }
            if let NodeRef::BasicLit(b) = n {
                if b.value == "\"oops\"" {
                    for s in stk {
                        got_stack.push(s.kind_name().to_string());
                    }
                }
            }
            true
        });

        let want_events = vec![
            "File",
            "Ident",    // package a
            "FuncDecl", // func f() [pruned]
            "FuncDecl",
            "Ident",
            "FuncType",
            "FieldList",
            "BlockStmt", // func g()
            "ExprStmt",
            "CallExpr",
            "Ident",
            "BasicLit", // print
            "ExprStmt",
            "CallExpr",
            "Ident",
            "BasicLit", // panic
        ];
        assert_eq!(events, want_events, "events mismatch");

        let want_stack = vec!["File", "FuncDecl", "BlockStmt", "ExprStmt", "CallExpr"];
        assert_eq!(got_stack, want_stack, "stack mismatch");
    }

    /// Sanity: walk via the Visitor trait reaches the same nodes
    /// inspect() does (i.e. no node category is silently skipped).
    #[test]
    fn test_walk_and_inspect_agree() {
        // Reuse the TestPreorderStack AST but a single func decl.
        let file = File {
            package: Pos(1),
            name: Ident::new_ident("p"),
            decls: vec![Decl::FuncDecl(FuncDecl {
                doc: None,
                recv: None,
                name: Ident::new_ident("f"),
                ty: FuncType {
                    id: 0,
                    func: Pos(1),
                    type_params: None,
                    params: Some(FieldList::default()),
                    results: None,
                },
                body: Some(BlockStmt {
                    lbrace: Pos::default(),
                    list: vec![Stmt::ReturnStmt(ReturnStmt::default())],
                    rbrace: Pos(1),
                    id: 0,
                }),
            })],
            ..File::default()
        };

        struct Counter<'a> {
            events: Vec<&'static str>,
            _p: std::marker::PhantomData<&'a ()>,
        }
        impl<'a> Visitor<'a> for Counter<'a> {
            fn enter(&mut self, n: NodeRef<'a>) -> bool {
                self.events.push(n.kind_name());
                true
            }
        }

        let mut c = Counter::<'_> {
            events: Vec::new(),
            _p: std::marker::PhantomData,
        };
        walk(&mut c, NodeRef::File(&file));

        let mut events_inspect = Vec::<&'static str>::new();
        inspect(NodeRef::File(&file), |n| {
            if let Some(node) = n {
                events_inspect.push(node.kind_name());
            }
            true
        });

        assert_eq!(c.events, events_inspect);
    }
}
