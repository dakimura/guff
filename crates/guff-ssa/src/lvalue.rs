//! SSA L-values (assignable locations).
//!
//! Port of go/ssa's `lvalue` interface and implementations.

use crate::builder::Builder;
use crate::value::Value;
use guff::ast::Expr;
use guff::Pos;
use guff_types::TypeId;

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
}

/// StoreBuf accumulates deferred stores so that a group of assignments can
/// evaluate all right-hand sides before any store takes effect. This preserves
/// left-to-right evaluation with correct semantics for parallel assignment
/// (`x, y = y, x`) and for composite literals that may reference the location
/// being initialized. (Go: `storebuf`.)
#[derive(Default)]
pub struct StoreBuf {
    stores: Vec<(Box<dyn LValue>, Value)>,
}

impl StoreBuf {
    pub fn new() -> Self {
        Self { stores: Vec::new() }
    }

    /// Appends a deferred store of `rhs` into `lhs`. (Go: `storebuf.store`.)
    pub fn store(&mut self, lhs: Box<dyn LValue>, rhs: Value) {
        self.stores.push((lhs, rhs));
    }

    /// Emits all buffered stores, in the order they were appended. (Go:
    /// `storebuf.emit`.)
    pub fn emit(self, b: &mut Builder) {
        for (lhs, rhs) in self.stores {
            lhs.store(b, rhs);
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
    /// `None` for synthetic locations (e.g. composite-literal element slots).
    /// (Go: `address.expr`)
    pub expr: Option<Expr>,
}

impl LValue for Address {
    fn store(&self, b: &mut Builder, v: Value) {
        b.emit_store(self.addr, v, self.pos);
        if let Some(expr) = &self.expr {
            // store.Val is `v` (caller coerces for assignability).
            b.emit_debug_ref(expr, v, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        b.emit_load(self.addr, self.typ)
    }

    fn address(&self, b: &mut Builder) -> Value {
        if let Some(expr) = &self.expr {
            b.emit_debug_ref(expr, self.addr, true);
        }
        self.addr
    }

    fn typ(&self) -> TypeId {
        self.typ
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
    pub expr: Option<Expr>,
}

impl LazyAddress {
    /// emit_addr runs the deferred address computation, emitting the field
    /// address instruction into the builder's current block. (Go: `l.addr(fn)`.)
    fn emit_addr(&self, b: &mut Builder) -> Value {
        let block = b.block.expect("no current block");
        crate::emit::emit_field_selection(
            b.prog, b.func_id, block, self.recv, self.field, /*want_addr*/ true, self.pos,
        )
    }
}

impl LValue for LazyAddress {
    fn store(&self, b: &mut Builder, v: Value) {
        let addr = self.emit_addr(b);
        b.emit_store(addr, v, self.pos);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref(expr, v, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        // DEFERRED vs go/ssa: `load.pos = l.pos` (load position tracking).
        b.emit_load(addr, self.typ)
    }

    fn address(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref(expr, addr, true);
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
    pub expr: Option<Expr>,
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
        b.emit_store(addr, v, self.pos);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref(expr, v, false);
        }
    }

    fn load(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        b.emit_load(addr, self.typ)
    }

    fn address(&self, b: &mut Builder) -> Value {
        let addr = self.emit_addr(b);
        if let Some(expr) = &self.expr {
            b.emit_debug_ref(expr, addr, true);
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
