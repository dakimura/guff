//! SSA Instructions.

use guff::token::Token;
use guff_types::{ObjectId, TypeId};
use crate::ids::FuncId;
use crate::value::Value;

/// InstrData represents an SSA instruction.
/// (Go: `Instruction`)
#[derive(Debug)]
pub enum InstrData {
    // Instructions that are also values
    Alloc(Alloc),
    BinOp(BinOp),
    Call(Call),
    Convert(Convert),
    Extract(Extract),
    Field(Field),
    FieldAddr(FieldAddr),
    Index(Index),
    IndexAddr(IndexAddr),
    Lookup(Lookup),
    MakeChan(MakeChan),
    MakeClosure(MakeClosure),
    MakeInterface(MakeInterface),
    MakeMap(MakeMap),
    MakeSlice(MakeSlice),
    MultiConvert(MultiConvert),
    Next(Next),
    Phi(Phi),
    Range(Range),
    Select(Select),
    Slice(Slice),
    SliceToArrayPointer(SliceToArrayPointer),
    TypeAssert(TypeAssert),
    UnOp(UnOp),
    ChangeType(ChangeType),
    ChangeInterface(ChangeInterface),

    // Instructions that are only effects
    DebugRef(DebugRef),
    Defer(Defer),
    Go(Go),
    If(If),
    Jump(Jump),
    MapUpdate(MapUpdate),
    Panic(Panic),
    Return(Return),
    RunDefers(RunDefers),
    Send(Send),
    Store(Store),
}

// Placeholder structs for each instruction type.
// These will be populated in later chunks.

/// Alloc allocates a fresh cell (stack `local` or heap `new`) and yields its
/// address. (Go: `Alloc`)
#[derive(Debug)]
pub struct Alloc {
    /// The Alloc's own value type: the *pointer* type `*T`, where `T` is the
    /// allocated cell's type (Go: `Alloc.Type()` is `types.NewPointer(T)`). The
    /// disassembler derefs this to print `local T (comment)`.
    pub typ: TypeId,
    pub heap: bool,
    pub comment: String,
    pub index: i32, // index within Function.Locals (if lifted) or -1
}
#[derive(Debug)]
pub struct BinOp {
    pub op: Token,
    pub x: Value,
    pub y: Value,
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct CallCommon {
    pub value: Value,
    /// Non-`None` in interface invoke mode (`v.Method(args)`). (Go:
    /// `CallCommon.Method`.)
    pub method: Option<ObjectId>,
    pub args: Vec<Value>,
}
#[derive(Debug)]
pub struct Call {
    pub call: CallCommon,
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct Convert {}
/// Extract yields the `index`th component of the tuple value `tuple`.
/// (Go: `Extract`)
#[derive(Debug)]
pub struct Extract {
    pub tuple: Value,
    pub index: usize,
    pub typ: TypeId,
}
/// Field yields the value of the `field`th field of the struct value `x`.
/// (Go: `Field`)
#[derive(Debug)]
pub struct Field {
    pub x: Value,
    pub field: usize,
    /// the field's declared type (Go: `Field.Type()`).
    pub typ: TypeId,
}
/// FieldAddr yields the address of the `field`th field of the struct pointed to
/// by `x` (`x` has type `*struct`). (Go: `FieldAddr`)
#[derive(Debug)]
pub struct FieldAddr {
    pub x: Value,
    pub field: usize,
    /// the pointer-to-field type `*T` (Go: `FieldAddr.Type()`).
    pub typ: TypeId,
}
/// Index yields the value of `x[index]` where `x` is an array (held in a
/// register) or a string. Addressable arrays/slices use [`IndexAddr`] + load
/// instead. (Go: `Index`)
#[derive(Debug)]
pub struct Index {
    pub x: Value,
    pub index: Value,
    /// the element type `x`'s indexing yields (Go: `Index.Type()`).
    pub typ: TypeId,
}
/// IndexAddr yields the address `&x[index]` of an element of an addressable
/// array (`x` is `*array`) or slice. (Go: `IndexAddr`)
#[derive(Debug)]
pub struct IndexAddr {
    pub x: Value,
    pub index: Value,
    /// the pointer-to-element type `*T` (Go: `IndexAddr.Type()`).
    pub typ: TypeId,
}
/// Lookup yields the value of `x[index]` where `x` is a map or a string.
/// When `comma_ok` is set the result is a 2-tuple `(v, ok)` reporting whether
/// the key was present; otherwise it is the element value (the zero value if
/// absent). String indexing is never comma-ok. (Go: `Lookup`)
#[derive(Debug)]
pub struct Lookup {
    pub x: Value,
    pub index: Value,
    pub comma_ok: bool,
    /// the element type, or the 2-tuple `(elem, bool)` when `comma_ok`
    /// (Go: `Lookup.Type()`).
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct MakeChan {
    /// Channel buffer size (`0` or absent ⇒ unbuffered). (Go: `MakeChan.Size`.)
    pub size: Option<Value>,
    /// The channel type. (Go: `MakeChan.Type()`.)
    pub typ: TypeId,
}
/// MakeClosure yields a closure value whose code is `fn_` and whose free
/// variables' values are supplied by `bindings`. (Go: `MakeClosure`)
#[derive(Debug)]
pub struct MakeClosure {
    /// the anonymous function whose body the closure runs.
    pub fn_: FuncId,
    /// values bound to `fn_`'s free variables, one per [`crate::function::FreeVar`]
    /// in declaration order (i.e. each FreeVar's captured `outer` value, taken
    /// in the enclosing function's value-space).
    pub bindings: Vec<Value>,
    /// the closure's type (a `*types.Signature`).
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct MakeInterface {}
/// MakeMap yields a new, empty map (`make(map[K]V)` or an empty map literal).
/// `reserve`, when present, hints the initial capacity. (Go: `MakeMap`)
#[derive(Debug)]
pub struct MakeMap {
    /// initial space reservation (number of entries); `None` => default.
    pub reserve: Option<Value>,
    /// the map type (Go: `MakeMap.Type()`).
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct MakeSlice {
    pub len: Option<Value>,
    pub cap: Option<Value>,
    /// The resulting slice type. (Go: `MakeSlice.Type()`.)
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct MultiConvert {}
/// Next advances a map or string range iterator. (Go: `Next`)
#[derive(Debug)]
pub struct Next {
    pub iter: Value,
    pub is_string: bool,
    /// `(ok bool, key, value)` tuple type (Go: `Next.Type()`).
    pub typ: TypeId,
}
/// Range creates an iterator over a map or string. (Go: `Range`)
#[derive(Debug)]
pub struct Range {
    pub x: Value,
    /// opaque iterator type (Go: `tRangeIter`).
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct Phi {
    pub edges: Vec<Option<Value>>,
    pub comment: String,
    pub typ: TypeId,
}
/// One communication state of a [`Select`]. (Go: `SelectState`)
#[derive(Debug, Clone)]
pub struct SelectState {
    /// `SendOnly` or `RecvOnly`.
    pub dir: guff_types::ChanDir,
    /// Channel to send on / receive from.
    pub chan: Value,
    /// Value to send (`None` for receive).
    pub send: Option<Value>,
}

/// Select tests whether (or blocks until) one of the specified send/receive
/// states is entered. Returns the tuple
/// `(index int, recvOk bool, r0 T0, …, rn-1 Tn-1)`.
/// (Go: `Select`)
#[derive(Debug)]
pub struct Select {
    pub states: Vec<SelectState>,
    pub blocking: bool,
    /// Result tuple type. (Go: `Select.Type()`.)
    pub typ: TypeId,
}
/// Slice yields a slice of the sequence `x` (a slice, string, or `*array`)
/// bounded by the optional `low`/`high`/`max` indices, as in `x[low:high:max]`.
/// Composite slice literals use it to reslice a freshly built backing array
/// (`slice t[:]`). (Go: `Slice`)
#[derive(Debug)]
pub struct Slice {
    pub x: Value,
    pub low: Option<Value>,
    pub high: Option<Value>,
    pub max: Option<Value>,
    /// the resulting slice type (Go: `Slice.Type()`).
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct SliceToArrayPointer {}
#[derive(Debug)]
pub struct TypeAssert {
    pub x: Value,
    pub assert_type: TypeId,
    pub comma_ok: bool,
    /// the result type: `assert_type` for the single-value form, or the
    /// 2-tuple `(value assert_type, ok bool)` when `comma_ok`.
    /// (Go: `TypeAssert.Type()`.)
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct UnOp {
    pub op: Token,
    pub x: Value,
    pub comma_ok: bool,
    pub typ: TypeId,
}
/// ChangeType yields `x` converted to type `typ`, where the conversion has no
/// runtime effect (e.g. between a named type and its underlying type, or a
/// generic instance's concrete type and its type-parameter form).
/// (Go: `ChangeType`)
#[derive(Debug)]
pub struct ChangeType {
    pub x: Value,
    pub typ: TypeId,
}
#[derive(Debug)]
pub struct ChangeInterface {}

/// A DebugRef pseudo-instruction maps a source-level expression to the SSA
/// value `x` that represents its value (`is_addr == false`) or address
/// (`is_addr == true`). It has no dynamic effect and is emitted only when
/// debugging is enabled (see [`crate::builder::Builder::debug_info`]).
/// (Go: `DebugRef`)
#[derive(Debug)]
pub struct DebugRef {
    /// the value or address of the referring expression
    pub x: Value,
    /// true if `x` is the address that the (addressable) expression denotes
    pub is_addr: bool,
    /// the source var/func object when the expression is an `*ast.Ident`
    /// denoting one; `None` for non-ident expressions.
    pub object: Option<ObjectId>,
    /// the stable node id of the (unparenthesized) referring expression. This
    /// is the identity used by [`crate::function::Function::value_for_expr`] to
    /// recover the SSA value for a source expression. (Go: `DebugRef.Expr`,
    /// which go/ssa matches by pointer identity; `0` means unstamped/hand-built
    /// and never matches.)
    pub expr_id: u32,
    /// a description of the referring expression used by the disassembler:
    /// the reflect-style AST node name (e.g. `*ast.CallExpr`) for
    /// non-ident expressions. Only consulted when `object` is `None`.
    /// (Go: `reflect.TypeOf(s.Expr)`)
    pub expr_descr: String,
}
#[derive(Debug)]
pub struct Defer {
    pub call: CallCommon,
}
#[derive(Debug)]
pub struct Go {
    pub call: CallCommon,
}
#[derive(Debug)]
pub struct If {
    pub cond: Value,
}
#[derive(Debug)]
pub struct Jump {}
/// MapUpdate updates the association of `map[key]` to `value`. (Go: `MapUpdate`)
#[derive(Debug)]
pub struct MapUpdate {
    pub map: Value,
    pub key: Value,
    pub value: Value,
}
#[derive(Debug)]
pub struct Panic {
    pub x: Value,
}
#[derive(Debug)]
pub struct Return {
    pub results: Vec<Value>,
}
#[derive(Debug)]
pub struct RunDefers {}
/// Send sends `x` on channel `chan`. (Go: `Send`)
#[derive(Debug)]
pub struct Send {
    pub chan: Value,
    pub x: Value,
}
#[derive(Debug)]
pub struct Store {
    pub addr: Value,
    pub val: Value,
}

impl InstrData {
    /// is_value reports whether this instruction also defines a value (a
    /// register), i.e. whether it can be referenced as an operand and prints
    /// with a `tN = ` prefix. (Go: instruction implements the `Value` interface)
    pub fn is_value(&self) -> bool {
        matches!(
            self,
            InstrData::Alloc(_) | InstrData::BinOp(_) | InstrData::Call(_)
                | InstrData::Convert(_) | InstrData::Extract(_) | InstrData::Field(_)
                | InstrData::FieldAddr(_) | InstrData::Index(_) | InstrData::IndexAddr(_)
                | InstrData::Lookup(_) | InstrData::MakeChan(_) | InstrData::MakeClosure(_)
                | InstrData::MakeInterface(_) | InstrData::MakeMap(_) | InstrData::MakeSlice(_)
                | InstrData::MultiConvert(_) | InstrData::Next(_) | InstrData::Phi(_)
                | InstrData::Range(_) | InstrData::Select(_) | InstrData::Slice(_)
                | InstrData::SliceToArrayPointer(_) | InstrData::TypeAssert(_) | InstrData::UnOp(_)
                | InstrData::ChangeType(_) | InstrData::ChangeInterface(_)
        )
    }

    /// result_type returns the type of the value defined by this instruction,
    /// for the value-producing instructions whose type we track. Effect-only
    /// instructions (and value instructions whose type is not yet recorded)
    /// return `None`. (Go: `Value.Type()`)
    pub fn result_type(&self) -> Option<TypeId> {
        match self {
            InstrData::Alloc(a) => Some(a.typ),
            InstrData::BinOp(b) => Some(b.typ),
            InstrData::UnOp(u) => Some(u.typ),
            InstrData::Call(c) => Some(c.typ),
            InstrData::Phi(p) => Some(p.typ),
            InstrData::MakeClosure(m) => Some(m.typ),
            InstrData::Extract(e) => Some(e.typ),
            InstrData::Field(fld) => Some(fld.typ),
            InstrData::FieldAddr(fld) => Some(fld.typ),
            InstrData::ChangeType(c) => Some(c.typ),
            InstrData::Index(i) => Some(i.typ),
            InstrData::IndexAddr(i) => Some(i.typ),
            InstrData::Lookup(l) => Some(l.typ),
            InstrData::TypeAssert(t) => Some(t.typ),
            InstrData::MakeMap(m) => Some(m.typ),
            InstrData::MakeChan(c) => Some(c.typ),
            InstrData::MakeSlice(s) => Some(s.typ),
            InstrData::Slice(s) => Some(s.typ),
            InstrData::Range(r) => Some(r.typ),
            InstrData::Next(n) => Some(n.typ),
            InstrData::Select(s) => Some(s.typ),
            _ => None,
        }
    }

    /// for_each_operand calls f for each operand of this instruction.
    pub fn for_each_operand<F>(&self, mut f: F)
    where
        F: FnMut(&Value),
    {
        match self {
            InstrData::BinOp(i) => {
                f(&i.x);
                f(&i.y);
            }
            InstrData::UnOp(i) => {
                f(&i.x);
            }
            InstrData::Store(i) => {
                f(&i.addr);
                f(&i.val);
            }
            InstrData::Call(i) => {
                f(&i.call.value);
                for a in &i.call.args {
                    f(a);
                }
            }
            InstrData::Defer(i) => {
                f(&i.call.value);
                for a in &i.call.args {
                    f(a);
                }
            }
            InstrData::Go(i) => {
                f(&i.call.value);
                for a in &i.call.args {
                    f(a);
                }
            }
            InstrData::Phi(i) => {
                for e in &i.edges {
                    if let Some(v) = e {
                        f(v);
                    }
                }
            }
            InstrData::Index(i) => {
                f(&i.x);
                f(&i.index);
            }
            InstrData::IndexAddr(i) => {
                f(&i.x);
                f(&i.index);
            }
            InstrData::Slice(i) => {
                f(&i.x);
                if let Some(v) = &i.low {
                    f(v);
                }
                if let Some(v) = &i.high {
                    f(v);
                }
                if let Some(v) = &i.max {
                    f(v);
                }
            }
            InstrData::MakeMap(i) => {
                if let Some(v) = &i.reserve {
                    f(v);
                }
            }
            InstrData::MakeChan(i) => {
                if let Some(v) = &i.size {
                    f(v);
                }
            }
            InstrData::MakeSlice(i) => {
                if let Some(v) = &i.len {
                    f(v);
                }
                if let Some(v) = &i.cap {
                    f(v);
                }
            }
            InstrData::TypeAssert(i) => {
                f(&i.x);
            }
            InstrData::Lookup(i) => {
                f(&i.x);
                f(&i.index);
            }
            InstrData::MapUpdate(i) => {
                f(&i.map);
                f(&i.key);
                f(&i.value);
            }
            InstrData::Return(i) => {
                for r in &i.results {
                    f(r);
                }
            }
            InstrData::If(i) => {
                f(&i.cond);
            }
            InstrData::DebugRef(i) => {
                f(&i.x);
            }
            InstrData::MakeClosure(i) => {
                for b in &i.bindings {
                    f(b);
                }
            }
            InstrData::Extract(i) => {
                f(&i.tuple);
            }
            InstrData::Range(i) => {
                f(&i.x);
            }
            InstrData::Next(i) => {
                f(&i.iter);
            }
            InstrData::Field(i) => {
                f(&i.x);
            }
            InstrData::FieldAddr(i) => {
                f(&i.x);
            }
            InstrData::ChangeType(i) => {
                f(&i.x);
            }
            InstrData::Select(i) => {
                for st in &i.states {
                    f(&st.chan);
                    if let Some(v) = &st.send {
                        f(v);
                    }
                }
            }
            InstrData::Send(i) => {
                f(&i.chan);
                f(&i.x);
            }
            InstrData::Panic(i) => {
                f(&i.x);
            }
            _ => {}
        }
    }

    /// for_each_operand_mut calls f for each operand of this instruction, allowing mutation.
    pub fn for_each_operand_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut Value),
    {
        match self {
            InstrData::BinOp(i) => {
                f(&mut i.x);
                f(&mut i.y);
            }
            InstrData::UnOp(i) => {
                f(&mut i.x);
            }
            InstrData::Store(i) => {
                f(&mut i.addr);
                f(&mut i.val);
            }
            InstrData::Call(i) => {
                f(&mut i.call.value);
                for a in &mut i.call.args {
                    f(a);
                }
            }
            InstrData::Defer(i) => {
                f(&mut i.call.value);
                for a in &mut i.call.args {
                    f(a);
                }
            }
            InstrData::Go(i) => {
                f(&mut i.call.value);
                for a in &mut i.call.args {
                    f(a);
                }
            }
            InstrData::Phi(i) => {
                for e in &mut i.edges {
                    if let Some(v) = e {
                        f(v);
                    }
                }
            }
            InstrData::Index(i) => {
                f(&mut i.x);
                f(&mut i.index);
            }
            InstrData::IndexAddr(i) => {
                f(&mut i.x);
                f(&mut i.index);
            }
            InstrData::Slice(i) => {
                f(&mut i.x);
                if let Some(v) = &mut i.low {
                    f(v);
                }
                if let Some(v) = &mut i.high {
                    f(v);
                }
                if let Some(v) = &mut i.max {
                    f(v);
                }
            }
            InstrData::MakeMap(i) => {
                if let Some(v) = &mut i.reserve {
                    f(v);
                }
            }
            InstrData::MakeChan(i) => {
                if let Some(v) = &mut i.size {
                    f(v);
                }
            }
            InstrData::MakeSlice(i) => {
                if let Some(v) = &mut i.len {
                    f(v);
                }
                if let Some(v) = &mut i.cap {
                    f(v);
                }
            }
            InstrData::TypeAssert(i) => {
                f(&mut i.x);
            }
            InstrData::Lookup(i) => {
                f(&mut i.x);
                f(&mut i.index);
            }
            InstrData::MapUpdate(i) => {
                f(&mut i.map);
                f(&mut i.key);
                f(&mut i.value);
            }
            InstrData::Return(i) => {
                for r in &mut i.results {
                    f(r);
                }
            }
            InstrData::If(i) => {
                f(&mut i.cond);
            }
            InstrData::DebugRef(i) => {
                f(&mut i.x);
            }
            InstrData::MakeClosure(i) => {
                for b in &mut i.bindings {
                    f(b);
                }
            }
            InstrData::Extract(i) => {
                f(&mut i.tuple);
            }
            InstrData::Range(i) => {
                f(&mut i.x);
            }
            InstrData::Next(i) => {
                f(&mut i.iter);
            }
            InstrData::Field(i) => {
                f(&mut i.x);
            }
            InstrData::FieldAddr(i) => {
                f(&mut i.x);
            }
            InstrData::ChangeType(i) => {
                f(&mut i.x);
            }
            InstrData::Select(i) => {
                for st in &mut i.states {
                    f(&mut st.chan);
                    if let Some(v) = &mut st.send {
                        f(v);
                    }
                }
            }
            InstrData::Send(i) => {
                f(&mut i.chan);
                f(&mut i.x);
            }
            InstrData::Panic(i) => {
                f(&mut i.x);
            }
            _ => {}
        }
    }
}
