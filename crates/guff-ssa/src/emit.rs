//! SSA instruction emission helpers.
//!
//! Port of go/ssa's `emit.go`.

use crate::function::Function;
use crate::ids::{BlockId, FuncId, InstrId};
use crate::instr::{
    Alloc, Call, CallCommon, ChangeType, Convert, Extract, Field, FieldAddr, IndexAddr, InstrData,
    Return, Store, TypeAssert, UnOp,
};
use crate::program::{value_type_of, Program};
use crate::value::Value;
use guff::token::Token;
use guff::{Pos, NO_POS};
use guff_types::arena::TypeData;
use guff_types::{
    empty_tuple, identical, is_pointer, new_pointer, new_tuple, new_var, pointer_elem,
    signature_results, struct_field, tuple_at, tuple_len, BasicKind, ObjectData, ObjectId, TypeId,
};

/// emit adds the instruction `data` to the end of the specified `block`,
/// with no source position. (Go: `Function.emit`)
pub fn emit(f: &mut Function, block: BlockId, data: InstrData) -> InstrId {
    emit_with_pos(f, block, data, NO_POS)
}

/// emit_with_pos adds `data` to the end of `block` and records `pos` as the
/// instruction's source position. This is the position-carrying form of
/// [`emit`]; go/ssa sets `pos` on the instruction struct before calling
/// `f.emit`.
pub fn emit_with_pos(f: &mut Function, block: BlockId, data: InstrData, pos: Pos) -> InstrId {
    let id = f.instrs.alloc(data);
    f.set_pos(id, pos);
    f.blocks.get_mut(block).instrs.push(id);
    id
}

/// emit_load emits a load instruction (`*addr`).
pub fn emit_load(f: &mut Function, block: BlockId, addr: Value, typ: TypeId) -> Value {
    let id = emit(f, block, InstrData::UnOp(UnOp {
        op: Token::MUL,
        x: addr,
        comma_ok: false,
        typ,
    }));
    Value::Instr(id)
}

/// emit_alloc emits an `Alloc` of a fresh cell holding a value of type `typ`
/// (the pointee) and returns the Alloc value. Its own value type is `*typ`
/// (Go: `Alloc.Type()`): go/ssa's `emitAlloc` sets the register's type to
/// `types.NewPointer(typ)`, so the pointer type is interned here at emit time.
/// `comment` labels the cell in the disassembly (`local <typ> (<comment>)`).
pub fn emit_alloc(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    typ: TypeId,
    pos: Pos,
    comment: String,
) -> Value {
    let ptr = new_pointer(&mut prog.type_arena, typ);
    let id = emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::Alloc(Alloc { typ: ptr, heap: false, comment, index: -1 }),
        pos,
    );
    Value::Instr(id)
}

/// emit_new emits a heap-allocated `Alloc` of type `typ` (`new T`), used for
/// escaping composite literals (`&T{…}`) and other allocations that outlive the
/// activation. Unlike [`emit_local`] it is *not* recorded in `locals`. (Go:
/// `emitNew`)
pub fn emit_new(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    typ: TypeId,
    pos: Pos,
    comment: String,
) -> Value {
    let ptr = new_pointer(&mut prog.type_arena, typ);
    let id = emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::Alloc(Alloc { typ: ptr, heap: true, comment, index: -1 }),
        pos,
    );
    Value::Instr(id)
}

/// emit_local emits a stack-allocated `Alloc` of type `typ` and records it in
/// the function's `locals` list. (Go: `emitLocal`)
pub fn emit_local(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    typ: TypeId,
    pos: Pos,
    comment: String,
) -> Value {
    let v = emit_alloc(prog, fid, block, typ, pos, comment);
    if let Value::Instr(id) = v {
        prog.functions.get_mut(fid).locals.push(id);
    }
    v
}

/// emit_local_var emits a stack local for the type-checker variable `obj` and
/// records its address in the function's `objects` map, so later references to
/// `obj` resolve to this cell. (Go: `emitLocalVar`, which also populates the
/// `f.vars` map keyed by the `*types.Var`.)
pub fn emit_local_var(prog: &mut Program, fid: FuncId, block: BlockId, obj: ObjectId) -> Value {
    let typ = obj.typ(&prog.object_arena).expect("local var has a type");
    let comment = obj.name(&prog.object_arena).to_string();
    let v = emit_local(prog, fid, block, typ, NO_POS, comment);
    prog.functions.get_mut(fid).objects.insert(obj, v);
    v
}

/// emit_store emits a store instruction (`*addr = val`) at position `pos`.
/// (Go: `emitStore`)
pub fn emit_store(f: &mut Function, block: BlockId, addr: Value, val: Value, pos: Pos) {
    emit_with_pos(f, block, InstrData::Store(Store {
        addr,
        val,
    }), pos);
}

/// emit_extract emits an instruction to extract the `index`th component of the
/// tuple value `tuple`, and returns the extracted value. Its type is the
/// `index`th element of `tuple`'s tuple type. (Go: `emitExtract`)
pub fn emit_extract(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    tuple: Value,
    index: usize,
) -> Value {
    let tuple_ty = value_type_of(prog, prog.functions.get(fid), tuple);
    let typ = tuple_elem_type(prog, tuple_ty, index);
    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Extract(Extract { tuple, index, typ }),
    );
    Value::Instr(id)
}

/// The type of the `index`th element (a `Var`) of tuple type `tuple`.
fn tuple_elem_type(prog: &Program, tuple: TypeId, index: usize) -> TypeId {
    let var = tuple_at(&prog.type_arena, tuple, index);
    match prog.object_arena.get(var) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("tuple element is not a Var"),
    }
}

/// emit_type_coercion emits `v` coerced to `typ` via a [`ChangeType`], or
/// returns `v` unchanged if it already has that type. Used to reconcile a
/// generic instance's concrete types with the origin's type-parameter form.
/// (Go: `emitTypeCoercion`)
pub fn emit_type_coercion(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    v: Value,
    typ: TypeId,
) -> Value {
    let vt = value_type_of(prog, prog.functions.get(fid), v);
    if identical(&mut prog.type_arena, &prog.object_arena, &prog.package_arena, vt, typ) {
        return v; // no coercion needed
    }
    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::ChangeType(ChangeType { x: v, typ }),
    );
    Value::Instr(id)
}

/// emit_conv emits code to convert `val` to exactly type `typ`.
///
/// This is a pragmatic subset of go/ssa's `emitConv`: identical types are a
/// no-op; same-underlying (named ↔ underlying) uses [`ChangeType`]; everything
/// else uses [`Convert`]. Full interface / slice-to-array / multi-convert
/// cases remain DEFERRED — enough to lower explicit `T(x)` conversions during
/// buildir without panicking.
/// (Go: `emitConv`)
pub fn emit_conv(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    val: Value,
    typ: TypeId,
) -> Value {
    let t_src = value_type_of(prog, prog.functions.get(fid), val);
    if identical(
        &mut prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        t_src,
        typ,
    ) {
        return val;
    }

    let ut_src = t_src.underlying(&prog.type_arena);
    let ut_dst = typ.underlying(&prog.type_arena);
    if identical(
        &mut prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        ut_src,
        ut_dst,
    ) {
        let id = emit(
            prog.functions.get_mut(fid),
            block,
            InstrData::ChangeType(ChangeType { x: val, typ }),
        );
        return Value::Instr(id);
    }

    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Convert(Convert { x: val, typ }),
    );
    Value::Instr(id)
}

/// emit_type_test emits a comma-ok type assertion `x.(t)`, yielding the 2-tuple
/// `(value t, ok bool)`: the asserted value together with a boolean reporting
/// whether the assertion held (so it never panics). The result tuple is built
/// with named components `value`/`ok`, matching go/ssa's disassembly.
/// (Go: `emitTypeTest`.)
pub fn emit_type_test(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    x: Value,
    t: TypeId,
    pos: Pos,
) -> Value {
    let ok_ty = prog.basic_type(BasicKind::Bool);
    let value_var = new_var(&mut prog.object_arena, "value", t);
    let ok_var = new_var(&mut prog.object_arena, "ok", ok_ty);
    let typ = new_tuple(&mut prog.type_arena, &[value_var, ok_var])
        .expect("2-tuple is never empty");
    let id = emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::TypeAssert(TypeAssert {
            x,
            assert_type: t,
            comma_ok: true,
            typ,
        }),
        pos,
    );
    Value::Instr(id)
}

/// emit_call emits a call instruction with the given call components and result
/// type, returning the call's result value. (Go: `emitCall`)
///
/// DEFERRED vs. go/ssa: the "no-return callee" handling (inserting a `Panic`
/// and an unreachable block after a call to a function known never to return)
/// is omitted — `Program.noReturn` is not yet modelled.
pub fn emit_call(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    call: CallCommon,
    typ: TypeId,
) -> Value {
    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Call(Call { call, typ }),
    );
    Value::Instr(id)
}

/// emit_tail_call emits a function call in tail position: the call's result
/// becomes function `fid`'s return value(s). The caller fills all of `call`
/// except its type (derived here from `fid`'s signature results). Intended for
/// wrapper methods. (Go: `emitTailCall`)
///
/// Mirroring go/ssa, `call.typ` is set from `fid`'s results: the sole result's
/// type when there is one, the results tuple when there are several, and the
/// empty tuple (`()`) when there are none. In the 0-result case the emitted
/// call value is unused and the trailing `return` carries no operands.
pub fn emit_tail_call(prog: &mut Program, fid: FuncId, block: BlockId, call: CallCommon) {
    let sig = prog
        .functions
        .get(fid)
        .signature
        .expect("tail call in a function with a signature");
    let results = signature_results(&prog.type_arena, sig);
    let nr = tuple_len(&prog.type_arena, results);

    // call.typ: no result → the empty tuple (go's nil `*Tuple`, printed `()`);
    // one result → that result's type; several → the results tuple.
    let call_typ = match nr {
        0 => empty_tuple(&mut prog.type_arena),
        1 => tuple_elem_type(prog, results.unwrap(), 0),
        _ => results.unwrap(),
    };

    let tuple = emit_call(prog, fid, block, call, call_typ);

    // Return value(s): none, the single result, or each extracted component.
    // go/ssa relies on the wrapper's result types matching the callee's exactly,
    // so no coercion is applied.
    let ret_results: Vec<Value> = match nr {
        0 => Vec::new(),
        1 => vec![tuple],
        _ => (0..nr).map(|i| emit_extract(prog, fid, block, tuple, i)).collect(),
    };
    emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Return(Return { results: ret_results }),
    );
}

/// field_of returns the `index`th field (a `Var` object) of the struct that
/// `typ`'s underlying type is. (Go: `fieldOf` — returns `None` for a non-struct,
/// reached under incomplete hybrid type info rather than panicking.)
pub(crate) fn field_of(prog: &Program, typ: TypeId, index: usize) -> Option<ObjectId> {
    let u = typ.underlying(&prog.type_arena);
    if !matches!(prog.type_arena.get(u), TypeData::Struct(_)) {
        return None;
    }
    Some(struct_field(&prog.type_arena, u, index))
}

/// emit_field_selection emits the field selection `v.index`, returning the
/// field's address when `want_addr` is set, else its value. If `v` is a pointer
/// to a struct it emits a `FieldAddr` (and a `Load` unless `want_addr`); if `v`
/// is a struct value it emits a `Field`. (Go: `emitFieldSelection`, minus the
/// trailing `emitDebugRef` which is deferred.)
pub fn emit_field_selection(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    v: Value,
    index: usize,
    want_addr: bool,
    pos: Pos,
) -> Value {
    let vt = value_type_of(prog, prog.functions.get(fid), v);
    if is_pointer(&prog.type_arena, vt) {
        let pointee = pointer_elem(&prog.type_arena, vt);
        let Some(fld) = field_of(prog, pointee, index) else {
            return emit_invalid_zero(prog);
        };
        let fld_ty = fld.typ(&prog.object_arena).expect("field has a type");
        let ptr_ty = new_pointer(&mut prog.type_arena, fld_ty);
        let id = emit_with_pos(
            prog.functions.get_mut(fid),
            block,
            InstrData::FieldAddr(FieldAddr { x: v, field: index, typ: ptr_ty }),
            pos,
        );
        let addr = Value::Instr(id);
        if want_addr {
            addr
        } else {
            emit_load(prog.functions.get_mut(fid), block, addr, fld_ty)
        }
    } else {
        let Some(fld) = field_of(prog, vt, index) else {
            return emit_invalid_zero(prog);
        };
        let fld_ty = fld.typ(&prog.object_arena).expect("field has a type");
        let id = emit_with_pos(
            prog.functions.get_mut(fid),
            block,
            InstrData::Field(Field { x: v, field: index, typ: fld_ty }),
            pos,
        );
        Value::Instr(id)
    }
}

/// emit_index_addr emits `&x[index]`, the address of an element of an
/// addressable array (`x` is `*array`) or slice. `et` is the result's
/// pointer-to-element type `*T`. (Go: the `IndexAddr` emitted by `builder.addr`'s
/// `*ast.IndexExpr` case.)
pub fn emit_index_addr(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    x: Value,
    index: Value,
    et: TypeId,
    pos: Pos,
) -> Value {
    let id = emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::IndexAddr(IndexAddr { x, index, typ: et }),
        pos,
    );
    Value::Instr(id)
}

/// emit_implicit_selections emits the chain of (possibly promoted) embedded
/// field selections along `indices`, returning the value or address of the
/// final one — the implicit part of an `x.f0.f1...f` selection, excluding the
/// last explicit field. A struct value uses `Field`; a struct pointer uses
/// `FieldAddr`, additionally loading when the embedded field is itself a
/// pointer (indirect embedding). (Go: `emitImplicitSelections`.)
pub fn emit_implicit_selections(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    mut v: Value,
    indices: &[usize],
    pos: Pos,
) -> Value {
    for &index in indices {
        let vt = value_type_of(prog, prog.functions.get(fid), v);
        if is_pointer(&prog.type_arena, vt) {
            let pointee = pointer_elem(&prog.type_arena, vt);
            let Some(fld) = field_of(prog, pointee, index) else {
                return emit_invalid_zero(prog);
            };
            let fld_ty = fld.typ(&prog.object_arena).expect("field has a type");
            let ptr_ty = new_pointer(&mut prog.type_arena, fld_ty);
            let id = emit_with_pos(
                prog.functions.get_mut(fid),
                block,
                InstrData::FieldAddr(FieldAddr { x: v, field: index, typ: ptr_ty }),
                pos,
            );
            v = Value::Instr(id);
            // Load the field's value iff indirectly embedded (a pointer field).
            if is_pointer(&prog.type_arena, fld_ty) {
                v = emit_load(prog.functions.get_mut(fid), block, v, fld_ty);
            }
        } else {
            let Some(fld) = field_of(prog, vt, index) else {
                return emit_invalid_zero(prog);
            };
            let fld_ty = fld.typ(&prog.object_arena).expect("field has a type");
            let id = emit_with_pos(
                prog.functions.get_mut(fid),
                block,
                InstrData::Field(Field { x: v, field: index, typ: fld_ty }),
                pos,
            );
            v = Value::Instr(id);
        }
    }
    v
}

/// Placeholder for incomplete type info (non-struct underlying).
fn emit_invalid_zero(prog: &mut Program) -> Value {
    let typ = prog.basic_type(BasicKind::Invalid);
    prog.emit_const(None, typ)
}
