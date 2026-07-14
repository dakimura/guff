// Port of Go's go/ast/ast.go to Rust.
//
// Original: Copyright 2009/2012 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Layout
// ------
//
// Go uses interfaces (`Node`, `Expr`, `Stmt`, `Decl`, `Spec`) with a
// closed set of concrete implementations. The Rust port models these
// as enums: each "interface" is an enum whose variants own the
// per-node-type struct. This gives us exhaustive `match` for free —
// essential for `walk.rs` later — without any `dyn`-based dispatch.
//
// Recursive positions (e.g. `ParenExpr.X: Expr`, `IfStmt.Init: Stmt`)
// use `Box<>`; lists use `Vec<>`; nullable pointers map to `Option<>`.
//
// Deprecated bookkeeping (`Object`, `Scope`, `Ident.Obj`, `File.Scope`,
// `File.Unresolved`, `Package.{Scope,Imports}`) is intentionally
// omitted. The Go documentation already steers callers toward
// `go/types` for that information.
//
// Naming: Go field names are converted to snake_case. Rust keywords
// (`type`, `if`, `else`, `for`, `return`, ...) get a trailing
// underscore (`ty`, `if_`, `else_`, `for_`, `return_`, ...).

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::token::{is_exported, Token};
use crate::Pos;

/// Process-wide source of stable AST-node identities.
///
/// Go's `go/types` keys `Info.Defs`/`Info.Uses` (and friends) on the
/// `*ast.Ident`/`ast.Node` pointer. In this port the type checker clones
/// `Expr`/`Ident` values freely, so pointer identity is unavailable; instead
/// the parser stamps each identifier with a unique id from this counter and
/// the maps key on that. A global (rather than per-parser) counter guarantees
/// ids are unique *across* the files of a single package — they share one
/// `Info`. Clones inherit their source node's id. Starts at 1 so that `0` can
/// serve as the "unstamped / synthetic" sentinel.
static NODE_ID: AtomicU32 = AtomicU32::new(1);

/// Returns a fresh, process-unique nonzero node id. See [`NODE_ID`].
pub fn next_node_id() -> u32 {
    NODE_ID.fetch_add(1, Ordering::Relaxed)
}

// ============================================================
// Comments
// ============================================================

/// A `//`-style or `/* */`-style comment.
///
/// `text` does not contain `\r`s (the parser strips them); the end
/// position is computed from `text.len()`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Comment {
    pub slash: Pos,
    pub text: String,
}

impl Comment {
    pub fn pos(&self) -> Pos {
        self.slash
    }
    pub fn end(&self) -> Pos {
        Pos(self.slash.0 + self.text.len() as i64)
    }
}

/// A sequence of comments with no other tokens and no empty lines
/// between them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommentGroup {
    pub list: Vec<Comment>,
}

impl CommentGroup {
    pub fn pos(&self) -> Pos {
        self.list.first().map(|c| c.pos()).unwrap_or_default()
    }
    pub fn end(&self) -> Pos {
        self.list.last().map(|c| c.end()).unwrap_or_default()
    }

    /// Plain-text content of the comment group.
    ///
    /// Comment markers (`//`, `/*`, `*/`), the first space of a
    /// line comment, and leading/trailing empty lines are removed.
    /// Directive lines (`//go:noinline`, `//line ...`) are dropped.
    /// Runs of interior blank lines collapse to one; trailing
    /// whitespace on each line is trimmed. The result is newline-
    /// terminated unless empty.
    pub fn text(&self) -> String {
        let comments: Vec<&str> = self.list.iter().map(|c| c.text.as_str()).collect();

        let mut lines: Vec<String> = Vec::with_capacity(10);
        for &raw in &comments {
            // Strip comment markers.
            let bytes = raw.as_bytes();
            if bytes.len() < 2 {
                // Pathological / not a real comment; skip.
                continue;
            }
            let c: &str = match bytes[1] {
                b'/' => {
                    // //-style (no trailing newline)
                    let body = &raw[2..];
                    if body.is_empty() {
                        ""
                    } else if let Some(stripped) = body.strip_prefix(' ') {
                        // Strip first space (matches Example test behavior).
                        stripped
                    } else if is_directive(body) {
                        continue;
                    } else {
                        body
                    }
                }
                b'*' => &raw[2..raw.len() - 2],
                _ => raw,
            };

            for line in c.split('\n') {
                lines.push(strip_trailing_whitespace(line));
            }
        }

        // Drop leading blank lines; collapse runs of blanks to a single blank.
        let mut n = 0;
        for i in 0..lines.len() {
            if !lines[i].is_empty() || (n > 0 && !lines[n - 1].is_empty()) {
                lines[n] = std::mem::take(&mut lines[i]);
                n += 1;
            }
        }
        lines.truncate(n);

        // Final "" to make join() leave a trailing newline.
        if n > 0 && !lines[n - 1].is_empty() {
            lines.push(String::new());
        }

        lines.join("\n")
    }
}

fn is_whitespace_byte(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

fn strip_trailing_whitespace(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut i = bytes.len();
    while i > 0 && is_whitespace_byte(bytes[i - 1]) {
        i -= 1;
    }
    s[..i].to_string()
}

/// `is_directive` reports whether `c` (a comment body with `//`
/// stripped) is a Go comment directive. Mirrors the Go test cases
/// exactly:
///
/// * `//line ` / `//extern ` / `//export ` are directives.
/// * `//[a-z0-9]+:[a-z0-9]` is a generic directive.
pub fn is_directive(c: &str) -> bool {
    if c.starts_with("line ") || c.starts_with("extern ") || c.starts_with("export ") {
        return true;
    }
    let bytes = c.as_bytes();
    let colon = match c.find(':') {
        Some(i) => i,
        None => return false,
    };
    // `colon <= 0` in Go means the colon is at the very start.
    if colon == 0 || colon + 1 >= bytes.len() {
        return false;
    }
    for (i, &b) in bytes.iter().enumerate().take(colon + 2) {
        if i == colon {
            continue;
        }
        if !b.is_ascii_lowercase() && !b.is_ascii_digit() {
            return false;
        }
    }
    true
}

// ============================================================
// Fields
// ============================================================

/// A field in a struct, a method in an interface, or a
/// parameter/result in a signature.
#[derive(Debug, Clone, Default)]
pub struct Field {
    pub doc: Option<CommentGroup>,
    pub names: Vec<Ident>,
    pub ty: Option<Expr>,
    pub tag: Option<BasicLit>,
    pub comment: Option<CommentGroup>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    /// Keys the implicit object of an anonymous parameter / unnamed receiver
    /// in `types::Info::implicits`.
    pub id: u32,
}

impl Field {
    pub fn pos(&self) -> Pos {
        if let Some(n) = self.names.first() {
            return n.pos();
        }
        if let Some(t) = &self.ty {
            return t.pos();
        }
        Pos::default()
    }
    pub fn end(&self) -> Pos {
        if let Some(t) = &self.tag {
            return t.end();
        }
        if let Some(t) = &self.ty {
            return t.end();
        }
        if let Some(n) = self.names.last() {
            return n.end();
        }
        Pos::default()
    }
}

/// A list of fields enclosed by parentheses, braces, or brackets.
#[derive(Debug, Clone, Default)]
pub struct FieldList {
    pub opening: Pos,
    pub list: Vec<Field>,
    pub closing: Pos,
}

impl FieldList {
    pub fn pos(&self) -> Pos {
        if self.opening.is_valid() {
            return self.opening;
        }
        if let Some(f) = self.list.first() {
            return f.pos();
        }
        Pos::default()
    }
    pub fn end(&self) -> Pos {
        if self.closing.is_valid() {
            return Pos(self.closing.0 + 1);
        }
        if let Some(f) = self.list.last() {
            return f.end();
        }
        Pos::default()
    }
    /// Number of parameters / struct fields. Embedded/unnamed entries
    /// count as one.
    pub fn num_fields(&self) -> usize {
        self.list
            .iter()
            .map(|g| if g.names.is_empty() { 1 } else { g.names.len() })
            .sum()
    }
}

// ============================================================
// Expressions and types
// ============================================================

/// Channel direction bitmask (`Send`, `Recv`, or both).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChanDir(pub u8);

impl ChanDir {
    pub const SEND: ChanDir = ChanDir(1 << 0);
    pub const RECV: ChanDir = ChanDir(1 << 1);
}

// --- Concrete expression structs ------------------------------------

#[derive(Debug, Clone, Default)]
pub struct BadExpr {
    pub from: Pos,
    pub to: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

/// Note: the `obj` field is deprecated in Go (the `ast.Object` model is
/// superseded by `go/types`). It is included here only so that
/// [`crate::scope`], [`crate::resolve`], and [`crate::parser_resolver`]
/// have somewhere to record their results. Wrapped in a `Mutex` so the
/// resolver pass can write through `&Ident` while the rest of the AST is
/// held immutably, and so loaded packages / analysis actions are `Sync`
/// for multi-threaded runners (R9 / PL11).
#[derive(Debug, Default)]
pub struct Ident {
    pub name_pos: Pos,
    pub name: String,
    pub obj: std::sync::Mutex<Option<std::sync::Arc<crate::scope::Object>>>,
    /// Stable per-node identity, used by `go/types`'s `Info.Defs`/`Info.Uses`
    /// maps (Go keys those on the `*ast.Ident` pointer; we cannot, because the
    /// type checker clones `Expr`/`Ident` freely — a clone must denote the same
    /// source identifier). The parser assigns a process-unique nonzero id via
    /// [`next_node_id`]; clones share it. `0` means "not stamped" (hand-built
    /// or synthetic nodes) and is never recorded.
    pub id: u32,
}

impl Clone for Ident {
    fn clone(&self) -> Self {
        Self {
            name_pos: self.name_pos,
            name: self.name.clone(),
            obj: std::sync::Mutex::new(self.obj.lock().unwrap().clone()),
            id: self.id,
        }
    }
}

impl Ident {
    /// `new_ident("Foo")` — convenience for ASTs built by hand
    /// (matches Go's `ast.NewIdent`).
    pub fn new_ident(name: impl Into<String>) -> Self {
        Self {
            name_pos: Pos::default(),
            name: name.into(),
            obj: std::sync::Mutex::new(None),
            id: 0,
        }
    }
    /// The stable node id (see the `id` field). `0` = unstamped.
    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn is_exported(&self) -> bool {
        is_exported(&self.name)
    }
    pub fn pos(&self) -> Pos {
        self.name_pos
    }
    pub fn end(&self) -> Pos {
        Pos(self.name_pos.0 + self.name.len() as i64)
    }
}

impl std::fmt::Display for Ident {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.name.is_empty() {
            f.write_str("<nil>")
        } else {
            f.write_str(&self.name)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Ellipsis {
    pub ellipsis: Pos,
    pub elt: Option<Box<Expr>>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct BasicLit {
    pub value_pos: Pos,
    pub value_end: Pos,
    pub kind: Option<Token>,
    pub value: String,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

impl BasicLit {
    pub fn pos(&self) -> Pos {
        self.value_pos
    }
    pub fn end(&self) -> Pos {
        if self.value_end.is_valid() {
            self.value_end
        } else {
            Pos(self.value_pos.0 + self.value.len() as i64)
        }
    }
}

#[derive(Debug, Clone)]
pub struct FuncLit {
    pub ty: FuncType,
    pub body: BlockStmt,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CompositeLit {
    pub ty: Option<Box<Expr>>,
    pub lbrace: Pos,
    pub elts: Vec<Expr>,
    pub rbrace: Pos,
    pub incomplete: bool,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct ParenExpr {
    pub lparen: Pos,
    pub x: Box<Expr>,
    pub rparen: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct SelectorExpr {
    pub x: Box<Expr>,
    pub sel: Ident,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct IndexExpr {
    pub x: Box<Expr>,
    pub lbrack: Pos,
    pub index: Box<Expr>,
    pub rbrack: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct IndexListExpr {
    pub x: Box<Expr>,
    pub lbrack: Pos,
    pub indices: Vec<Expr>,
    pub rbrack: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct SliceExpr {
    pub x: Box<Expr>,
    pub lbrack: Pos,
    pub low: Option<Box<Expr>>,
    pub high: Option<Box<Expr>>,
    pub max: Option<Box<Expr>>,
    pub slice3: bool,
    pub rbrack: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct TypeAssertExpr {
    pub x: Box<Expr>,
    pub lparen: Pos,
    pub ty: Option<Box<Expr>>,
    pub rparen: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct CallExpr {
    pub fun: Box<Expr>,
    pub lparen: Pos,
    pub args: Vec<Expr>,
    pub ellipsis: Pos,
    pub rparen: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

impl CallExpr {
    pub fn pos(&self) -> Pos {
        self.fun.pos()
    }
    pub fn end(&self) -> Pos {
        Pos(self.rparen.0 + 1)
    }
}

#[derive(Debug, Clone)]
pub struct StarExpr {
    pub star: Pos,
    pub x: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct UnaryExpr {
    pub op_pos: Pos,
    pub op: Token,
    pub x: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct BinaryExpr {
    pub x: Box<Expr>,
    pub op_pos: Pos,
    pub op: Token,
    pub y: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct KeyValueExpr {
    pub key: Box<Expr>,
    pub colon: Pos,
    pub value: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

// --- Concrete type structs (also Expr variants in Go) ---------------

#[derive(Debug, Clone)]
pub struct ArrayType {
    pub lbrack: Pos,
    pub len: Option<Box<Expr>>,
    pub elt: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct StructType {
    pub struct_: Pos,
    pub fields: FieldList,
    pub incomplete: bool,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

/// Per Go docs `Params` is non-nil for a real signature, but we keep
/// it `Option<FieldList>` for symmetry with `TypeParams`/`Results`.
#[derive(Debug, Clone, Default)]
pub struct FuncType {
    pub func: Pos,
    pub type_params: Option<FieldList>,
    pub params: Option<FieldList>,
    pub results: Option<FieldList>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

impl FuncType {
    pub fn pos(&self) -> Pos {
        match (&self.params, self.func.is_valid()) {
            (Some(p), false) => p.pos(),
            _ => self.func,
        }
    }
    pub fn end(&self) -> Pos {
        if let Some(r) = &self.results {
            r.end()
        } else if let Some(p) = &self.params {
            p.end()
        } else {
            Pos::default()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct InterfaceType {
    pub interface_: Pos,
    pub methods: FieldList,
    pub incomplete: bool,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct MapType {
    pub map_: Pos,
    pub key: Box<Expr>,
    pub value: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct ChanType {
    pub begin: Pos,
    pub arrow: Pos,
    pub dir: ChanDir,
    pub value: Box<Expr>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

// --- Expr enum ------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Expr {
    BadExpr(BadExpr),
    Ident(Ident),
    Ellipsis(Ellipsis),
    BasicLit(BasicLit),
    FuncLit(FuncLit),
    CompositeLit(CompositeLit),
    ParenExpr(ParenExpr),
    SelectorExpr(SelectorExpr),
    IndexExpr(IndexExpr),
    IndexListExpr(IndexListExpr),
    SliceExpr(SliceExpr),
    TypeAssertExpr(TypeAssertExpr),
    CallExpr(CallExpr),
    StarExpr(StarExpr),
    UnaryExpr(UnaryExpr),
    BinaryExpr(BinaryExpr),
    KeyValueExpr(KeyValueExpr),
    ArrayType(ArrayType),
    StructType(StructType),
    FuncType(FuncType),
    InterfaceType(InterfaceType),
    MapType(MapType),
    ChanType(ChanType),
}

impl Expr {
    pub fn pos(&self) -> Pos {
        match self {
            Expr::BadExpr(x) => x.from,
            Expr::Ident(x) => x.pos(),
            Expr::Ellipsis(x) => x.ellipsis,
            Expr::BasicLit(x) => x.value_pos,
            Expr::FuncLit(x) => x.ty.pos(),
            // (FuncType::pos defined above)
            Expr::CompositeLit(x) => x.ty.as_ref().map(|t| t.pos()).unwrap_or(x.lbrace),
            Expr::ParenExpr(x) => x.lparen,
            Expr::SelectorExpr(x) => x.x.pos(),
            Expr::IndexExpr(x) => x.x.pos(),
            Expr::IndexListExpr(x) => x.x.pos(),
            Expr::SliceExpr(x) => x.x.pos(),
            Expr::TypeAssertExpr(x) => x.x.pos(),
            Expr::CallExpr(x) => x.fun.pos(),
            Expr::StarExpr(x) => x.star,
            Expr::UnaryExpr(x) => x.op_pos,
            Expr::BinaryExpr(x) => x.x.pos(),
            Expr::KeyValueExpr(x) => x.key.pos(),
            Expr::ArrayType(x) => x.lbrack,
            Expr::StructType(x) => x.struct_,
            Expr::FuncType(x) => x.pos(),
            Expr::InterfaceType(x) => x.interface_,
            Expr::MapType(x) => x.map_,
            Expr::ChanType(x) => x.begin,
        }
    }

    pub fn end(&self) -> Pos {
        match self {
            Expr::BadExpr(x) => x.to,
            Expr::Ident(x) => x.end(),
            Expr::Ellipsis(x) => x
                .elt
                .as_ref()
                .map(|e| e.end())
                .unwrap_or(Pos(x.ellipsis.0 + 3)),
            Expr::BasicLit(x) => x.end(),
            Expr::FuncLit(x) => x.body.end(),
            Expr::CompositeLit(x) => Pos(x.rbrace.0 + 1),
            Expr::ParenExpr(x) => Pos(x.rparen.0 + 1),
            Expr::SelectorExpr(x) => x.sel.end(),
            Expr::IndexExpr(x) => Pos(x.rbrack.0 + 1),
            Expr::IndexListExpr(x) => Pos(x.rbrack.0 + 1),
            Expr::SliceExpr(x) => Pos(x.rbrack.0 + 1),
            Expr::TypeAssertExpr(x) => Pos(x.rparen.0 + 1),
            Expr::CallExpr(x) => Pos(x.rparen.0 + 1),
            Expr::StarExpr(x) => x.x.end(),
            Expr::UnaryExpr(x) => x.x.end(),
            Expr::BinaryExpr(x) => x.y.end(),
            Expr::KeyValueExpr(x) => x.value.end(),
            Expr::ArrayType(x) => x.elt.end(),
            Expr::StructType(x) => x.fields.end(),
            Expr::FuncType(x) => x.end(),
            Expr::InterfaceType(x) => x.methods.end(),
            Expr::MapType(x) => x.value.end(),
            Expr::ChanType(x) => x.value.end(),
        }
    }

    /// The stable node id of this expression (see [`Ident::id`]). `0` means the
    /// node was built by hand / never stamped, and is never recorded by the
    /// type checker's `Info` maps.
    pub fn id(&self) -> u32 {
        match self {
            Expr::BadExpr(x) => x.id,
            Expr::Ident(x) => x.id,
            Expr::Ellipsis(x) => x.id,
            Expr::BasicLit(x) => x.id,
            Expr::FuncLit(x) => x.id,
            Expr::CompositeLit(x) => x.id,
            Expr::ParenExpr(x) => x.id,
            Expr::SelectorExpr(x) => x.id,
            Expr::IndexExpr(x) => x.id,
            Expr::IndexListExpr(x) => x.id,
            Expr::SliceExpr(x) => x.id,
            Expr::TypeAssertExpr(x) => x.id,
            Expr::CallExpr(x) => x.id,
            Expr::StarExpr(x) => x.id,
            Expr::UnaryExpr(x) => x.id,
            Expr::BinaryExpr(x) => x.id,
            Expr::KeyValueExpr(x) => x.id,
            Expr::ArrayType(x) => x.id,
            Expr::StructType(x) => x.id,
            Expr::FuncType(x) => x.id,
            Expr::InterfaceType(x) => x.id,
            Expr::MapType(x) => x.id,
            Expr::ChanType(x) => x.id,
        }
    }

    /// Set this expression's stable node id. Used by [`crate::stamp`]'s
    /// post-parse stamping pass.
    pub fn set_id(&mut self, id: u32) {
        match self {
            Expr::BadExpr(x) => x.id = id,
            Expr::Ident(x) => x.id = id,
            Expr::Ellipsis(x) => x.id = id,
            Expr::BasicLit(x) => x.id = id,
            Expr::FuncLit(x) => x.id = id,
            Expr::CompositeLit(x) => x.id = id,
            Expr::ParenExpr(x) => x.id = id,
            Expr::SelectorExpr(x) => x.id = id,
            Expr::IndexExpr(x) => x.id = id,
            Expr::IndexListExpr(x) => x.id = id,
            Expr::SliceExpr(x) => x.id = id,
            Expr::TypeAssertExpr(x) => x.id = id,
            Expr::CallExpr(x) => x.id = id,
            Expr::StarExpr(x) => x.id = id,
            Expr::UnaryExpr(x) => x.id = id,
            Expr::BinaryExpr(x) => x.id = id,
            Expr::KeyValueExpr(x) => x.id = id,
            Expr::ArrayType(x) => x.id = id,
            Expr::StructType(x) => x.id = id,
            Expr::FuncType(x) => x.id = id,
            Expr::InterfaceType(x) => x.id = id,
            Expr::MapType(x) => x.id = id,
            Expr::ChanType(x) => x.id = id,
        }
    }
}

// ============================================================
// Statements
// ============================================================

#[derive(Debug, Clone, Default)]
pub struct BadStmt {
    pub from: Pos,
    pub to: Pos,
}

#[derive(Debug, Clone)]
pub struct DeclStmt {
    pub decl: Decl,
}

#[derive(Debug, Clone, Default)]
pub struct EmptyStmt {
    pub semicolon: Pos,
    pub implicit: bool,
}

#[derive(Debug, Clone)]
pub struct LabeledStmt {
    pub label: Ident,
    pub colon: Pos,
    pub stmt: Box<Stmt>,
}

#[derive(Debug, Clone)]
pub struct ExprStmt {
    pub x: Expr,
}

#[derive(Debug, Clone)]
pub struct SendStmt {
    pub chan_: Expr,
    pub arrow: Pos,
    pub value: Expr,
}

#[derive(Debug, Clone)]
pub struct IncDecStmt {
    pub x: Expr,
    pub tok_pos: Pos,
    pub tok: Token,
}

#[derive(Debug, Clone, Default)]
pub struct AssignStmt {
    pub lhs: Vec<Expr>,
    pub tok_pos: Pos,
    pub tok: Option<Token>,
    pub rhs: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct GoStmt {
    pub go_: Pos,
    pub call: CallExpr,
}

#[derive(Debug, Clone)]
pub struct DeferStmt {
    pub defer_: Pos,
    pub call: CallExpr,
}

#[derive(Debug, Clone, Default)]
pub struct ReturnStmt {
    pub return_: Pos,
    pub results: Vec<Expr>,
}

#[derive(Debug, Clone)]
pub struct BranchStmt {
    pub tok_pos: Pos,
    pub tok: Token,
    pub label: Option<Ident>,
}

#[derive(Debug, Clone, Default)]
pub struct BlockStmt {
    pub lbrace: Pos,
    pub list: Vec<Stmt>,
    pub rbrace: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    /// Keys the block's [`Scope`](crate::scope::Scope) in `types::Info::scopes`.
    pub id: u32,
}

impl BlockStmt {
    pub fn pos(&self) -> Pos {
        self.lbrace
    }
    pub fn end(&self) -> Pos {
        if self.rbrace.is_valid() {
            Pos(self.rbrace.0 + 1)
        } else if let Some(last) = self.list.last() {
            last.end()
        } else {
            Pos(self.lbrace.0 + 1)
        }
    }
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub if_: Pos,
    pub init: Option<Box<Stmt>>,
    pub cond: Expr,
    pub body: BlockStmt,
    pub else_: Option<Box<Stmt>>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CaseClause {
    pub case: Pos,
    pub list: Vec<Expr>,
    pub colon: Pos,
    pub body: Vec<Stmt>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct SwitchStmt {
    pub switch: Pos,
    pub init: Option<Box<Stmt>>,
    pub tag: Option<Expr>,
    pub body: BlockStmt,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct TypeSwitchStmt {
    pub switch: Pos,
    pub init: Option<Box<Stmt>>,
    pub assign: Box<Stmt>,
    pub body: BlockStmt,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct CommClause {
    pub case: Pos,
    pub comm: Option<Box<Stmt>>,
    pub colon: Pos,
    pub body: Vec<Stmt>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct SelectStmt {
    pub select_: Pos,
    pub body: BlockStmt,
}

#[derive(Debug, Clone)]
pub struct ForStmt {
    pub for_: Pos,
    pub init: Option<Box<Stmt>>,
    pub cond: Option<Expr>,
    pub post: Option<Box<Stmt>>,
    pub body: BlockStmt,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub struct RangeStmt {
    pub for_: Pos,
    pub key: Option<Expr>,
    pub value: Option<Expr>,
    pub tok_pos: Pos,
    pub tok: Option<Token>,
    pub range_: Pos,
    pub x: Expr,
    pub body: BlockStmt,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    pub id: u32,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // boxing every variant would trade size for many extra allocations
pub enum Stmt {
    BadStmt(BadStmt),
    DeclStmt(DeclStmt),
    EmptyStmt(EmptyStmt),
    LabeledStmt(LabeledStmt),
    ExprStmt(ExprStmt),
    SendStmt(SendStmt),
    IncDecStmt(IncDecStmt),
    AssignStmt(AssignStmt),
    GoStmt(GoStmt),
    DeferStmt(DeferStmt),
    ReturnStmt(ReturnStmt),
    BranchStmt(BranchStmt),
    BlockStmt(BlockStmt),
    IfStmt(IfStmt),
    CaseClause(CaseClause),
    SwitchStmt(SwitchStmt),
    TypeSwitchStmt(TypeSwitchStmt),
    CommClause(CommClause),
    SelectStmt(SelectStmt),
    ForStmt(ForStmt),
    RangeStmt(RangeStmt),
}

impl Stmt {
    pub fn pos(&self) -> Pos {
        match self {
            Stmt::BadStmt(s) => s.from,
            Stmt::DeclStmt(s) => s.decl.pos(),
            Stmt::EmptyStmt(s) => s.semicolon,
            Stmt::LabeledStmt(s) => s.label.pos(),
            Stmt::ExprStmt(s) => s.x.pos(),
            Stmt::SendStmt(s) => s.chan_.pos(),
            Stmt::IncDecStmt(s) => s.x.pos(),
            Stmt::AssignStmt(s) => s.lhs.first().map(|e| e.pos()).unwrap_or_default(),
            Stmt::GoStmt(s) => s.go_,
            Stmt::DeferStmt(s) => s.defer_,
            Stmt::ReturnStmt(s) => s.return_,
            Stmt::BranchStmt(s) => s.tok_pos,
            Stmt::BlockStmt(s) => s.lbrace,
            Stmt::IfStmt(s) => s.if_,
            Stmt::CaseClause(s) => s.case,
            Stmt::SwitchStmt(s) => s.switch,
            Stmt::TypeSwitchStmt(s) => s.switch,
            Stmt::CommClause(s) => s.case,
            Stmt::SelectStmt(s) => s.select_,
            Stmt::ForStmt(s) => s.for_,
            Stmt::RangeStmt(s) => s.for_,
        }
    }

    pub fn end(&self) -> Pos {
        match self {
            Stmt::BadStmt(s) => s.to,
            Stmt::DeclStmt(s) => s.decl.end(),
            Stmt::EmptyStmt(s) => {
                if s.implicit {
                    s.semicolon
                } else {
                    Pos(s.semicolon.0 + 1)
                }
            }
            Stmt::LabeledStmt(s) => s.stmt.end(),
            Stmt::ExprStmt(s) => s.x.end(),
            Stmt::SendStmt(s) => s.value.end(),
            Stmt::IncDecStmt(s) => Pos(s.tok_pos.0 + 2),
            Stmt::AssignStmt(s) => s.rhs.last().map(|e| e.end()).unwrap_or_default(),
            Stmt::GoStmt(s) => s.call.end(),
            Stmt::DeferStmt(s) => s.call.end(),
            Stmt::ReturnStmt(s) => {
                if let Some(last) = s.results.last() {
                    last.end()
                } else {
                    Pos(s.return_.0 + 6) // len("return")
                }
            }
            Stmt::BranchStmt(s) => {
                if let Some(l) = &s.label {
                    l.end()
                } else {
                    let len = s.tok.as_str().len() as i64;
                    Pos(s.tok_pos.0 + len)
                }
            }
            Stmt::BlockStmt(s) => s.end(),
            Stmt::IfStmt(s) => s
                .else_
                .as_ref()
                .map(|e| e.end())
                .unwrap_or_else(|| s.body.end()),
            Stmt::CaseClause(s) => s
                .body
                .last()
                .map(|b| b.end())
                .unwrap_or_else(|| Pos(s.colon.0 + 1)),
            Stmt::SwitchStmt(s) => s.body.end(),
            Stmt::TypeSwitchStmt(s) => s.body.end(),
            Stmt::CommClause(s) => s
                .body
                .last()
                .map(|b| b.end())
                .unwrap_or_else(|| Pos(s.colon.0 + 1)),
            Stmt::SelectStmt(s) => s.body.end(),
            Stmt::ForStmt(s) => s.body.end(),
            Stmt::RangeStmt(s) => s.body.end(),
        }
    }
}

// ============================================================
// Declarations
// ============================================================

#[derive(Debug, Clone, Default)]
pub struct ImportSpec {
    pub doc: Option<CommentGroup>,
    pub name: Option<Ident>,
    pub path: BasicLit,
    pub comment: Option<CommentGroup>,
    pub end_pos: Pos,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    /// Keys the implicit `PkgName` of a name-less import in
    /// `types::Info::implicits`.
    pub id: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ValueSpec {
    pub doc: Option<CommentGroup>,
    pub names: Vec<Ident>,
    pub ty: Option<Expr>,
    pub values: Vec<Expr>,
    pub comment: Option<CommentGroup>,
}

#[derive(Debug, Clone)]
pub struct TypeSpec {
    pub doc: Option<CommentGroup>,
    pub name: Ident,
    pub type_params: Option<FieldList>,
    pub assign: Pos,
    pub ty: Expr,
    pub comment: Option<CommentGroup>,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    /// Keys the generic type's type-parameter [`Scope`](crate::scope::Scope)
    /// in `types::Info::scopes`.
    pub id: u32,
}

#[derive(Debug, Clone)]
pub enum Spec {
    ImportSpec(ImportSpec),
    ValueSpec(ValueSpec),
    TypeSpec(TypeSpec),
}

impl Spec {
    pub fn pos(&self) -> Pos {
        match self {
            Spec::ImportSpec(s) => {
                if let Some(n) = &s.name {
                    n.pos()
                } else {
                    s.path.value_pos
                }
            }
            Spec::ValueSpec(s) => s.names.first().map(|n| n.pos()).unwrap_or_default(),
            Spec::TypeSpec(s) => s.name.pos(),
        }
    }
    pub fn end(&self) -> Pos {
        match self {
            Spec::ImportSpec(s) => {
                if s.end_pos.0 != 0 {
                    s.end_pos
                } else {
                    s.path.end()
                }
            }
            Spec::ValueSpec(s) => {
                if let Some(last) = s.values.last() {
                    last.end()
                } else if let Some(t) = &s.ty {
                    t.end()
                } else {
                    s.names.last().map(|n| n.end()).unwrap_or_default()
                }
            }
            Spec::TypeSpec(s) => s.ty.end(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BadDecl {
    pub from: Pos,
    pub to: Pos,
}

#[derive(Debug, Clone, Default)]
pub struct GenDecl {
    pub doc: Option<CommentGroup>,
    pub tok_pos: Pos,
    pub tok: Option<Token>,
    pub lparen: Pos,
    pub specs: Vec<Spec>,
    pub rparen: Pos,
}

#[derive(Debug, Clone)]
pub struct FuncDecl {
    pub doc: Option<CommentGroup>,
    pub recv: Option<FieldList>,
    pub name: Ident,
    pub ty: FuncType,
    pub body: Option<BlockStmt>,
}

#[derive(Debug, Clone)]
pub enum Decl {
    BadDecl(BadDecl),
    GenDecl(GenDecl),
    FuncDecl(FuncDecl),
}

impl Decl {
    pub fn pos(&self) -> Pos {
        match self {
            Decl::BadDecl(d) => d.from,
            Decl::GenDecl(d) => d.tok_pos,
            Decl::FuncDecl(d) => d.ty.pos(),
        }
    }
    pub fn end(&self) -> Pos {
        match self {
            Decl::BadDecl(d) => d.to,
            Decl::GenDecl(d) => {
                if d.rparen.is_valid() {
                    Pos(d.rparen.0 + 1)
                } else {
                    d.specs.first().map(|s| s.end()).unwrap_or_default()
                }
            }
            Decl::FuncDecl(d) => d
                .body
                .as_ref()
                .map(|b| b.end())
                .unwrap_or_else(|| d.ty.end()),
        }
    }
}

// ============================================================
// Files and packages
// ============================================================

/// A Go source file.
///
/// `scope` and `unresolved` are deprecated (mirror Go's `ast.File`
/// fields of the same name); they are populated by
/// [`crate::resolve::new_package`] for parity. New code should rely on
/// `go/types` instead.
#[derive(Debug, Clone, Default)]
pub struct File {
    pub doc: Option<CommentGroup>,
    pub package: Pos,
    pub name: Ident,
    pub decls: Vec<Decl>,

    pub file_start: Pos,
    pub file_end: Pos,
    pub scope: Option<std::sync::Arc<crate::scope::Scope>>,
    pub imports: Vec<ImportSpec>,
    pub unresolved: Vec<Ident>,
    pub comments: Vec<CommentGroup>,
    pub go_version: String,
    /// Stable node id (see [`Ident::id`]). `0` = unstamped/synthetic.
    /// Keys the file [`Scope`](crate::scope::Scope) in `types::Info::scopes`.
    pub id: u32,
}

impl File {
    pub fn pos(&self) -> Pos {
        self.package
    }
    pub fn end(&self) -> Pos {
        if let Some(d) = self.decls.last() {
            d.end()
        } else {
            self.name.end()
        }
    }
}

/// A set of source files collectively building a package.
///
/// Deprecated. `scope` and `imports` mirror the Go fields of the same
/// name; they are populated by [`crate::resolve::new_package`].
#[derive(Debug, Clone, Default)]
pub struct Package {
    pub name: String,
    pub scope: Option<std::sync::Arc<crate::scope::Scope>>,
    pub imports: BTreeMap<String, std::sync::Arc<crate::scope::Object>>,
    pub files: BTreeMap<String, File>,
}

impl Package {
    pub fn pos(&self) -> Pos {
        Pos::default()
    }
    pub fn end(&self) -> Pos {
        Pos::default()
    }
}

// ============================================================
// Convenience: NewIdent, IsExported, Unparen, IsGenerated
// ============================================================

/// `new_ident("Foo")` — convenience constructor matching Go's
/// `ast.NewIdent`.
pub fn new_ident(name: impl Into<String>) -> Ident {
    Ident::new_ident(name)
}

/// Mirror of `ast.IsExported`.
pub fn ast_is_exported(name: &str) -> bool {
    is_exported(name)
}

/// Strip outer `(... )` from an expression.
pub fn unparen(mut e: Expr) -> Expr {
    loop {
        match e {
            Expr::ParenExpr(p) => e = *p.x,
            other => return other,
        }
    }
}

/// True if the file's leading comments include the standard
/// `// Code generated ... DO NOT EDIT.` line.
pub fn is_generated(file: &File) -> bool {
    generator(file).is_some()
}

fn generator(file: &File) -> Option<String> {
    const PREFIX: &str = "// Code generated ";
    const SUFFIX: &str = " DO NOT EDIT.";
    for group in &file.comments {
        for comment in &group.list {
            if comment.pos().0 > file.package.0 {
                return None;
            }
            if !comment.text.contains(PREFIX) {
                continue;
            }
            for line in comment.text.split('\n') {
                if let Some(rest) = line.strip_prefix(PREFIX) {
                    if let Some(gen) = rest.strip_suffix(SUFFIX) {
                        return Some(gen.to_string());
                    }
                }
            }
        }
    }
    None
}

// ============================================================
// Node enum (top-level "interface" over everything walkable)
// ============================================================

/// A `Node` is any AST node — convenient wrapper for `walk.go`-style
/// traversal. Each variant holds the concrete type by value.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // see comment on `Stmt`
pub enum Node {
    Comment(Comment),
    CommentGroup(CommentGroup),
    Field(Field),
    FieldList(FieldList),
    Expr(Expr),
    Stmt(Stmt),
    Decl(Decl),
    Spec(Spec),
    File(Box<File>),
    Package(Box<Package>),
}

impl Node {
    pub fn pos(&self) -> Pos {
        match self {
            Node::Comment(c) => c.pos(),
            Node::CommentGroup(c) => c.pos(),
            Node::Field(f) => f.pos(),
            Node::FieldList(f) => f.pos(),
            Node::Expr(e) => e.pos(),
            Node::Stmt(s) => s.pos(),
            Node::Decl(d) => d.pos(),
            Node::Spec(s) => s.pos(),
            Node::File(f) => f.pos(),
            Node::Package(p) => p.pos(),
        }
    }
    pub fn end(&self) -> Pos {
        match self {
            Node::Comment(c) => c.end(),
            Node::CommentGroup(c) => c.end(),
            Node::Field(f) => f.end(),
            Node::FieldList(f) => f.end(),
            Node::Expr(e) => e.end(),
            Node::Stmt(s) => s.end(),
            Node::Decl(d) => d.end(),
            Node::Spec(s) => s.end(),
            Node::File(f) => f.pos(), // sentinel; use File::end() directly
            Node::Package(p) => p.end(),
        }
    }
}

// ============================================================
// Tests (mirror of ast_test.go)
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comment_text() {
        let cases: Vec<(Vec<&str>, &str)> = vec![
            (vec!["//"], ""),
            (vec!["//   "], ""),
            (vec!["//", "//", "//   "], ""),
            (vec!["// foo   "], "foo\n"),
            (vec!["//", "//", "// foo"], "foo\n"),
            (vec!["// foo  bar  "], "foo  bar\n"),
            (vec!["// foo", "// bar"], "foo\nbar\n"),
            (vec!["// foo", "//", "//", "//", "// bar"], "foo\n\nbar\n"),
            (vec!["// foo", "/* bar */"], "foo\n bar\n"),
            (vec!["//", "//", "//", "// foo", "//", "//", "//"], "foo\n"),
            (vec!["/**/"], ""),
            (vec!["/*   */"], ""),
            (vec!["/**/", "/**/", "/*   */"], ""),
            (vec!["/* Foo   */"], " Foo\n"),
            (vec!["/* Foo  Bar  */"], " Foo  Bar\n"),
            (vec!["/* Foo*/", "/* Bar*/"], " Foo\n Bar\n"),
            (
                vec!["/* Foo*/", "/**/", "/**/", "/**/", "// Bar"],
                " Foo\n\nBar\n",
            ),
            (
                vec!["/* Foo*/", "/*\n*/", "//", "/*\n*/", "// Bar"],
                " Foo\n\nBar\n",
            ),
            (vec!["/* Foo*/", "// Bar"], " Foo\nBar\n"),
            (vec!["/* Foo\n Bar*/"], " Foo\n Bar\n"),
            (
                vec!["// foo", "//go:noinline", "// bar", "//:baz"],
                "foo\nbar\n:baz\n",
            ),
            (vec!["// foo", "//lint123:ignore", "// bar"], "foo\nbar\n"),
        ];

        for (i, (list, want)) in cases.iter().enumerate() {
            let group = CommentGroup {
                list: list
                    .iter()
                    .map(|s| Comment {
                        slash: Pos::default(),
                        text: (*s).to_string(),
                    })
                    .collect(),
            };
            let got = group.text();
            assert_eq!(got, *want, "case {}: got {:?}; expected {:?}", i, got, want);
        }
    }

    #[test]
    fn test_is_directive() {
        let tests: &[(&str, bool)] = &[
            ("abc", false),
            ("go:inline", true),
            ("Go:inline", false),
            ("go:Inline", false),
            (":inline", false),
            ("lint:ignore", true),
            ("lint:1234", true),
            ("1234:lint", true),
            ("go: inline", false),
            ("go:", false),
            ("go:*", false),
            ("go:x*", true),
            ("export foo", true),
            ("extern foo", true),
            ("expert foo", false),
        ];
        for (input, want) in tests {
            assert_eq!(
                is_directive(input),
                *want,
                "is_directive({:?}) = {}, want {}",
                input,
                is_directive(input),
                want
            );
        }
    }
}
