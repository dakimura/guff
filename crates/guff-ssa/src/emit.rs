//! SSA instruction emission helpers.
//!
//! Port of go/ssa's `emit.go`.

use crate::function::Function;
use crate::ids::{BlockId, FuncId, InstrId};
use crate::instr::{
    Alloc, Call, CallCommon, ChangeInterface, ChangeType, Convert, Extract, Field, FieldAddr,
    IndexAddr, InstrData, MakeInterface, Panic, Return, Store, TypeAssert, UnOp,
};
use crate::program::{value_type_of, Program};
use crate::value::Value;
use guff::token::Token;
use guff::{Pos, NO_POS};
use guff_types::arena::TypeData;
use guff_types::{
    empty_tuple, identical, is_non_type_param_interface, is_pointer, new_pointer, new_tuple,
    new_var, pointer_elem, signature_results, struct_field, tuple_at, tuple_len, BasicKind,
    ObjectData, ObjectId, TypeId,
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

/// emit_load emits a load instruction (`*addr`) at source position `pos`.
///
/// go/ssa's `emitLoad` sets no position and its callers assign one afterwards
/// (`address.load` does `load.pos = a.pos`); guff takes the position as an
/// argument instead, so a caller cannot silently leave it unset.
pub fn emit_load_with_pos(
    f: &mut Function,
    block: BlockId,
    addr: Value,
    typ: TypeId,
    pos: Pos,
) -> Value {
    let id = emit_with_pos(
        f,
        block,
        InstrData::UnOp(UnOp {
            op: Token::MUL,
            x: addr,
            comma_ok: false,
            typ,
        }),
        pos,
    );
    Value::Instr(id)
}

/// emit_load emits a load instruction (`*addr`) with no source position — for
/// loads that correspond to no syntax (spilled parameters, lifted locals).
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
    // The cell carries the *variable's* position (Go: `emitLocal(f, v.Type(),
    // v.Pos(), v.Name())`), which for a spilled parameter is that parameter's
    // identifier. `lift` hands it on to the φ it builds, and that is the only
    // position a conditionally reassigned variable ever has.
    let pos = Pos(obj.pos(&prog.object_arena) as i64);
    let v = emit_local(prog, fid, block, typ, pos, comment);
    prog.functions.get_mut(fid).objects.insert(obj, v);
    v
}

/// emit_store emits a store instruction (`*addr = val`) at position `pos`,
/// converting `val` to the pointee type of `addr` first — that conversion is
/// where a concrete value assigned to an interface-typed variable becomes a
/// [`MakeInterface`], and so where the stored value picks up a referrer.
/// (Go: `emitStore`, whose `Val` is `emitConv(f, val, MustDeref(addr.Type()))`.)
pub fn emit_store(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    addr: Value,
    val: Value,
    pos: Pos,
) {
    let addr_ty = value_type_of(prog, prog.functions.get(fid), addr);
    let val = match prog.type_arena.get(addr_ty.underlying(&prog.type_arena)) {
        TypeData::Pointer(_) => {
            let elem = pointer_elem(&prog.type_arena, addr_ty);
            emit_conv(prog, fid, block, val, elem)
        }
        // Hybrid/incomplete type info can leave an lvalue's address typed as
        // something other than a pointer; store as-is rather than panicking.
        _ => val,
    };
    emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::Store(Store { addr, val }),
        pos,
    );
}

/// emit_extract emits an instruction to extract the `index`th component of the
/// tuple value `tuple`, and returns the extracted value. Its type is the
/// `index`th element of `tuple`'s tuple type. (Go: `emitExtract`)
///
/// When `tuple`'s type is not a Tuple (incomplete hybrid info often leaves
/// multi-value expressions as `Typ[Invalid]`), returns an Invalid placeholder
/// instead of panicking — same soft-fail doctrine as [`field_of`].
pub fn emit_extract(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    tuple: Value,
    index: usize,
) -> Value {
    let tuple_ty = value_type_of(prog, prog.functions.get(fid), tuple);
    let Some(typ) = tuple_elem_type(prog, tuple_ty, index) else {
        return emit_invalid_zero(prog);
    };
    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Extract(Extract { tuple, index, typ }),
    );
    Value::Instr(id)
}

/// The type of the `index`th element (a `Var`) of tuple type `tuple`.
/// Returns `None` when `tuple` is not a Tuple (incomplete hybrid info).
fn tuple_elem_type(prog: &Program, tuple: TypeId, index: usize) -> Option<TypeId> {
    if !matches!(prog.type_arena.get(tuple), TypeData::Tuple(_)) {
        return None;
    }
    let var = tuple_at(&prog.type_arena, tuple, index);
    match prog.object_arena.get(var) {
        ObjectData::Var(v) => Some(v.typ()),
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
/// Identical types are a no-op; a value-preserving change of type (see
/// [`is_value_preserving`]) uses [`ChangeType`]; an interface destination uses
/// [`ChangeInterface`] or [`MakeInterface`]; everything else uses [`Convert`].
///
/// DEFERRED vs go/ssa: the slice-to-array / slice-to-array-pointer cases and
/// `MultiConvert` (the type-parameter fan-out), which go/ssa reaches through its
/// `classify` term walk. Every other arm is ported.
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
    if is_value_preserving(prog, ut_src, ut_dst) {
        let id = emit(
            prog.functions.get_mut(fid),
            block,
            InstrData::ChangeType(ChangeType { x: val, typ }),
        );
        return Value::Instr(id);
    }

    // Conversion to, or construction of a value of, an interface type?
    // (Go: the `isNonTypeParamInterface(typ)` arm of `emitConv`.)
    if is_non_type_param_interface(&prog.type_arena, typ) {
        // Interface -> interface is a widening/narrowing of the method set;
        // it always succeeds, so it is a ChangeInterface, not a MakeInterface.
        if is_non_type_param_interface(&prog.type_arena, t_src) {
            let id = emit(
                prog.functions.get_mut(fid),
                block,
                InstrData::ChangeInterface(ChangeInterface { x: val, typ }),
            );
            return Value::Instr(id);
        }

        // Untyped nil: there is nothing to box. Go returns `zeroConst(typ)`,
        // an interface-typed nil constant.
        if matches!(
            prog.type_arena.get(ut_src),
            TypeData::Basic(b) if b.kind() == BasicKind::UntypedNil
        ) {
            return prog.emit_const(None, typ);
        }

        // Other untyped constants box at their default type (`untyped int`
        // boxes as `int`), so recurse once to give the operand a typed type.
        let val = match prog.type_arena.get(ut_src) {
            TypeData::Basic(b) if (b.info().0 & guff_types::IS_UNTYPED.0) != 0 => {
                let dflt = default_basic_type(prog, ut_src);
                emit_conv(prog, fid, block, val, dflt)
            }
            _ => val,
        };

        // The boxed type needs a runtime type descriptor.
        // (Go: `f.Pkg.Prog.needMethodsOf(val.Type())`.)
        let boxed = value_type_of(prog, prog.functions.get(fid), val);
        prog.note_runtime_type(boxed);

        let id = emit(
            prog.functions.get_mut(fid),
            block,
            InstrData::MakeInterface(MakeInterface { x: val, typ }),
        );
        return Value::Instr(id);
    }

    // Conversion of a compile-time constant to a basic type is *folded*: go/ssa
    // returns `NewConst(c.Value, typ)` rather than emitting anything. Emitting a
    // `Convert` here is not merely noisy — SA4015 asks "was this argument
    // converted from an integer?" by looking for an `ir.Convert`, so
    // `math.Ceil(1)` would become a finding upstream never makes.
    // (Go: the `if c, ok := val.(*Const)` block after `classify`.)
    if let Value::Const(cid) = val {
        if matches!(prog.type_arena.get(ut_dst), TypeData::Basic(_)) {
            let cv = prog.constants.get(cid).val.clone();
            return prog.emit_const(cv, typ);
        }
        // A nil constant converts to a nil of the destination type; the
        // slice-to-array cases that could panic are not modelled here.
        if prog.constants.get(cid).val.is_none() {
            return prog.emit_const(None, typ);
        }
    }

    let id = emit(
        prog.functions.get_mut(fid),
        block,
        InstrData::Convert(Convert { x: val, typ }),
    );
    Value::Instr(id)
}

/// Reports whether converting `ut_src` to `ut_dst` changes the type but neither
/// the value nor its representation — i.e. whether a [`ChangeType`] is enough.
/// (Go: `isValuePreserving`.)
///
/// The two special cases beyond "identical underlying types" are the ones that
/// look like real conversions and are not: a channel losing or gaining a
/// direction (`chan T` -> `chan<- T`, which is what `signal.Notify` does to its
/// argument) and a pointer changing base type. Emitting a `Convert` for those
/// hides the operand from every check that follows `ChangeType` back to the
/// value it renamed — SA1017 stops seeing the channel it was handed.
fn is_value_preserving(prog: &mut Program, ut_src: TypeId, ut_dst: TypeId) -> bool {
    if identical(
        &mut prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        ut_dst,
        ut_src,
    ) {
        return true;
    }
    match prog.type_arena.get(ut_dst) {
        TypeData::Chan(_) => matches!(prog.type_arena.get(ut_src), TypeData::Chan(_)),
        TypeData::Pointer(_) => matches!(prog.type_arena.get(ut_src), TypeData::Pointer(_)),
        _ => false,
    }
}

/// The default typed type for an untyped basic type (`untyped int` -> `int`),
/// resolved against the program's predeclared basics. (Go: `types.Default`.)
fn default_basic_type(prog: &Program, ut: TypeId) -> TypeId {
    let kind = match prog.type_arena.get(ut) {
        TypeData::Basic(b) => b.kind(),
        _ => return ut,
    };
    let dflt = match kind {
        BasicKind::UntypedBool => BasicKind::Bool,
        BasicKind::UntypedInt => BasicKind::Int,
        BasicKind::UntypedRune => BasicKind::Int32,
        BasicKind::UntypedFloat => BasicKind::Float64,
        BasicKind::UntypedComplex => BasicKind::Complex128,
        BasicKind::UntypedString => BasicKind::String,
        _ => return ut,
    };
    prog.basic_type(dflt)
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

/// The `*Function` a call statically resolves to, or `None` for a dynamic
/// call, an interface invoke, or a builtin. (Go: `CallCommon.StaticCallee`.)
pub fn static_callee(prog: &Program, f: &Function, common: &CallCommon) -> Option<FuncId> {
    match common.value {
        Value::Function(fid) => Some(fid),
        Value::Instr(iid) => match f.instrs.get(iid) {
            InstrData::MakeClosure(mc) => Some(mc.fn_),
            _ => None,
        },
        _ => None,
    }
}

/// The operand go/ssa panics with behind a no-return call: the string
/// `"noreturn"` boxed into `any`. (Go: `vNoReturn`, converted to `tEface`.)
///
/// Falls back to the bare string constant when the universe's `any` is not in
/// the arena, which only happens for hand-built test programs; nothing reads
/// this operand's type, and leaving the `Panic` out entirely would keep the
/// dead code alive.
fn no_return_operand(prog: &mut Program, fid: FuncId, block: BlockId) -> Value {
    let string_ty = prog.basic_type(guff_types::BasicKind::String);
    let v = prog.emit_const(
        Some(guff_constant::Value::String(std::sync::Arc::new(b"noreturn".to_vec()))),
        string_ty,
    );
    match universe_any(prog) {
        Some(any) => emit_conv(prog, fid, block, v, any),
        None => v,
    }
}

/// The universe's `any` (the predeclared alias for `interface{}`).
fn universe_any(prog: &Program) -> Option<TypeId> {
    for oid in prog.object_arena.ids() {
        let guff_types::ObjectData::TypeName(tn) = prog.object_arena.get(oid) else {
            continue;
        };
        if tn.name() != "any" || oid.pkg(&prog.object_arena).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

/// If `common` statically calls a function the `ctrlflow` analysis proved
/// cannot return, emit the `Panic` that go/ssa puts behind such a call and
/// return the fresh `unreachable.noreturn` block the caller must continue in.
/// Returns `None` when the call can return, leaving the caller's block alone.
///
/// `blockopt::delete_unreachable_blocks` then takes that block — which has no
/// predecessors — and everything it dominates away, which is how the
/// statements after `log.Fatal(…)` stop existing.
///
/// (Go: the second half of `emitCall`.)
pub fn emit_no_return_panic(
    prog: &mut Program,
    fid: FuncId,
    block: BlockId,
    call: InstrId,
    pos: Pos,
) -> Option<BlockId> {
    let f = prog.functions.get(fid);
    let InstrData::Call(Call { call, .. }) = f.instrs.get(call) else {
        return None;
    };
    let callee = static_callee(prog, f, call)?;
    let obj = prog.functions.get(callee).object?;
    if !prog.is_no_return(obj) {
        return None;
    }
    let x = no_return_operand(prog, fid, block);
    emit_with_pos(
        prog.functions.get_mut(fid),
        block,
        InstrData::Panic(Panic { x }),
        pos,
    );
    Some(new_block(
        prog.functions.get_mut(fid),
        fid,
        "unreachable.noreturn",
    ))
}

/// Appends a fresh, unattached basic block to `f`. (Go: `Function.newBasicBlock`.)
fn new_block(f: &mut Function, fid: FuncId, comment: &str) -> BlockId {
    let index = f.blocks.len() as i32;
    let mut b = crate::block::BasicBlock::new(index, fid);
    b.comment = comment.to_string();
    f.blocks.alloc(b)
}

/// emit_call emits a call instruction with the given call components and result
/// type, returning the call's result value. (Go: `emitCall`)
///
/// The no-return handling lives in [`emit_no_return_panic`], which the callers
/// that own a "current block" invoke after this: go/ssa's `emitCall` moves
/// `fn.currentBlock`, and this function has no builder to move.
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
        1 => tuple_elem_type(prog, results.unwrap(), 0)
            .unwrap_or_else(|| prog.basic_type(BasicKind::Invalid)),
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
