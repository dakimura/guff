//! SSA Builder — Function calls.
//!
//! Port of go/ssa's `builder.go` (call part).

use crate::builder::{unparen, Builder};
use crate::methods::recv_type;
use crate::value::Value;
use crate::instr::{Call, CallCommon, InstrData, MakeChan, MakeMap, MakeSlice, Panic};
use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff_constant::make_int64;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::{is_interface, is_pointer, SelectionKind, TypeId};

impl<'a> Builder<'a> {
    pub(crate) fn set_call(&mut self, e: &CallExpr, c: &mut CallCommon) {
        c.ellipsis = e.ellipsis.is_valid();
        self.set_call_func(e, c);
        self.emit_call_args(e, c);
    }

    /// Evaluates the actual parameters of `e` into `c.args`, then converts each
    /// to its formal parameter type. (Go: `emitCallArgs`.)
    ///
    /// The conversion is the point: without it a concrete value passed to an
    /// interface parameter never becomes a `MakeInterface`, so `take(t)` leaves
    /// no record that `T` was boxed — which is what upstream unparam builds its
    /// `typesImplementing` table from.
    ///
    /// **Deliberately not ported: the variadic slice construction.** go/ssa
    /// replaces the variadic tail with a freshly allocated array + `Slice`;
    /// guff passes those arguments through individually and records the spread
    /// with [`CallCommon::ellipsis`](crate::instr::CallCommon::ellipsis), which
    /// every analyzer here already reads. So the tail is converted to the
    /// variadic parameter's *element* type — the type each argument would have
    /// had inside the slice go/ssa builds.
    fn emit_call_args(&mut self, e: &CallExpr, c: &mut CallCommon) {
        // `offset` is 1 when `set_call_func` already pushed a concrete
        // receiver, 0 otherwise. (Go: `offset := len(args)`.)
        let offset = c.args.len();
        for arg in &e.args {
            c.args.push(self.expr(arg));
        }
        self.convert_call_args(e, c, offset);
    }

    /// The callee's signature, as recorded for the `Fun` expression. `None` when
    /// it is not a signature (a builtin with no usable type, or incomplete
    /// hybrid info), in which case the arguments are left unconverted.
    fn call_signature(&mut self, e: &CallExpr, c: &CallCommon) -> Option<TypeId> {
        // Builtins are typed ad hoc here and handled by their own lowering in
        // go/ssa; converting against a synthesized signature would be wrong.
        if matches!(c.value, Value::Builtin(_)) {
            return None;
        }
        let raw = self.prog.info.types.get(&unparen(&e.fun).id())?.typ;
        let sig = self.typ_type(raw);
        let u = sig.underlying(&self.prog.type_arena);
        matches!(self.prog.type_arena.get(u), TypeData::Signature(_)).then_some(u)
    }

    fn convert_call_args(&mut self, e: &CallExpr, c: &mut CallCommon, offset: usize) {
        let Some(sig) = self.call_signature(e, c) else {
            return;
        };
        let arena = &self.prog.type_arena;
        let params = guff_types::signature::signature_params(arena, sig);
        let n_params = guff_types::tuple::tuple_len(arena, params);
        let variadic = guff_types::signature::signature_variadic(arena, sig);
        let n_actual = c.args.len() - offset;

        // Actuals and formals must line up one-for-one before anything is
        // converted. They do not for a chained multi-value call (`f(g())`):
        // go/ssa flattens the tuple with `emitExtract`, guff keeps the tuple as
        // one argument. Converting under that mismatch would coerce the wrong
        // operand, so leave the call alone.
        // DEFERRED: the MRV flattening that would make this case convertible.
        let lines_up = if c.ellipsis {
            n_actual == n_params
        } else if variadic {
            n_actual + 1 >= n_params
        } else {
            n_actual == n_params
        };
        if !lines_up {
            return;
        }

        // `f(a, xs...)`: the slice is passed straight through, so every actual
        // takes its formal's type as-is, variadic parameter included.
        let n_direct = if c.ellipsis || !variadic {
            n_params
        } else {
            n_params - 1
        };

        let block = self.block.expect("no current block");
        let fid = self.func_id;
        for i in 0..n_direct {
            let Some(pt) = self.param_type(params, i) else {
                continue;
            };
            let arg = c.args[offset + i];
            c.args[offset + i] = crate::emit::emit_conv(self.prog, fid, block, arg, pt);
        }
        if c.ellipsis || !variadic {
            return;
        }
        // The variadic tail, converted to the element type.
        let Some(slice_ty) = self.param_type(params, n_params - 1) else {
            return;
        };
        let elem = {
            let arena = &self.prog.type_arena;
            let u = slice_ty.underlying(arena);
            match arena.get(u) {
                TypeData::Slice(_) => guff_types::slice::slice_elem(arena, u),
                _ => return,
            }
        };
        for i in (offset + n_direct)..c.args.len() {
            let arg = c.args[i];
            c.args[i] = crate::emit::emit_conv(self.prog, fid, block, arg, elem);
        }
    }

    /// The declared type of the `i`th parameter in a params tuple.
    fn param_type(&self, params: Option<TypeId>, i: usize) -> Option<TypeId> {
        let params = params?;
        let var = guff_types::tuple::tuple_at(&self.prog.type_arena, params, i);
        match self.prog.object_arena.get(var) {
            guff_types::arena::ObjectData::Var(v) => Some(v.typ()),
            _ => None,
        }
    }

    /// Populates the function parts of `c` from call expression `e`. (Go:
    /// `setCallFunc`.)
    fn set_call_func(&mut self, e: &CallExpr, c: &mut CallCommon) {
        let mut m = unparen(&e.fun);
        m = match m {
            Expr::IndexExpr(ie) => unparen(&ie.x),
            Expr::IndexListExpr(ile) => unparen(&ile.x),
            other => other,
        };

        if let Expr::SelectorExpr(sel) = m {
            if let Some(selection) = self.selection(sel) {
                if selection.kind() == SelectionKind::MethodVal {
                    let obj = selection.obj();
                    // Only treat as a method call when the object's type is a
                    // signature *with* a receiver. Mis-tagged selections (or
                    // non-Func objects) used to panic in `recv_type`.
                    let is_method = obj
                        .typ(&self.prog.object_arena)
                        .and_then(|sig| {
                            guff_types::signature::signature_recv(&self.prog.type_arena, sig)
                        })
                        .is_some();
                    if is_method {
                        let Some(recv_t) = recv_type(self.prog, obj) else {
                            // Defensive: signature_recv said method but type missing.
                            c.value = self.expr(&e.fun);
                            return;
                        };
                        let want_addr = is_pointer(&self.prog.type_arena, recv_t);
                        let v = self.receiver(&sel.x, want_addr, true, &selection);
                        if is_interface(&self.prog.type_arena, recv_t) {
                            c.value = v;
                            c.method = Some(obj);
                        } else {
                            c.value = Value::Function(self.prog.object_method(obj, &[]));
                            c.args.push(v);
                        }
                        return;
                    }
                }
            }
        }

        c.value = self.expr(&e.fun);
    }

    pub(crate) fn emit_call(&mut self, e: &CallExpr) -> Value {
        // Explicit type conversion, e.g. `string(x)` or `int64(n)`.
        // (Go: `fn.info.Types[e.Fun].IsType()` branch in `builder.expr0`.)
        if self.is_type_expr(&e.fun) {
            let Some(arg) = e.args.first() else {
                return self.invalid_zero();
            };
            let x = self.expr(arg);
            // Prefer the CallExpr's recorded type; fall back to the Fun type
            // expression when hybrid source-checking left the call untyped.
            let type_id = self
                .prog
                .info
                .types
                .get(&e.id)
                .or_else(|| self.prog.info.types.get(&unparen(&e.fun).id()))
                .map(|tv| tv.typ);
            let Some(type_id) = type_id else {
                // Incomplete checker info must not abort the SSA builder (and,
                // near the stack limit, a panic+backtrace can escalate to a
                // fatal stack overflow). Pass the argument through unconverted.
                return x;
            };
            let typ = self.typ_type(type_id);
            let block = self.block.expect("no current block");
            let fid = self.func_id;
            let y = crate::emit::emit_conv(self.prog, fid, block, x, typ);
            // Stamp the conversion instruction with the '(' position when we
            // actually emitted one (Go updates Convert/ChangeType.pos).
            if let Value::Instr(iid) = y {
                self.prog.functions.get_mut(fid).set_pos(iid, e.lparen);
            }
            return y;
        }

        let mut c = CallCommon {
            value: Value::Builtin(unsafe { std::mem::transmute(1u32) }),
            method: None,
            args: Vec::new(),
            ellipsis: e.ellipsis.is_valid(),
        };
        self.set_call_func(e, &mut c);

        if self.is_make_builtin(c.value) {
            return self.emit_make(e);
        }

        if self.is_builtin_named(c.value, "panic") {
            return self.emit_panic(e);
        }

        self.emit_call_args(e, &mut c);

        let typ = match self.prog.info.types.get(&e.id) {
            Some(tv) => self.typ_type(tv.typ),
            None => self.prog.basic_type(BasicKind::Invalid),
        };

        let block = self.block.expect("no current block");
        let id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Call(Call { call: c, typ }),
            e.lparen,
        );
        Value::Instr(id)
    }

    /// Reports whether `e` denotes a type (used to distinguish `T(x)` conversions
    /// from ordinary calls). (Go: `TypeAndValue.IsType()`.)
    fn is_type_expr(&self, e: &Expr) -> bool {
        let e = unparen(e);
        if matches!(
            self.prog.info.types.get(&e.id()).map(|tv| tv.mode),
            Some(guff_types::OperandMode::TypeExpr)
        ) {
            return true;
        }
        // Syntactic fallback: hybrid source-checking of dependencies can omit
        // `TypeExpr` mode on nodes that are unambiguously types (notably
        // `[]byte` as `ArrayType` with nil Len). Without this, `T(x)` falls
        // through to value `expr()` and panics.
        match e {
            Expr::ArrayType(_)
            | Expr::StructType(_)
            | Expr::FuncType(_)
            | Expr::InterfaceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_) => true,
            Expr::StarExpr(s) => self.is_type_expr(&s.x),
            _ => false,
        }
    }

    fn is_builtin_named(&self, fun: Value, name: &str) -> bool {
        let Value::Builtin(bid) = fun else {
            return false;
        };
        self.prog.builtins.get(bid).name == name
    }

    fn is_make_builtin(&self, fun: Value) -> bool {
        self.is_builtin_named(fun, "make")
    }

    /// Lowers `panic(x)` to the `Panic` block terminator followed by an
    /// unreachable block, as go/ssa's `builder.builtin` does.
    ///
    /// Emitting it as an ordinary call instead left a fallthrough edge out of
    /// the panicking block, so `if p == nil { panic(…) }; p.F` had a join with
    /// two predecessors and the non-nil successor no longer dominated the
    /// deref — SA5011 then reported the guarded use (consul
    /// `internal/resource/sort.go`).
    fn emit_panic(&mut self, e: &CallExpr) -> Value {
        let x = match e.args.first() {
            Some(arg) => self.expr(arg),
            None => self.invalid_zero(),
        };
        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Panic(Panic { x }),
            e.lparen,
        );
        let unreachable = self.new_basic_block("unreachable".to_string());
        self.set_block(Some(unreachable));
        // go/ssa returns vFalse here: any non-nil value will do, and a const
        // keeps `expr` from recording a DebugRef for the discarded result.
        let bool_ty = self.prog.basic_type(BasicKind::Bool);
        self.prog
            .emit_const(Some(guff_constant::Value::Bool(false)), bool_ty)
    }

    /// Lowers `make(T, …)` to `MakeSlice` / `MakeMap` / `MakeChan`. (Go:
    /// `builder.expr` for `make`.)
    fn emit_make(&mut self, e: &CallExpr) -> Value {
        let typ = self.type_of(e.id);
        let u = typ.underlying(&self.prog.type_arena);
        let block = self.block.expect("no current block");

        let data = match self.prog.type_arena.get(u) {
            TypeData::Chan(_) => {
                let size = if e.args.len() >= 2 {
                    Some(self.expr(&e.args[1]))
                } else {
                    Some(self.int_zero())
                };
                InstrData::MakeChan(MakeChan { size, typ })
            }
            TypeData::Map(_) => {
                let reserve = e.args.get(1).map(|a| self.expr(a));
                InstrData::MakeMap(MakeMap { reserve, typ })
            }
            TypeData::Slice(_) => {
                let len = e.args.get(1).map(|a| self.expr(a));
                let cap = e
                    .args
                    .get(2)
                    .map(|a| self.expr(a))
                    .or(len);
                InstrData::MakeSlice(MakeSlice { len, cap, typ })
            }
            _ => return self.invalid_zero(),
        };

        let id = crate::emit::emit_with_pos(self.func_mut(), block, data, e.lparen);
        Value::Instr(id)
    }

    fn int_zero(&mut self) -> Value {
        let int_ty = self.prog.basic_type(BasicKind::Int);
        self.prog
            .emit_const(Some(make_int64(0)), int_ty)
    }

    pub(crate) fn selection(&self, e: &SelectorExpr) -> Option<guff_types::Selection> {
        self.prog.info.selections.get(&e.id).cloned()
    }
}
