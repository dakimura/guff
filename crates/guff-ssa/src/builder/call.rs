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
                    let recv_t = recv_type(self.prog, obj);
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

        c.value = self.expr(&e.fun);
    }

    pub(crate) fn emit_call(&mut self, e: &CallExpr) -> Value {
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
