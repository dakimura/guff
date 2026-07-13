//! Synthesis of delegating wrapper functions.
//!
//! Port of go/ssa's `wrappers.go`. Implemented so far:
//! - [`build_instantiation_wrapper`] — generic instantiation wrappers (E12);
//! - [`create_wrapper`] / [`build_wrapper`] — promotion/indirection wrappers and
//!   method-expression thunks;
//! - [`create_bound`] / [`build_bound`] — bound-method closure targets.

use guff_types::{
    instantiate, is_interface, is_pointer, selection_type, signature_params, signature_recv,
    signature_results, tuple_at, tuple_len, ObjectData, ObjectId, Selection, SelectionKind, TypeId,
};

use crate::builder::Builder;
use crate::create::{create_function, create_params, create_wrapper_params};
use crate::emit::{
    emit, emit_call, emit_extract, emit_implicit_selections, emit_load, emit_tail_call,
    emit_type_coercion,
};
use crate::function::{BuildStrategy, FreeVar};
use crate::ids::{BlockId, FuncId, FreeVarId};
use crate::instantiate::targstr;
use crate::instr::{CallCommon, InstrData, Return};
use crate::methods::recv_type;
use crate::program::Program;
use crate::value::Value;
use guff::{Pos, NO_POS};

/// Local copy of a type-checker [`Selection`] with the synthesized method
/// signature type attached. (Go: `selection` in `wrappers.go`.)
#[derive(Clone, Debug)]
pub struct WrapperSelection {
    pub kind: SelectionKind,
    pub recv: TypeId,
    pub typ: TypeId,
    pub obj: ObjectId,
    pub index: Vec<i32>,
    pub indirect: bool,
}

impl WrapperSelection {
    pub fn from_selection(prog: &mut Program, sel: &Selection) -> Self {
        let typ = selection_type(&mut prog.type_arena, &mut prog.object_arena, sel);
        Self {
            kind: sel.kind(),
            recv: sel.recv(),
            typ,
            obj: sel.obj(),
            index: sel.index().to_vec(),
            indirect: sel.indirect(),
        }
    }
}

/// Creates a synthetic wrapper (or thunk) for selection `sel`. (Go:
/// `createWrapper`.)
pub fn create_wrapper(prog: &mut Program, sel: &WrapperSelection, targs: &[TypeId]) -> FuncId {
    let obj_name = sel.obj.name(&prog.object_arena).to_string();
    let (mut name, sig) = maybe_instance(prog, &obj_name, sel.typ, targs);

    let (description, param_start_hint) = if sel.kind == SelectionKind::MethodExpr {
        name.push_str("$thunk");
        (format!("thunk for {}", obj_name), 1usize)
    } else {
        (format!("wrapper for {}", obj_name), 0usize)
    };

    let mut f = crate::function::Function::new(name, None, None);
    f.method = Some(sel.clone());
    f.object = Some(sel.obj);
    f.signature = Some(sig);
    f.synthetic = Some(description);
    f.build_strategy = BuildStrategy::Wrapper;
    f.type_args = targs.to_vec();
    let _ = param_start_hint; // used by build_wrapper via method.kind
    prog.functions.alloc(f)
}

/// Creates a bound-method wrapper for concrete/interface method `obj`. (Go:
/// `createBound`.)
pub fn create_bound(prog: &mut Program, obj: ObjectId, targs: &[TypeId]) -> FuncId {
    let obj_name = obj.name(&prog.object_arena).to_string();
    let orig_sig = obj
        .typ(&prog.object_arena)
        .expect("bound method must have signature");
    let (name, inst_sig) = maybe_instance(prog, &obj_name, orig_sig, targs);
    let sig = change_recv(prog, inst_sig, None);

    let recv_typ = recv_type(prog, obj);
    let fid = create_function(prog, format!("{}$bound", name), None, None);
    {
        let f = prog.functions.get_mut(fid);
        f.object = Some(obj);
        f.signature = Some(sig);
        f.synthetic = Some(format!("bound method wrapper for {}", obj_name));
        f.build_strategy = BuildStrategy::Bound;
        f.type_args = targs.to_vec();
        f.freevars.alloc(FreeVar {
            name: "recv".to_string(),
            typ: recv_typ,
            parent: fid,
            outer: Value::Builtin(unsafe { std::mem::transmute(1u32) }),
        });
    }
    fid
}

/// Creates a method-expression thunk. (Go: `createThunk`.)
pub fn create_thunk(prog: &mut Program, sel: &WrapperSelection, targs: &[TypeId]) -> FuncId {
    assert_eq!(sel.kind, SelectionKind::MethodExpr);
    let fid = create_wrapper(prog, sel, targs);
    assert!(
        signature_recv(&prog.type_arena, prog.functions.get(fid).signature.unwrap()).is_none(),
        "thunk must not have a receiver in its signature"
    );
    fid
}

/// Builds the body of wrapper/thunk `fid`. (Go: `(*builder).buildWrapper`.)
pub fn build_wrapper(prog: &mut Program, fid: FuncId) {
    let method = prog.functions.get(fid).method.clone().expect("wrapper has method");
    let obj = prog.functions.get(fid).object.expect("wrapper has object");
    let type_args = prog.functions.get(fid).type_args.clone();
    let sig = prog.functions.get(fid).signature.expect("wrapper has signature");

    let (recv_obj, param_start) = if method.kind == SelectionKind::MethodExpr {
        let params = signature_params(&prog.type_arena, sig).expect("thunk has params");
        (tuple_at(&prog.type_arena, params, 0), 1)
    } else {
        (
            signature_recv(&prog.type_arena, sig).expect("wrapper has receiver"),
            0,
        )
    };

    let entry = {
        let mut b = Builder::new(prog, fid);
        let e = b.new_basic_block("entry".to_string());
        b.set_block(Some(e));
        e
    };
    create_wrapper_params(prog, fid, entry, recv_obj, param_start);

    let indices: Vec<usize> = method.index.iter().map(|&i| i as usize).collect();
    let last = indices.len().saturating_sub(1);

    let mut v = *prog
        .functions
        .get(fid)
        .objects
        .get(&recv_obj)
        .expect("spilled receiver in objects");

    if is_pointer(&prog.type_arena, method.recv) {
        let pointee = guff_types::pointer_elem(&prog.type_arena, method.recv);
        v = emit_load(prog.functions.get_mut(fid), entry, v, pointee);
    }

    if last > 0 {
        v = emit_implicit_selections(prog, fid, entry, v, &indices[..last], NO_POS);
    }

    let rt = recv_type(prog, obj);
    let mut call = CallCommon {
        value: Value::Builtin(unsafe { std::mem::transmute(1u32) }),
        method: None,
        args: Vec::new(),
    };

    if !is_interface(&prog.type_arena, rt) {
        if !is_pointer(&prog.type_arena, rt) {
            let vt = crate::program::value_type_of(prog, prog.functions.get(fid), v);
            v = emit_load(prog.functions.get_mut(fid), entry, v, vt);
        }
        call.value = Value::Function(prog.object_method(obj, &type_args));
        call.args.push(v);
    } else {
        call.method = Some(obj);
        let vt = crate::program::value_type_of(prog, prog.functions.get(fid), v);
        call.value = emit_load(prog.functions.get_mut(fid), entry, v, vt);
    }

    for (pid, _) in prog.functions.get(fid).params.iter().skip(param_start) {
        call.args.push(Value::Param(pid));
    }

    emit_tail_call(prog, fid, entry, call);
    prog.finish_function(fid);
}

/// Builds the body of bound-method wrapper `fid`. (Go: `(*builder).buildBound`.)
pub fn build_bound(prog: &mut Program, fid: FuncId) {
    let obj = prog.functions.get(fid).object.expect("bound has object");
    let type_args = prog.functions.get(fid).type_args.clone();

    create_params(prog, fid);
    let entry = {
        let mut b = Builder::new(prog, fid);
        let e = b.new_basic_block("entry".to_string());
        b.set_block(Some(e));
        e
    };

    let recv_fv: FreeVarId = prog.functions.get(fid).freevars.iter().next().unwrap().0;
    let recv = Value::FreeVar(recv_fv);
    let rt = recv_type(prog, obj);

    let mut call = CallCommon {
        value: Value::Builtin(unsafe { std::mem::transmute(1u32) }),
        method: None,
        args: Vec::new(),
    };

    if !is_interface(&prog.type_arena, rt) {
        call.value = Value::Function(prog.object_method(obj, &type_args));
        call.args.push(recv);
    } else {
        call.method = Some(obj);
        call.value = recv;
    }

    for (pid, _) in prog.functions.get(fid).params.iter() {
        call.args.push(Value::Param(pid));
    }

    emit_tail_call(prog, fid, entry, call);
    prog.finish_function(fid);
}

fn maybe_instance(prog: &mut Program, name: &str, sig: TypeId, targs: &[TypeId]) -> (String, TypeId) {
    if targs.is_empty() {
        return (name.to_string(), sig);
    }
    let new_name = format!(
        "{}{}",
        name,
        targstr(
            &prog.type_arena,
            &prog.object_arena,
            &prog.package_arena,
            targs,
        )
    );
    let inst = instantiate(
        &mut prog.type_arena,
        &mut prog.object_arena,
        &mut prog.ctxt,
        sig,
        targs.to_vec(),
    );
    let sig = prog.canon.canonical_type(
        &mut prog.type_arena,
        &prog.object_arena,
        &prog.package_arena,
        inst,
    );
    (new_name, sig)
}

fn change_recv(prog: &mut Program, sig: TypeId, recv: Option<ObjectId>) -> TypeId {
    let (params, results, variadic) = match prog.type_arena.get(sig) {
        guff_types::TypeData::Signature(s) => (s.params(), s.results(), s.variadic()),
        _ => panic!("change_recv: not a signature"),
    };
    guff_types::signature::new_signature_type(
        &mut prog.type_arena,
        recv,
        &[],
        &[],
        params,
        results,
        variadic,
    )
}

/// Builds the body of an instantiation wrapper `fid`. The body calls the
/// origin generic function, converting between the instance's argument/result
/// types and the origin's parameter/result types via `ChangeType`.
/// (Go: `(*builder).buildInstantiationWrapper`.)
///
/// # Panics
/// - if `fid` has no `top_level_origin` (not an instantiation wrapper);
/// - if the instance has a receiver (method case, deferred);
/// - if the origin has no results (0-result case, deferred).
pub fn build_instantiation_wrapper(prog: &mut Program, fid: FuncId) {
    let orig = prog
        .functions
        .get(fid)
        .top_level_origin
        .expect("instantiation wrapper has an origin");
    let sig = prog.functions.get(fid).signature.expect("wrapper has a signature");
    let orig_sig = prog.functions.get(orig).signature.expect("origin has a signature");

    assert!(
        signature_recv(&prog.type_arena, sig).is_none(),
        "method instantiation wrapper (receiver) is deferred"
    );

    // startBody: create the instance's parameters from its signature, open the
    // entry block.
    create_params(prog, fid);
    let entry = {
        let mut b = Builder::new(prog, fid);
        let e = b.new_basic_block("entry".to_string());
        b.set_block(Some(e));
        e
    };

    // Call type: origin results len==1 → the single result type; else the whole
    // results tuple. (0 results is deferred; see module docs.)
    let orig_results = signature_results(&prog.type_arena, orig_sig);
    let call_typ = match tuple_len(&prog.type_arena, orig_results) {
        0 => guff_types::tuple::empty_tuple(&mut prog.type_arena),
        1 => tuple_elem_type(prog, orig_results.unwrap(), 0),
        _ => orig_results.unwrap(),
    };

    // Each instance parameter becomes an argument to the origin call, coerced to
    // the origin's (type-parameter) parameter type.
    let orig_params = signature_params(&prog.type_arena, orig_sig);
    let param_ids: Vec<_> = prog.functions.get(fid).params.iter().map(|(id, _)| id).collect();
    let mut args: Vec<Value> = Vec::with_capacity(param_ids.len());
    for (i, pid) in param_ids.into_iter().enumerate() {
        let target = tuple_elem_type(prog, orig_params.expect("origin has params"), i);
        let coerced = emit_type_coercion(prog, fid, entry, Value::Param(pid), target);
        args.push(coerced);
    }

    let results = emit_call(
        prog,
        fid,
        entry,
        CallCommon { value: Value::Function(orig), method: None, args },
        call_typ,
    );

    // Build the return, coercing each result back to the instance's result type.
    let inst_results = signature_results(&prog.type_arena, sig);
    let nr = tuple_len(&prog.type_arena, inst_results);
    let mut ret_results: Vec<Value> = Vec::new();
    match nr {
        0 => {}
        1 => {
            let target = tuple_elem_type(prog, inst_results.unwrap(), 0);
            ret_results.push(emit_type_coercion(prog, fid, entry, results, target));
        }
        _ => {
            for i in 0..nr {
                let v = emit_extract(prog, fid, entry, results, i);
                let target = tuple_elem_type(prog, inst_results.unwrap(), i);
                ret_results.push(emit_type_coercion(prog, fid, entry, v, target));
            }
        }
    }
    emit(
        prog.functions.get_mut(fid),
        entry,
        InstrData::Return(Return { results: ret_results }),
    );

    prog.finish_function(fid);
}

/// The type of the `i`th element (a `Var`) of tuple type `tuple`.
fn tuple_elem_type(prog: &Program, tuple: TypeId, i: usize) -> TypeId {
    let var = tuple_at(&prog.type_arena, tuple, i);
    match prog.object_arena.get(var) {
        ObjectData::Var(v) => v.typ(),
        _ => panic!("tuple element is not a Var"),
    }
}
