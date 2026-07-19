//! SSA Builder — Function calls.
//!
//! Port of go/ssa's `builder.go` (call part).

use crate::builder::{unparen, Builder};
use crate::methods::recv_type;
use crate::value::Value;
use crate::instr::{Call, CallCommon, InstrData, MakeChan, MakeMap, MakeSlice};
use guff::ast::{CallExpr, Expr, SelectorExpr};
use guff_constant::make_int64;
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::{is_interface, is_pointer, SelectionKind};

impl<'a> Builder<'a> {
    pub(crate) fn set_call(&mut self, e: &CallExpr, c: &mut CallCommon) {
        self.set_call_func(e, c);
        for arg in &e.args {
            c.args.push(self.expr(arg));
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
            let x = self.expr(
                e.args
                    .first()
                    .expect("type conversion CallExpr has one argument"),
            );
            let tv = self.prog.info.types.get(&e.id).expect("no type for conversion");
            let typ = self.typ_type(tv.typ);
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
        };
        self.set_call_func(e, &mut c);

        if self.is_make_builtin(c.value) {
            return self.emit_make(e);
        }

        for arg in &e.args {
            c.args.push(self.expr(arg));
        }

        let tv = self.prog.info.types.get(&e.id).expect("no type for call");
        let typ = self.typ_type(tv.typ);

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
        matches!(
            self.prog.info.types.get(&e.id()).map(|tv| tv.mode),
            Some(guff_types::OperandMode::TypeExpr)
        )
    }

    fn is_make_builtin(&self, fun: Value) -> bool {
        let Value::Builtin(bid) = fun else {
            return false;
        };
        self.prog.builtins.get(bid).name == "make"
    }

    /// Lowers `make(T, …)` to `MakeSlice` / `MakeMap` / `MakeChan`. (Go:
    /// `builder.expr` for `make`.)
    fn emit_make(&mut self, e: &CallExpr) -> Value {
        let tv = self.prog.info.types.get(&e.id).expect("no type for make");
        let typ = self.typ_type(tv.typ);
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
            other => panic!("emit_make on non-slice/map/chan type: {other:?}"),
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
