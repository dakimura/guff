//! SSA L-values (assignable locations).
//!
//! Port of go/ssa's `lvalue` interface and implementations.

use crate::builder::Builder;
use crate::value::Value;
use guff::ast::{Expr, Ident};
use guff::Pos;
use guff_types::TypeId;

/// The parts of a source expression that a DebugRef records, captured when an
/// lvalue is built so the lvalue need not own a deep copy of the syntax.
///
/// go/ssa keeps the `ast.Expr` pointer itself (`address.expr`,
/// `lazyAddress.expr`); [`Builder::emit_debug_ref`] only ever reads the node
/// id, its position, its reflect name, and — for an identifier — the object it
/// denotes and whether it is blank. Cloning the `Expr` instead copied whole
/// subtrees (`a.b[i].c = …`, `T{…}` with every element) on every assignment,
/// even when debug info was off.
#[derive(Clone, Copy, Debug)]
pub struct DebugExpr {
    pub(crate) id: u32,
    pub(crate) pos: Pos,
    pub(crate) descr: &'static str,
    /// `Some(is_blank)` when the (unparenthesized) expression is an identifier.
    pub(crate) ident: Option<bool>,
}

impl DebugExpr {
    /// Captures `e` after stripping parentheses, as `emitDebugRef` does.
    pub fn of(e: &Expr) -> Self {
        let e = crate::builder::unparen(e);
        match e {
            Expr::Ident(id) => Self::of_ident(id),
            _ => Self {
                id: e.id(),
                pos: e.pos(),
                descr: crate::builder::expr_reflect_name(e),
                ident: None,
            },
        }
    }

    /// Same as `DebugExpr::of(&Expr::Ident(id.clone()))`, without the clone.
    pub fn of_ident(id: &Ident) -> Self {
        Self {
            id: id.id,
            pos: id.name_pos,
            descr: "*ast.Ident",
            ident: Some(id.name == "_"),
        }
    }

    /// Same as `DebugExpr::of(&Expr::IndexExpr(ie.clone()))`, without the clone.
    pub fn of_index(ie: &guff::ast::IndexExpr) -> Self {
        Self { id: ie.id, pos: ie.x.pos(), descr: "*ast.IndexExpr", ident: None }
    }

    /// Same as `DebugExpr::of(&Expr::CompositeLit(cl.clone()))`, without the clone.
    pub fn of_composite_lit(cl: &guff::ast::CompositeLit) -> Self {
        Self {
            id: cl.id,
            pos: cl.ty.as_ref().map(|t| t.pos()).unwrap_or(cl.lbrace),
            descr: "*ast.CompositeLit",
            ident: None,
        }
    }
}

/// LValue represents an assignable location.
/// (Go: `lvalue` interface)
pub trait LValue {
    fn store(&self, b: &mut Builder, v: Value);
    fn load(&self, b: &mut Builder) -> Value;
    fn address(&self, b: &mut Builder) -> Value;
    fn typ(&self) -> TypeId;
    /// Reports whether this is the blank lvalue (`_`). Used by `assign` to avoid
    /// querying `typ()` on a blank location. (Go: `is[blank](loc)`.)
    fn is_blank(&self) -> bool {
        false
    }
    /// Reports whether this is a plain [`Address`] — the only lvalue that
    /// honnef's IR initializes a composite literal into in place. (Go, honnef
    /// go/ir v0.8.2: `loc.(*address)` in `builder.assign`.)
    fn is_address(&self) -> bool {
        false
    }
}

/// StoreBuf accumulates deferred stores so that a group of assignments can
/// evaluate all right-hand sides before any store takes effect. This preserves
/// left-to-right evaluation with correct semantics for parallel assignment
/// (`x, y = y, x`) and for composite literals that may reference the location
/// being initialized. (Go: `storebuf`.)
#[derive(Default)]
pub struct StoreBuf {
    stores: Vec<Deferred>,
}

/// One entry of a [`StoreBuf`]: a store, or a DebugRef that must not be
/// emitted before the stores queued ahead of it. (Go, honnef go/ir v0.8.2:
/// `store`, whose `debugRef` field excludes the others.)
enum Deferred {
    Store(Box<dyn LValue>, Value),
    /// The address DebugRef of a composite literal initialized in place.
    AddrDebugRef(DebugExpr, Value),
}

impl StoreBuf {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    /// Appends a deferred store of `rhs` into `lhs`. (Go: `storebuf.store`.)
    pub fn store(&mut self, lhs: Box<dyn LValue>, rhs: Value) {
        self.stores.push(Deferred::Store(lhs, rhs));
    }

    /// Appends a DebugRef recording that `e` lives at address `addr`, emitted
    /// after the stores appended so far. (Go, honnef go/ir v0.8.2:
    /// `storebuf.storeDebugRef`.)
    pub fn store_addr_debug_ref(&mut self, e: DebugExpr, addr: Value) {
        self.stores.push(Deferred::AddrDebugRef(e, addr));
    }

    /// Emits all buffered stores, in the order they were appended. (Go:
    /// `storebuf.emit`.)
    pub fn emit(self, b: &mut Builder) {
        for d in self.stores {
            match d {
                Deferred::Store(lhs, rhs) => lhs.store(b, rhs),
                Deferred::AddrDebugRef(e, addr) => b.emit_debug_ref_at(e, addr, true),
            }
        }
    }
}

/// address is an lvalue represented by a memory address.
pub struct Address {
    pub addr: Value,
    pub typ: TypeId,
    /// position of the source syntax that denotes this location, used as the
    /// position of stores through it. (Go: `address.pos`)
    pub pos: Pos,
    /// Source syntax of the value (not address), for DebugRef in debug mode.
    /// `None` for synthetic locations (e.g. the memclear of a partial composite
    /// literal).
    /// (Go: `address.expr`)
    pub expr: Option<DebugExpr>,
}

impl LValue for Address {
    fn store(&self, b: &mut Builder, v: Value) {
        // store.Val: `v` converted for assignability.
        let stored = b.emit_store(self.addr, v, self.pos);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, stored, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        b.emit_load_at(self.addr, self.typ, self.pos)
    }

    fn address(&self, b: &mut Builder) -> Value {
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, self.addr, true);
        }
        self.addr
    }

    fn typ(&self) -> TypeId {
        self.typ
    }

    fn is_address(&self) -> bool {
        true
    }
}

/// LazyAddress is an lvalue whose address computation is deferred until the
/// first `store`/`load`/`address` call. Deferring lets go/ssa control *when*
/// a side effect of using the location (e.g. a nil-pointer dereference in
/// `x.f = p()`) happens relative to evaluating the value being stored: the
/// receiver `x` is emitted eagerly when the lvalue is built, but the field
/// address instruction is only emitted at use time. (Go: `lazyAddress`.)
///
/// This port covers the struct-field-selection case (`x.f`): the deferred
/// computation is [`crate::emit::emit_field_selection`] with `want_addr`, on
/// the already-emitted receiver `recv`.
pub struct LazyAddress {
    /// The already-emitted receiver value/address of the enclosing struct
    /// (a struct value, or a `*struct`).
    pub recv: Value,
    /// The final explicit field index within the receiver's struct type.
    pub field: usize,
    /// Type of the location (the selected field's type).
    pub typ: TypeId,
    /// Source position of the field selector, used for the store/load.
    pub pos: Pos,
    /// Source syntax of the value (typically the field `Ident`), for DebugRef.
    /// (Go: `lazyAddress.expr`)
    pub expr: Option<DebugExpr>,
}

impl LazyAddress {
    /// emit_addr runs the deferred address computation, emitting the field
    /// address instruction into the builder's current block. (Go: `l.addr(fn)`.)
    fn emit_addr(&self, b: &mut Builder) -> Value {
        let block = b.block.expect("no current block");
        let addr = crate::emit::emit_field_selection(
            b.prog, b.func_id, block, self.recv, self.field, /*want_addr*/ true, self.pos,
        );
        // go/ssa's `emitFieldSelection` ends with
        // `emitDebugRef(f, id, v, wantAddr)`; `store`/`address` then add
        // their own ref to the same Ident, as `lazyAddress` does.
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, addr, true);
        }
        addr
    }
}

impl LValue for LazyAddress {
    fn store(&self, b: &mut Builder, v: Value) {
        let addr = self.emit_addr(b);
        // store.Val: `v` converted for assignability.
        let stored = b.emit_store(addr, v, self.pos);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, stored, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        b.emit_load_at(addr, self.typ, self.pos)
    }

    fn address(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, addr, true);
        }
        addr
    }

    fn typ(&self) -> TypeId {
        self.typ
    }
}

/// Element is an lvalue for a map element `m[k]`. A map element has no
/// address: `load` emits a (non-comma-ok) [`Lookup`](crate::instr::Lookup) and
/// `store` emits a [`MapUpdate`](crate::instr::MapUpdate). The map `m` and key
/// `k` are evaluated eagerly when the lvalue is built; only the Lookup/MapUpdate
/// instruction is deferred to use time. (Go: `element`.)
pub struct Element {
    /// the map value.
    pub m: Value,
    /// the key value (already converted to the map's key type).
    pub k: Value,
    /// the map's element type (Go: `element.t`).
    pub typ: TypeId,
    /// source position of the `m[k]` (or `{k:v}`) syntax.
    pub pos: Pos,
}

impl LValue for Element {
    fn store(&self, b: &mut Builder, v: Value) {
        let block = b.block.expect("no current block");
        // Convert the stored value to the map's element type (Go: emitConv).
        let value = crate::emit::emit_type_coercion(b.prog, b.func_id, block, v, self.typ);
        crate::emit::emit_with_pos(
            b.func_mut(),
            block,
            crate::instr::InstrData::MapUpdate(crate::instr::MapUpdate {
                map: self.m,
                key: self.k,
                value,
            }),
            self.pos,
        );
    }

    fn load(&self, b: &mut Builder) -> Value {
        let block = b.block.expect("no current block");
        let id = crate::emit::emit_with_pos(
            b.func_mut(),
            block,
            crate::instr::InstrData::Lookup(crate::instr::Lookup {
                x: self.m,
                index: self.k,
                comma_ok: false,
                typ: self.typ,
            }),
            self.pos,
        );
        Value::Instr(id)
    }

    fn address(&self, _b: &mut Builder) -> Value {
        panic!("map elements are not addressable");
    }

    fn typ(&self) -> TypeId {
        self.typ
    }
}

/// LazyIndexAddr is an lvalue for an element of an addressable array or slice
/// (`x[index]`). Like [`LazyAddress`], it defers the address instruction
/// ([`IndexAddr`](crate::instr::IndexAddr), `&x[index]`) to use time so that a
/// panic from an out-of-bounds or nil `x` happens after the value being stored
/// is evaluated (the two phases of `AssignStmt`). The container `x` and index
/// are evaluated eagerly. (Go: the `lazyAddress` built by `builder.addr`'s
/// `*ast.IndexExpr` case.)
pub struct LazyIndexAddr {
    /// the container: `*array` (ixArrVar) or slice value (ixVar).
    pub x: Value,
    /// the index value.
    pub index: Value,
    /// the pointer-to-element type `*T` of the emitted `IndexAddr`.
    pub et: TypeId,
    /// the element type `T` (the location's type = `MustDeref(et)`).
    pub typ: TypeId,
    /// source position of the `[` in `x[index]`.
    pub pos: Pos,
    /// Source syntax of the index expression, for DebugRef. (Go: `lazyAddress.expr`)
    pub expr: Option<DebugExpr>,
}

impl LazyIndexAddr {
    fn emit_addr(&self, b: &mut Builder) -> Value {
        let block = b.block.expect("no current block");
        crate::emit::emit_index_addr(b.prog, b.func_id, block, self.x, self.index, self.et, self.pos)
    }
}

impl LValue for LazyIndexAddr {
    fn store(&self, b: &mut Builder, v: Value) {
        let addr = self.emit_addr(b);
        // store.Val: `v` converted for assignability.
        let stored = b.emit_store(addr, v, self.pos);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, stored, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        b.emit_load(addr, self.typ)
    }

    fn address(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref_at(*expr, addr, true);
        }
        addr
    }

    fn typ(&self) -> TypeId {
        self.typ
    }
}

/// blank is an lvalue for the blank identifier `_`.
pub struct Blank;

impl LValue for Blank {
    fn store(&self, _b: &mut Builder, _v: Value) {
        // do nothing
    }

    fn load(&self, _b: &mut Builder) -> Value {
        panic!("blank lvalue has no value");
    }

    fn address(&self, _b: &mut Builder) -> Value {
        panic!("blank lvalue has no address");
    }

    fn typ(&self) -> TypeId {
        panic!("blank lvalue has no type");
    }

    fn is_blank(&self) -> bool {
        true
    }
}
