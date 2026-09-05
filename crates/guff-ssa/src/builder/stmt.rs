//! SSA Builder — Statements.
//!
//! Port of go/ssa's `builder.go` (stmt part).

use crate::builder::Builder;
use crate::value::Value;
use guff::ast::{
    Stmt, AssignStmt, DeclStmt, ReturnStmt, Decl, ValueSpec, IfStmt, ForStmt, DeferStmt, GoStmt,
    Expr, Ident, RangeStmt, BranchStmt, LabeledStmt, SwitchStmt, TypeSwitchStmt, SelectStmt,
    SendStmt, IncDecStmt, CaseClause,
};
use guff::token::Token;
use guff_types::{
    chan_elem, map_elem, map_key, new_tuple, object::var::new_var, BasicKind, ChanDir, TypeData,
};
use crate::instr::{
    BinOp, Call, CallCommon, Index, InstrData, Next, Panic, Range, Select, SelectState, Send, UnOp,
};
use guff_types::object::builtin::BuiltinId;
use guff_types::{array_elem, array_len, basic::IS_INTEGER, basic::IS_STRING, basic::IS_UNSIGNED, pointer_elem, slice_elem, TypeId};

impl<'a> Builder<'a> {
    /// stmt translates a statement to SSA. (Go: `builder.stmt`)
    pub fn stmt(&mut self, s: &Stmt) {
        self.stmt_with_label(s, None);
    }

    /// Like [`stmt`](Self::stmt) but threads the optional label of an enclosing
    /// [`LabeledStmt`] to breakable statements (`for`, `range`, …).
    pub(crate) fn stmt_with_label(&mut self, s: &Stmt, label: Option<&str>) {
        match s {
            Stmt::AssignStmt(a) => self.assign_stmt(a),
            Stmt::DeclStmt(d) => self.decl_stmt(d),
            Stmt::ExprStmt(e) => {
                self.expr(&e.x);
            }
            Stmt::IfStmt(i) => self.if_stmt(i),
            Stmt::ForStmt(f) => self.for_stmt(f, label),
            Stmt::DeferStmt(d) => self.defer_stmt(d),
            Stmt::GoStmt(g) => self.go_stmt(g),
            Stmt::ReturnStmt(r) => self.return_stmt(r),
            Stmt::RangeStmt(r) => self.range_stmt(r, label),
            Stmt::BranchStmt(b) => self.branch_stmt(b),
            Stmt::LabeledStmt(l) => self.labeled_stmt(l, label),
            Stmt::BlockStmt(b) => {
                for inner in &b.list {
                    self.stmt_with_label(inner, label);
                }
            }
            Stmt::EmptyStmt(_) => {}
            Stmt::SendStmt(s) => self.send_stmt(s),
            Stmt::IncDecStmt(s) => self.inc_dec_stmt(s),
            Stmt::SwitchStmt(s) => self.switch_stmt(s, label),
            Stmt::TypeSwitchStmt(s) => self.type_switch_stmt(s, label),
            Stmt::SelectStmt(s) => self.select_stmt(s, label),
            // CaseClause / CommClause only appear inside switch/select bodies.
            Stmt::CaseClause(_) | Stmt::CommClause(_) | Stmt::BadStmt(_) => {
                panic!("unexpected stmt at top level: {:?}", s);
            }
        }
    }

    fn labeled_stmt(&mut self, l: &LabeledStmt, outer_label: Option<&str>) {
        if l.label.name == "_" {
            self.stmt_with_label(&l.stmt, outer_label);
            return;
        }
        let name = l.label.name.clone();
        let goto_ = self.lblock_of(&name);
        self.prog
            .functions
            .get_mut(self.func_id)
            .lblocks
            .get_mut(&name)
            .expect("lblock exists")
            .resolved = true;
        self.emit_jump(goto_);
        self.set_block(Some(goto_));
        self.stmt_with_label(&l.stmt, Some(&name));
        let _ = goto_; // label entry block is wired via lblock_of
    }

    fn branch_stmt(&mut self, s: &BranchStmt) {
        let (target, label_exit) = if let Some(lbl) = &s.label {
            let name = lbl.name.clone();
            if let Some(tb) = self.labelled_block(&name, s.tok) {
                (tb, None)
            } else {
                let lb = self.lblock_of(&name);
                let exit = if self.func().jump_var.is_some() && s.tok == Token::GOTO {
                    Some(self.label_exit(&name, s.tok_pos))
                } else {
                    None
                };
                (
                    crate::builder::TargetBlock {
                        func: self.func_id,
                        block: lb,
                    },
                    exit,
                )
            }
        } else {
            let tb = self
                .targeted_block(s.tok)
                .unwrap_or_else(|| panic!("{:?} not in loop/switch", s.tok));
            (tb, None)
        };
        let _ = label_exit;

        if target.func == self.func_id {
            self.emit_jump(target.block);
        } else {
            let e = self.block_exit(target.func, target.block, s.tok_pos);
            let jump = self.func().jump_var.expect("yield function has jump_var");
            let exit_id = self.int_const(e.id);
            self.store_jump_var(jump, exit_id, s.tok_pos);
            let bool_ty = self.prog.basic_type(BasicKind::Bool);
            let v_false = self
                .prog
                .emit_const(Some(guff_constant::Value::Bool(false)), bool_ty);
            let block = self.block.expect("no current block");
            crate::emit::emit_with_pos(
                self.func_mut(),
                block,
                crate::instr::InstrData::Return(crate::instr::Return {
                    results: vec![v_false],
                }),
                s.tok_pos,
            );
        }
        let unreachable = self.new_basic_block("unreachable".to_string());
        self.set_block(Some(unreachable));
    }

    /// range_int emits the header for a range loop with an integer operand.
    /// (Go: `builder.rangeInt`)
    fn range_int(
        &mut self,
        s: &RangeStmt,
        mut x: Value,
        x_ty: TypeId,
        label: Option<&str>,
    ) {
        let pos = s.for_;
        let want_key = s
            .key
            .as_ref()
            .is_some_and(|k| !is_blank_ident(k));

        if guff_types::is_untyped(&self.prog.type_arena, x_ty) {
            let int_ty = self.prog.basic_type(BasicKind::Int);
            let block = self.block.expect("no current block");
            x = crate::emit::emit_type_coercion(self.prog, self.func_id, block, x, int_ty);
        }
        let t = crate::program::value_type_of(self.prog, self.func(), x);

        let block = self.block.expect("no current block");
        let iter_addr = crate::emit::emit_local(
            self.prog,
            self.func_id,
            block,
            t,
            pos,
            "rangeint.iter".to_string(),
        );
        let zero = {
            let z = self.int_const(0);
            let block = self.block.expect("no current block");
            crate::emit::emit_type_coercion(self.prog, self.func_id, block, z, t)
        };
        let body = self.new_basic_block("rangeint.body".to_string());
        let done = self.new_basic_block("rangeint.done".to_string());
        let cond = self.emit_compare(Token::LSS, zero, x, pos);
        self.emit_if(cond, body, done);

        let loop_ = self.new_basic_block("rangeint.loop".to_string());
        self.set_block(Some(loop_));

        let iter_val = self.emit_load(iter_addr, t);
        let one = self.int_const(1);
        let one_t = {
            let block = self.block.expect("no current block");
            crate::emit::emit_type_coercion(self.prog, self.func_id, block, one, t)
        };
        let incr = self.emit_binop(Token::ADD, iter_val, one_t, t, pos);
        self.emit_store(iter_addr, incr, pos);
        let loop_cond = self.emit_compare(Token::LSS, incr, x, pos);
        self.emit_if(loop_cond, body, done);

        self.set_block(Some(body));

        if s.tok == Some(Token::DEFINE) {
            self.range_create_vars(s, want_key, false);
        }

        if want_key {
            let k = self.emit_load(iter_addr, t);
            if let Some(key) = &s.key {
                self.address(key, false).store(self, k);
            }
        }

        // go/ssa sets the labelled statement's break/continue targets *before*
        // building the body (`label._break = done`). Setting them after left
        // every `break <label>` inside the body unresolved, and `branch_stmt`
        // falls back to the label's goto block on a miss — so a labelled break
        // jumped to the top of the loop it was trying to leave. Every CFG
        // consumer saw the wrong edge; wastedassign saw the loop's own next
        // store and called the assignment before the break wasted (gitea
        // `services/gitdiff`).
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }
        self.push_targets(done, loop_);
        for stmt in &s.body.list {
            self.stmt_with_label(stmt, None);
        }
        self.pop_targets();

        if self.block.is_some() {
            self.emit_jump(loop_);
        }
        self.set_block(Some(done));
    }

    /// range_stmt translates a `for range` loop. (Go: `builder.rangeStmt`)
    fn range_stmt(&mut self, s: &RangeStmt, label: Option<&str>) {
        let x = self.expr(&s.x);
        let x_ty = crate::program::value_type_of(self.prog, self.func(), x);
        let u = x_ty.underlying(&self.prog.type_arena);
        match self.prog.type_arena.get(u) {
            TypeData::Chan(_) => self.range_chan(s, x, u, label),
            TypeData::Slice(_) | TypeData::Array(_) => self.range_indexed(s, x, x_ty, label),
            TypeData::Pointer(_) => {
                let elem = pointer_elem(&self.prog.type_arena, u);
                if matches!(
                    self.prog.type_arena.get(elem.underlying(&self.prog.type_arena)),
                    TypeData::Array(_)
                ) {
                    self.range_indexed(s, x, x_ty, label);
                } else {
                    // Incomplete hybrid info left a non-array pointer — soft-skip.
                    self.range_soft_skip(s, label);
                }
            }
            TypeData::Map(_) => self.range_iter(s, x, u, false, label),
            TypeData::Basic(b) => {
                let info = b.info();
                if info.contains(IS_STRING) {
                    self.range_iter(s, x, u, true, label);
                } else if info.contains(IS_INTEGER) || info.contains(IS_UNSIGNED) {
                    self.range_int(s, x, x_ty, label);
                } else {
                    // Typ[Invalid] / other non-rangeable basic under incomplete
                    // hybrid info — skip rather than abort the SSA build.
                    let _ = x;
                    self.range_soft_skip(s, label);
                }
            }
            TypeData::Signature(_) => self.range_func(s, x, label),
            // Incomplete Named / TypeParam / Interface / … — soft-skip.
            _ => {
                let _ = x;
                self.range_soft_skip(s, label);
            }
        }
    }

    /// Soft-skip a range whose ranged type is incomplete or non-rangeable.
    /// Still creates `:=` locals so later uses resolve, then continues in the
    /// current block as if the loop never ran.
    fn range_soft_skip(&mut self, s: &RangeStmt, label: Option<&str>) {
        if s.tok == Some(Token::DEFINE) {
            let want_key = s
                .key
                .as_ref()
                .is_some_and(|k| !is_blank_ident(k));
            let want_value = s
                .value
                .as_ref()
                .is_some_and(|v| !is_blank_ident(v));
            self.range_create_vars(s, want_key, want_value);
        }
        if let Some(name) = label {
            // No loop blocks — label targets stay unset; break/continue of this
            // label would already be ill-typed under incomplete info.
            let _ = name;
        }
    }

    /// range_indexed emits the header for a loop over an array, slice, or
    /// `*array`. (Go: `builder.rangeIndexed`)
    fn range_indexed(&mut self, s: &RangeStmt, x: Value, x_ty: TypeId, label: Option<&str>) {
        let pos = s.for_;
        let int_ty = self.prog.basic_type(BasicKind::Int);
        let want_key = s
            .key
            .as_ref()
            .is_some_and(|k| !is_blank_ident(k));
        let want_value = s
            .value
            .as_ref()
            .is_some_and(|v| !is_blank_ident(v));

        let core = x_ty.underlying(&self.prog.type_arena);
        let length = match self.prog.type_arena.get(core) {
            TypeData::Array(_) => self.int_const(array_len(&self.prog.type_arena, core)),
            _ => self.emit_len(x, pos),
        };

        let block = self.block.expect("no current block");
        let index_addr = crate::emit::emit_local(
            self.prog,
            self.func_id,
            block,
            int_ty,
            pos,
            "rangeindex".to_string(),
        );
        let minus_one = self.int_const(-1);
        self.emit_store(index_addr, minus_one, pos);

        let loop_ = self.new_basic_block("rangeindex.loop".to_string());
        self.emit_jump(loop_);
        self.set_block(Some(loop_));

        let index_val = self.emit_load(index_addr, int_ty);
        let one = self.int_const(1);
        let incr = self.emit_binop(Token::ADD, index_val, one, int_ty, pos);
        self.emit_store(index_addr, incr, pos);

        let body = self.new_basic_block("rangeindex.body".to_string());
        let done = self.new_basic_block("rangeindex.done".to_string());
        let cond = self.emit_compare(Token::LSS, incr, length, pos);
        self.emit_if(cond, body, done);

        self.set_block(Some(body));

        if s.tok == Some(Token::DEFINE) {
            self.range_create_vars(s, want_key, want_value);
        }

        if want_key {
            let k = self.emit_load(index_addr, int_ty);
            if let Some(key) = &s.key {
                self.address(key, false).store(self, k);
            }
        }
        if want_value {
            let v = self.range_indexed_value(x, x_ty, incr);
            if let Some(value) = &s.value {
                self.address(value, false).store(self, v);
            }
        }

        // go/ssa sets the labelled statement's break/continue targets *before*
        // building the body (`label._break = done`). Setting them after left
        // every `break <label>` inside the body unresolved, and `branch_stmt`
        // falls back to the label's goto block on a miss — so a labelled break
        // jumped to the top of the loop it was trying to leave. Every CFG
        // consumer saw the wrong edge; wastedassign saw the loop's own next
        // store and called the assignment before the break wasted (gitea
        // `services/gitdiff`).
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }
        self.push_targets(done, loop_);
        for stmt in &s.body.list {
            self.stmt_with_label(stmt, None);
        }
        self.pop_targets();

        if self.block.is_some() {
            self.emit_jump(loop_);
        }
        self.set_block(Some(done));
    }

    /// range_iter emits the header for a loop over a map or string.
    /// (Go: `builder.rangeIter`)
    fn range_iter(
        &mut self,
        s: &RangeStmt,
        x: Value,
        core: TypeId,
        is_string: bool,
        label: Option<&str>,
    ) {
        let pos = s.for_;
        let bool_ty = self.prog.basic_type(BasicKind::Bool);
        let invalid_ty = self.prog.basic_type(BasicKind::Invalid);
        let want_key = s
            .key
            .as_ref()
            .is_some_and(|k| !is_blank_ident(k));
        let want_value = s
            .value
            .as_ref()
            .is_some_and(|v| !is_blank_ident(v));

        let iter_ty = invalid_ty;
        let block = self.block.expect("no current block");
        let rng_id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Range(Range { x, typ: iter_ty }),
            pos,
        );
        let it = Value::Instr(rng_id);

        let loop_ = self.new_basic_block("rangeiter.loop".to_string());
        self.emit_jump(loop_);
        self.set_block(Some(loop_));

        let (key_ty, val_ty) = if is_string {
            (
                if want_key {
                    self.prog.basic_type(BasicKind::Int)
                } else {
                    invalid_ty
                },
                if want_value {
                    self.prog.basic_type(BasicKind::Int32)
                } else {
                    invalid_ty
                },
            )
        } else {
            (
                if want_key {
                    map_key(&self.prog.type_arena, core)
                } else {
                    invalid_ty
                },
                if want_value {
                    map_elem(&self.prog.type_arena, core)
                } else {
                    invalid_ty
                },
            )
        };
        let ok_var = new_var(&mut self.prog.object_arena, "ok", bool_ty);
        let k_var = new_var(&mut self.prog.object_arena, "k", key_ty);
        let v_var = new_var(&mut self.prog.object_arena, "v", val_ty);
        let okv_tuple = new_tuple(&mut self.prog.type_arena, &[ok_var, k_var, v_var]).expect("tuple");

        let block = self.block.expect("no current block");
        let next_id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Next(Next {
                iter: it,
                is_string,
                typ: okv_tuple,
            }),
            pos,
        );
        let okv = Value::Instr(next_id);

        let body = self.new_basic_block("rangeiter.body".to_string());
        let done = self.new_basic_block("rangeiter.done".to_string());
        let ok_cond = self.emit_extract(okv, 0);
        self.emit_if(ok_cond, body, done);

        self.set_block(Some(body));

        if s.tok == Some(Token::DEFINE) {
            self.range_create_vars(s, want_key, want_value);
        }

        if want_key {
            let k = self.emit_extract(okv, 1);
            if let Some(key) = &s.key {
                self.address(key, false).store(self, k);
            }
        }
        if want_value {
            let v = self.emit_extract(okv, 2);
            if let Some(value) = &s.value {
                self.address(value, false).store(self, v);
            }
        }

        // go/ssa sets the labelled statement's break/continue targets *before*
        // building the body (`label._break = done`). Setting them after left
        // every `break <label>` inside the body unresolved, and `branch_stmt`
        // falls back to the label's goto block on a miss — so a labelled break
        // jumped to the top of the loop it was trying to leave. Every CFG
        // consumer saw the wrong edge; wastedassign saw the loop's own next
        // store and called the assignment before the break wasted (gitea
        // `services/gitdiff`).
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }
        self.push_targets(done, loop_);
        for stmt in &s.body.list {
            self.stmt_with_label(stmt, None);
        }
        self.pop_targets();

        if self.block.is_some() {
            self.emit_jump(loop_);
        }
        self.set_block(Some(done));
    }

    /// Creates iteration variables for `for k, v := range x` (Go 1.22+:
    /// inside the loop). (Go: `rangeStmt`'s `createVars`.)
    pub(crate) fn range_create_vars(&mut self, s: &RangeStmt, want_key: bool, want_value: bool) {
        if want_key {
            if let Some(Expr::Ident(id)) = s.key.as_ref() {
                if !is_blank_name(id) {
                    self.local_var(id);
                }
            }
        }
        if want_value {
            if let Some(Expr::Ident(id)) = s.value.as_ref() {
                if !is_blank_name(id) {
                    self.local_var(id);
                }
            }
        }
    }

    fn range_indexed_value(&mut self, x: Value, x_ty: TypeId, index: Value) -> Value {
        let core = x_ty.underlying(&self.prog.type_arena);
        let block = self.block.expect("no current block");
        // (Go: `instr.setPos(x.Pos())` in all three arms of `rangeIndexed` —
        // the range expression, not the `for` keyword. nilness reports
        // "range of nil slice" at exactly this position.)
        let x_pos = crate::program::value_pos(self.prog, self.func(), x);
        match self.prog.type_arena.get(core) {
            TypeData::Array(_) => {
                let elem = array_elem(&self.prog.type_arena, core);
                let id = crate::emit::emit_with_pos(
                    self.func_mut(),
                    block,
                    InstrData::Index(Index { x, index, typ: elem }),
                    x_pos,
                );
                Value::Instr(id)
            }
            TypeData::Slice(_) => {
                let elem = slice_elem(&self.prog.type_arena, core);
                let ptr_ty = guff_types::new_pointer(&mut self.prog.type_arena, elem);
                let iaddr =
                    crate::emit::emit_index_addr(self.prog, self.func_id, block, x, index, ptr_ty, x_pos);
                self.emit_load(iaddr, elem)
            }
            TypeData::Pointer(_) => {
                let arr = pointer_elem(&self.prog.type_arena, core);
                let elem = array_elem(&self.prog.type_arena, arr);
                let ptr_ty = guff_types::new_pointer(&mut self.prog.type_arena, elem);
                let iaddr =
                    crate::emit::emit_index_addr(self.prog, self.func_id, block, x, index, ptr_ty, x_pos);
                self.emit_load(iaddr, elem)
            }
            // Incomplete hybrid info mis-routed an indexed range — placeholder.
            _ => self.invalid_zero(),
        }
    }

    fn emit_binop(&mut self, op: Token, x: Value, y: Value, typ: TypeId, pos: guff::Pos) -> Value {
        let block = self.block.expect("no current block");
        let id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::BinOp(BinOp { op, x, y, typ }),
            pos,
        );
        Value::Instr(id)
    }

    pub(crate) fn emit_compare(&mut self, op: Token, x: Value, y: Value, pos: guff::Pos) -> Value {
        let bool_ty = self.prog.basic_type(BasicKind::Bool);
        self.emit_binop(op, x, y, bool_ty, pos)
    }

    fn emit_len(&mut self, x: Value, pos: guff::Pos) -> Value {
        let int_ty = self.prog.basic_type(BasicKind::Int);
        let len_fn = self.builtin_ssa(BuiltinId::Len);
        let block = self.block.expect("no current block");
        let id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Call(Call {
                call: CallCommon {
                    value: len_fn,
                    method: None,
                    args: vec![x],
                    ellipsis: false,
                },
                typ: int_ty,
            }),
            pos,
        );
        Value::Instr(id)
    }

    fn builtin_ssa(&mut self, id: BuiltinId) -> Value {
        use guff_types::ObjectData;
        for oid in self.prog.object_arena.ids() {
            if let ObjectData::Builtin(b) = self.prog.object_arena.get(oid) {
                if b.id() == id {
                    let b_ssa = crate::program::Builtin {
                        name: b.name().to_string(),
                        typ: b.typ(),
                    };
                    let bid = self.prog.builtins.alloc(b_ssa);
                    return Value::Builtin(bid);
                }
            }
        }
        panic!("builtin {id:?} not found in object arena");
    }

    /// [`chan_elem`], tolerant of an operand that is not a channel.
    ///
    /// go/ssa can assume the type-checker typed every receive, but guff also
    /// builds SSA for packages whose checker info is incomplete, where a
    /// receive operand can come back Invalid. Panicking here unwinds the worker
    /// thread and drops every finding for the package, so fall back to Invalid
    /// the same way [`Builder::type_of`] does.
    fn chan_elem_or_invalid(&mut self, t: TypeId) -> TypeId {
        if matches!(self.prog.type_arena.get(t), TypeData::Chan(_)) {
            chan_elem(&self.prog.type_arena, t)
        } else {
            self.prog.basic_type(BasicKind::Invalid)
        }
    }

    /// range_chan emits the header for a loop that receives from channel `x`
    /// until it is closed. (Go: `builder.rangeChan`)
    fn range_chan(
        &mut self,
        s: &RangeStmt,
        x: crate::value::Value,
        u: guff_types::TypeId,
        label: Option<&str>,
    ) {
        let elem = self.chan_elem_or_invalid(u);
        let bool_ty = self.prog.basic_type(BasicKind::Bool);
        let want_key = s
            .key
            .as_ref()
            .is_some_and(|k| !is_blank_ident(k));

        let loop_ = self.new_basic_block("rangechan.loop".to_string());
        self.emit_jump(loop_);
        self.set_block(Some(loop_));

        let k_var = new_var(&mut self.prog.object_arena, "k", elem);
        let ok_var = new_var(&mut self.prog.object_arena, "ok", bool_ty);
        let recv_tuple = new_tuple(&mut self.prog.type_arena, &[k_var, ok_var]).expect("tuple");

        let block = self.block.expect("no current block");
        let recv_id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::UnOp(UnOp {
                op: Token::ARROW,
                x,
                comma_ok: true,
                typ: recv_tuple,
            }),
            s.tok_pos,
        );
        let ko = crate::value::Value::Instr(recv_id);

        let body = self.new_basic_block("rangechan.body".to_string());
        let done = self.new_basic_block("rangechan.done".to_string());
        let ok_cond = self.emit_extract(ko, 1);
        self.emit_if(ok_cond, body, done);

        self.set_block(Some(body));
        if want_key {
            let k = self.emit_extract(ko, 0);
            if let Some(key) = &s.key {
                let lval = self.address(key, false);
                lval.store(self, k);
            }
        }

        // go/ssa sets the labelled statement's break/continue targets *before*
        // building the body (`label._break = done`). Setting them after left
        // every `break <label>` inside the body unresolved, and `branch_stmt`
        // falls back to the label's goto block on a miss — so a labelled break
        // jumped to the top of the loop it was trying to leave. Every CFG
        // consumer saw the wrong edge; wastedassign saw the loop's own next
        // store and called the assignment before the break wasted (gitea
        // `services/gitdiff`).
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }
        self.push_targets(done, loop_);
        for stmt in &s.body.list {
            self.stmt_with_label(stmt, None);
        }
        self.pop_targets();

        if self.block.is_some() {
            self.emit_jump(loop_);
        }
        self.set_block(Some(done));
    }

    fn defer_stmt(&mut self, s: &DeferStmt) {
        let mut c = CallCommon {
            value: crate::value::Value::Builtin(unsafe { std::mem::transmute(1u32) }),
            method: None,
            args: Vec::new(),
            ellipsis: false,
        };
        self.set_call(&s.call, &mut c);
        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Defer(crate::instr::Defer { call: c }),
            s.defer_,
        );
    }

    fn go_stmt(&mut self, s: &GoStmt) {
        let mut c = CallCommon {
            value: crate::value::Value::Builtin(unsafe { std::mem::transmute(1u32) }),
            method: None,
            args: Vec::new(),
            ellipsis: false,
        };
        self.set_call(&s.call, &mut c);
        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Go(crate::instr::Go { call: c }),
            s.go_,
        );
    }

    fn if_stmt(&mut self, s: &IfStmt) {
        if let Some(init) = &s.init {
            self.stmt(init);
        }
        let t = self.new_basic_block("if.then".to_string());
        let done = self.new_basic_block("if.done".to_string());
        let mut e = done;
        if s.else_.is_some() {
            e = self.new_basic_block("if.else".to_string());
        }
        self.cond(&s.cond, t, e);

        self.set_block(Some(t));
        self.stmt(&Stmt::BlockStmt(s.body.clone()));
        if self.block.is_some() {
            self.emit_jump(done);
        }

        if let Some(els) = &s.else_ {
            self.set_block(Some(e));
            self.stmt(els);
            if self.block.is_some() {
                self.emit_jump(done);
            }
        }

        self.set_block(Some(done));
    }

    /// send_stmt emits `ch <- x`. (Go: `*ast.SendStmt` case of `builder.stmt`)
    fn send_stmt(&mut self, s: &SendStmt) {
        let ch = self.expr(&s.chan_);
        let ch_ty = self.type_of_value(ch);
        let core = ch_ty.underlying(&self.prog.type_arena);
        let elem = self.chan_elem_or_invalid(core);
        let x = self.expr(&s.value);
        let block = self.block.expect("no current block");
        let x = crate::emit::emit_type_coercion(self.prog, self.func_id, block, x, elem);
        self.emit_pos(
            InstrData::Send(Send { chan: ch, x }),
            s.arrow,
        );
    }

    /// inc_dec_stmt emits `x++` / `x--` as `x = x ± 1`. (Go: `*ast.IncDecStmt`)
    fn inc_dec_stmt(&mut self, s: &IncDecStmt) {
        let op = if s.tok == Token::DEC {
            Token::SUB
        } else {
            Token::ADD
        };
        let loc = self.address(&s.x, false);
        let one = self.int_const(1);
        self.assign_op(loc, one, op, s.tok_pos);
    }

    /// assign_op emits `loc = loc <op> val`. (Go: `builder.assignOp`)
    fn assign_op(
        &mut self,
        loc: Box<dyn crate::lvalue::LValue>,
        val: Value,
        op: Token,
        pos: guff::Pos,
    ) {
        let typ = loc.typ();
        let old = loc.load(self);
        let block = self.block.expect("no current block");
        // Coerce `val` (often untyped int `1`) to the location's type.
        let val = crate::emit::emit_type_coercion(self.prog, self.func_id, block, val, typ);
        let result = self.emit_binop(op, old, val, typ, pos);
        loc.store(self, result);
    }

    /// switch_stmt lowers a value `switch` to an if-else chain.
    /// Multiway dispatch is recovered later by `ssautil::switches`.
    /// (Go: `builder.switchStmt`)
    fn switch_stmt(&mut self, s: &SwitchStmt, label: Option<&str>) {
        if let Some(init) = &s.init {
            self.stmt(init);
        }
        // No tag ⇒ boolean switch (each case is a bool condition).
        let tag = s.tag.as_ref().map(|t| self.expr(t));
        let bool_switch = tag.is_none();

        let done = self.new_basic_block("switch.done".to_string());
        if let Some(name) = label {
            self.set_label_break(name, done);
        }

        let mut dflt_body: Option<&[Stmt]> = None;
        let mut dflt_fallthrough = None;
        let mut dflt_block = None;
        let mut fallthru: Option<crate::ids::BlockId> = None;
        let ncases = s.body.list.len();

        for (i, clause) in s.body.list.iter().enumerate() {
            let Stmt::CaseClause(cc) = clause else {
                panic!("switch body must be CaseClause");
            };

            let body = match fallthru {
                Some(b) => b,
                None => self.new_basic_block("switch.body".to_string()),
            };

            // Preallocate body block for the next case (fallthrough target).
            fallthru = if i + 1 < ncases {
                Some(self.new_basic_block("switch.body".to_string()))
            } else {
                Some(done)
            };

            if cc.list.is_empty() {
                dflt_body = Some(&cc.body);
                dflt_fallthrough = fallthru;
                dflt_block = Some(body);
                continue;
            }

            let mut next_cond = None;
            for cond in &cc.list {
                let nc = self.new_basic_block("switch.next".to_string());
                if bool_switch {
                    let cond_ty = self.type_of(cond.id());
                    if !guff_types::is_non_type_param_interface(&self.prog.type_arena, cond_ty) {
                        self.cond(cond, body, nc);
                    } else {
                        let bool_ty = self.prog.basic_type(BasicKind::Bool);
                        let v_true = self
                            .prog
                            .emit_const(Some(guff_constant::Value::Bool(true)), bool_ty);
                        let rhs = self.expr(cond);
                        let c = self.emit_compare(Token::EQL, rhs, v_true, cond.pos());
                        self.emit_if(c, body, nc);
                    }
                } else {
                    let tag_v = tag.expect("tag present for value switch");
                    let rhs = self.expr(cond);
                    let c = self.emit_compare(Token::EQL, tag_v, rhs, cond.pos());
                    self.emit_if(c, body, nc);
                }
                self.set_block(Some(nc));
                next_cond = Some(nc);
            }

            self.set_block(Some(body));
            self.push_break_targets(done, fallthru);
            for stmt in &cc.body {
                self.stmt(stmt);
            }
            self.pop_targets();
            if self.block.is_some() {
                self.emit_jump(done);
            }
            if let Some(nc) = next_cond {
                self.set_block(Some(nc));
            }
        }

        if let Some(db) = dflt_block {
            if self.block.is_some() {
                self.emit_jump(db);
            }
            self.set_block(Some(db));
            self.push_break_targets(done, dflt_fallthrough);
            if let Some(body) = dflt_body {
                for stmt in body {
                    self.stmt(stmt);
                }
            }
            self.pop_targets();
        }
        if self.block.is_some() {
            self.emit_jump(done);
        }
        self.set_block(Some(done));
    }

    /// type_switch_stmt lowers `switch x.(type)` to a type-assert if-else chain.
    /// (Go: `builder.typeSwitchStmt`)
    fn type_switch_stmt(&mut self, s: &TypeSwitchStmt, label: Option<&str>) {
        if let Some(init) = &s.init {
            self.stmt(init);
        }

        let x = match s.assign.as_ref() {
            Stmt::ExprStmt(es) => {
                let ta = type_assert_of(&es.x);
                self.expr(&ta.x)
            }
            Stmt::AssignStmt(as_) => {
                let ta = type_assert_of(&as_.rhs[0]);
                self.expr(&ta.x)
            }
            other => panic!("type switch assign is ExprStmt or AssignStmt, got {other:?}"),
        };

        let done = self.new_basic_block("typeswitch.done".to_string());
        if let Some(name) = label {
            self.set_label_break(name, done);
        }

        let mut default_: Option<&CaseClause> = None;
        for clause in &s.body.list {
            let Stmt::CaseClause(cc) = clause else {
                panic!("type switch body must be CaseClause");
            };
            if cc.list.is_empty() {
                default_ = Some(cc);
                continue;
            }

            let body = self.new_basic_block("typeswitch.body".to_string());
            let mut next = None;
            let mut ti = x;
            for cond in &cc.list {
                let nc = self.new_basic_block("typeswitch.next".to_string());
                let ct = self.type_of(cond.id());
                let condv = if self.is_untyped_nil(ct) {
                    let zero = self.prog.emit_const(None, self.type_of_value(x));
                    ti = x;
                    self.emit_compare(Token::EQL, x, zero, cond.pos())
                } else {
                    let fid = self.func_id;
                    let block = self.block.expect("no current block");
                    let yok =
                        crate::emit::emit_type_test(self.prog, fid, block, x, ct, cc.case);
                    ti = self.emit_extract(yok, 0);
                    self.emit_extract(yok, 1)
                };
                self.emit_if(condv, body, nc);
                self.set_block(Some(nc));
                next = Some(nc);
            }
            if cc.list.len() != 1 {
                ti = x;
            }
            self.set_block(Some(body));
            self.type_case_body(cc, ti, done);
            if let Some(nc) = next {
                self.set_block(Some(nc));
            }
        }
        if let Some(cc) = default_ {
            self.type_case_body(cc, x, done);
        } else if self.block.is_some() {
            self.emit_jump(done);
        }
        self.set_block(Some(done));
    }

    fn type_case_body(&mut self, cc: &CaseClause, x: Value, done: crate::ids::BlockId) {
        if let Some(obj) = self.prog.info.implicits.get(&cc.id).copied() {
            if matches!(
                self.prog.object_arena.get(obj),
                guff_types::ObjectData::Var(_)
            ) {
                let block = self.block.expect("no current block");
                let local = crate::emit::emit_local_var(self.prog, self.func_id, block, obj);
                let block = self.block.expect("no current block");
                crate::emit::emit(
                    self.func_mut(),
                    block,
                    InstrData::Store(crate::instr::Store {
                        addr: local,
                        val: x,
                    }),
                );
            }
        }
        self.push_break_targets(done, None);
        for stmt in &cc.body {
            self.stmt(stmt);
        }
        self.pop_targets();
        if self.block.is_some() {
            self.emit_jump(done);
        }
    }

    /// select_stmt lowers `select { … }`. (Go: `builder.selectStmt`)
    fn select_stmt(&mut self, s: &SelectStmt, label: Option<&str>) {
        // A blocking select of a single case degenerates to a simple send/recv.
        if s.body.list.len() == 1 {
            if let Stmt::CommClause(clause) = &s.body.list[0] {
                if let Some(comm) = &clause.comm {
                    self.stmt(comm);
                    let done = self.new_basic_block("select.done".to_string());
                    if let Some(name) = label {
                        self.set_label_break(name, done);
                    }
                    self.push_break_targets(done, None);
                    for stmt in &clause.body {
                        self.stmt(stmt);
                    }
                    self.pop_targets();
                    if self.block.is_some() {
                        self.emit_jump(done);
                    }
                    self.set_block(Some(done));
                    return;
                }
            }
        }

        let mut states: Vec<SelectState> = Vec::new();
        let mut blocking = true;
        for clause in &s.body.list {
            let Stmt::CommClause(cc) = clause else {
                panic!("select body must be CommClause");
            };
            match cc.comm.as_deref() {
                None => {
                    blocking = false;
                }
                Some(Stmt::SendStmt(send)) => {
                    let ch = self.expr(&send.chan_);
                    let ch_ty = self.type_of_value(ch);
                    let core = ch_ty.underlying(&self.prog.type_arena);
                    let elem = self.chan_elem_or_invalid(core);
                    let block = self.block.expect("no current block");
                    let val = self.expr(&send.value);
                    let val =
                        crate::emit::emit_type_coercion(self.prog, self.func_id, block, val, elem);
                    states.push(SelectState {
                        dir: ChanDir::SendOnly,
                        chan: ch,
                        send: Some(val),
                    });
                }
                Some(Stmt::AssignStmt(as_)) => {
                    let recv = unparen_unary_recv(&as_.rhs[0]);
                    states.push(SelectState {
                        dir: ChanDir::RecvOnly,
                        chan: self.expr(&recv.x),
                        send: None,
                    });
                }
                Some(Stmt::ExprStmt(es)) => {
                    let recv = unparen_unary_recv(&es.x);
                    states.push(SelectState {
                        dir: ChanDir::RecvOnly,
                        chan: self.expr(&recv.x),
                        send: None,
                    });
                }
                Some(other) => panic!("unexpected select comm: {other:?}"),
            }
        }

        let int_ty = self.prog.basic_type(BasicKind::Int);
        let bool_ty = self.prog.basic_type(BasicKind::Bool);
        let mut vars = vec![
            new_var(&mut self.prog.object_arena, "index", int_ty),
            new_var(&mut self.prog.object_arena, "ok", bool_ty),
        ];
        for st in &states {
            if st.dir == ChanDir::RecvOnly {
                let ch_ty = self.type_of_value(st.chan);
                let core = ch_ty.underlying(&self.prog.type_arena);
                let elem = self.chan_elem_or_invalid(core);
                vars.push(new_var(&mut self.prog.object_arena, "", elem));
            }
        }
        let sel_ty = new_tuple(&mut self.prog.type_arena, &vars).expect("select tuple");
        let sel_id = self.emit_pos(
            InstrData::Select(Select {
                states: states.clone(),
                blocking,
                typ: sel_ty,
            }),
            s.select_,
        );
        let sel = Value::Instr(sel_id);
        let idx = self.emit_extract(sel, 0);

        let done = self.new_basic_block("select.done".to_string());
        if let Some(name) = label {
            self.set_label_break(name, done);
        }

        let mut default_body: Option<&[Stmt]> = None;
        let mut state = 0usize;
        let mut r = 2usize; // index in sel tuple of next recv value
        for clause in &s.body.list {
            let Stmt::CommClause(cc) = clause else {
                continue;
            };
            if cc.comm.is_none() {
                default_body = Some(&cc.body);
                continue;
            }
            let body = self.new_basic_block("select.body".to_string());
            let next = self.new_basic_block("select.next".to_string());
            let state_const = self.int_const(state as i64);
            let cmp = self.emit_compare(Token::EQL, idx, state_const, guff::NO_POS);
            self.emit_if(cmp, body, next);
            self.set_block(Some(body));
            self.push_break_targets(done, None);

            match cc.comm.as_deref() {
                Some(Stmt::ExprStmt(_)) => {
                    r += 1;
                }
                Some(Stmt::AssignStmt(as_)) => {
                    if as_.tok == Some(Token::DEFINE) {
                        if let Expr::Ident(id) = &as_.lhs[0] {
                            if let Some(Some(obj)) = self.prog.info.defs.get(&id.id).copied() {
                                if matches!(
                                    self.prog.object_arena.get(obj),
                                    guff_types::ObjectData::Var(_)
                                ) {
                                    let block = self.block.expect("no current block");
                                    crate::emit::emit_local_var(
                                        self.prog, self.func_id, block, obj,
                                    );
                                }
                            }
                        }
                    }
                    let x = self.address(&as_.lhs[0], false);
                    let v = self.emit_extract(sel, r);
                    x.store(self, v);
                    if as_.lhs.len() == 2 {
                        if as_.tok == Some(Token::DEFINE) {
                            if let Expr::Ident(id) = &as_.lhs[1] {
                                if let Some(Some(obj)) = self.prog.info.defs.get(&id.id).copied() {
                                    if matches!(
                                        self.prog.object_arena.get(obj),
                                        guff_types::ObjectData::Var(_)
                                    ) {
                                        let block = self.block.expect("no current block");
                                        crate::emit::emit_local_var(
                                            self.prog, self.func_id, block, obj,
                                        );
                                    }
                                }
                            }
                        }
                        let ok = self.address(&as_.lhs[1], false);
                        let ok_val = self.emit_extract(sel, 1);
                        ok.store(self, ok_val);
                    }
                    r += 1;
                }
                _ => {}
            }

            for stmt in &cc.body {
                self.stmt(stmt);
            }
            self.pop_targets();
            if self.block.is_some() {
                self.emit_jump(done);
            }
            self.set_block(Some(next));
            state += 1;
        }

        if let Some(body) = default_body {
            self.push_break_targets(done, None);
            for stmt in body {
                self.stmt(stmt);
            }
            self.pop_targets();
        } else {
            // A blocking select must match some case.
            let msg = self.prog.emit_const(
                Some(guff_constant::make_string(
                    "blocking select matched no case",
                )),
                self.prog.basic_type(BasicKind::String),
            );
            self.emit(InstrData::Panic(Panic { x: msg }));
            let unreachable = self.new_basic_block("unreachable".to_string());
            self.set_block(Some(unreachable));
        }
        if self.block.is_some() {
            self.emit_jump(done);
        }
        self.set_block(Some(done));
    }

    fn set_label_break(&mut self, name: &str, break_: crate::ids::BlockId) {
        if let Some(lb) = self.prog.functions.get_mut(self.func_id).lblocks.get_mut(name) {
            lb.break_ = Some(break_);
        }
    }

    fn is_untyped_nil(&self, t: guff_types::TypeId) -> bool {
        match self.prog.type_arena.get(t) {
            TypeData::Basic(b) => b.kind() == BasicKind::UntypedNil,
            _ => false,
        }
    }

    fn type_of_value(&self, v: Value) -> guff_types::TypeId {
        crate::program::value_type_of(self.prog, self.prog.functions.get(self.func_id), v)
    }

    fn for_stmt(&mut self, s: &ForStmt, label: Option<&str>) {
        if let Some(init) = &s.init {
            self.stmt_with_label(init, None);
        }
        let body = self.new_basic_block("for.body".to_string());
        let done = self.new_basic_block("for.done".to_string());
        let loop_ = self.new_basic_block("for.loop".to_string());

        self.emit_jump(loop_);
        self.set_block(Some(loop_));

        if let Some(cond) = &s.cond {
            self.cond(cond, body, done);
        } else {
            self.emit_jump(body);
        }

        self.set_block(Some(body));
        // go/ssa sets the labelled statement's break/continue targets *before*
        // building the body (`label._break = done`). Setting them after left
        // every `break <label>` inside the body unresolved, and `branch_stmt`
        // falls back to the label's goto block on a miss — so a labelled break
        // jumped to the top of the loop it was trying to leave. Every CFG
        // consumer saw the wrong edge; wastedassign saw the loop's own next
        // store and called the assignment before the break wasted (gitea
        // `services/gitdiff`).
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }
        self.push_targets(done, loop_);
        self.stmt(&Stmt::BlockStmt(s.body.clone()));
        if let Some(post) = &s.post {
            self.stmt_with_label(post, None);
        }
        self.pop_targets();
        if self.block.is_some() {
            self.emit_jump(loop_);
        }

        self.set_block(Some(done));
    }

    /// assign_stmt translates an assignment or short variable declaration.
    /// The side effects of all LHSs and RHSs must occur in left-to-right order,
    /// so it computes every lvalue first (evaluating LHS receivers eagerly),
    /// then evaluates the RHSs, and only then emits the stores. Deferring the
    /// stores lets a parallel assignment (`x, y = y, x`) and short var decl read
    /// the old LHS values. (Go: `builder.assignStmt`, whose `storebuf` this
    /// two-phase structure inlines.)
    ///
    /// `:=` (a short variable declaration, `s.tok == DEFINE`) creates a fresh
    /// local cell for each newly defined variable before taking its address.
    /// Compound assignments (`+=`, etc.) lower to `loc = loc <op> rhs`.
    fn assign_stmt(&mut self, s: &AssignStmt) {
        use guff::token::Token;

        // Compound assignment: x += y → x = x + y
        if let Some(tok) = s.tok {
            if let Some(op) = compound_assign_op(tok) {
                let loc = self.address(&s.lhs[0], false);
                let val = self.expr(&s.rhs[0]);
                self.assign_op(loc, val, op, s.tok_pos);
                return;
            }
        }

        let is_def = s.tok == Some(Token::DEFINE);

        // Phase 1: compute an lvalue for each LHS (left-to-right), creating a
        // local cell first for each variable a `:=` newly defines.
        let mut lvals: Vec<Box<dyn crate::lvalue::LValue>> = Vec::with_capacity(s.lhs.len());
        for lhs in &s.lhs {
            if is_blank_ident(lhs) {
                lvals.push(Box::new(crate::lvalue::Blank));
                continue;
            }
            if is_def {
                if let Expr::Ident(id) = lhs {
                    if let Some(Some(obj)) = self.prog.info.defs.get(&id.id).copied() {
                        if matches!(self.prog.object_arena.get(obj), guff_types::ObjectData::Var(_)) {
                            let block = self.block.expect("no current block");
                            crate::emit::emit_local_var(self.prog, self.func_id, block, obj);
                        }
                    }
                }
            }
            lvals.push(self.address(lhs, false)); // non-escaping
        }

        if s.lhs.len() == s.rhs.len() {
            // Simple / parallel assignment or short var decl. The RHSs may refer
            // to the LHSs, so evaluate every RHS into a store buffer, then flush.
            // Each newly-`:=`-defined cell holds its zero value (`is_zero`).
            let mut sb = crate::lvalue::StoreBuf::new();
            for (lval, r) in lvals.into_iter().zip(&s.rhs) {
                self.assign(lval, r, is_def, Some(&mut sb));
            }
            sb.emit(self);
        } else {
            // Multi-valued assignment: a, b = f()
            let tuple = self.expr_n(&s.rhs[0]);
            self.emit_debug_ref(&s.rhs[0], tuple, false);
            let block = self.block.expect("no current block");
            for (i, lval) in lvals.into_iter().enumerate() {
                let elem = crate::emit::emit_extract(self.prog, self.func_id, block, tuple, i);
                lval.store(self, elem);
            }
        }
    }

    /// assign emits code to initialize the lvalue `loc` with the value of
    /// expression `e`. It is equivalent to `loc.store(expr(e))` but generates
    /// better code for a composite literal in an addressable location: when
    /// `loc` is a pointer-core location and `e` is a composite literal, an
    /// `&`-operation is implied and the literal's address is stored. When `sb`
    /// is `Some`, the store is deferred into the buffer (evaluating `e` still
    /// happens eagerly) so a group of assignments can update in the right order.
    /// (Go: `builder.assign`. `is_zero` is accepted for parity but, as in go, is
    /// not consulted here.)
    pub(crate) fn assign(
        &mut self,
        loc: Box<dyn crate::lvalue::LValue>,
        e: &Expr,
        _is_zero: bool,
        sb: Option<&mut crate::lvalue::StoreBuf>,
    ) {
        // Can we initialize it in place?
        let inner = crate::builder::unparen(e);
        if let Expr::CompositeLit(_) = inner {
            // A composite literal never evaluates to a pointer, so if the
            // location's type is a pointer, an `&`-operation is implied.
            if !loc.is_blank() && guff_types::is_pointer(&self.prog.type_arena, loc.typ()) {
                let ptr = self.address(inner, true).address(self);
                match sb {
                    Some(sb) => sb.store(loc, ptr),
                    None => loc.store(self, ptr),
                }
                return;
            }
        }

        // Simple case: evaluate the RHS and store it.
        let rhs = self.expr(e);
        match sb {
            Some(sb) => sb.store(loc, rhs),
            None => loc.store(self, rhs),
        }
    }

    /// comp_lit emits code to initialize the aggregate at address `addr` from
    /// the composite literal `cl`. `is_zero` reports whether `addr` already holds
    /// the zero value (so field/element clearing can be skipped). Stores are
    /// appended to `sb` for correct in-place initialization. (Go:
    /// `builder.compLit`.)
    pub(crate) fn comp_lit(
        &mut self,
        addr: crate::value::Value,
        cl: &guff::ast::CompositeLit,
        is_zero: bool,
        sb: &mut crate::lvalue::StoreBuf,
    ) {
        use guff_types::TypeData;
        // typ = Deref(typeOf(cl)): retain the named/alias type, but strip the
        // implicit pointer of a nested literal (e.g. the `*T` element of
        // `[]*T{{}}`, where `addr` holds a `T`).
        let raw = self.type_of(cl.id);
        let typ = if guff_types::is_pointer(&self.prog.type_arena, raw) {
            guff_types::pointer_elem(&self.prog.type_arena, raw)
        } else {
            raw
        };
        let u = typ.underlying(&self.prog.type_arena);
        match self.prog.type_arena.get(u) {
            TypeData::Struct(_) => self.comp_lit_struct(addr, cl, typ, u, is_zero, sb),
            TypeData::Array(_) | TypeData::Slice(_) => {
                self.comp_lit_array_slice(addr, cl, typ, u, is_zero, sb)
            }
            TypeData::Map(_) => self.comp_lit_map(addr, cl, typ, u, sb),
            // Ill-typed packages can surface Invalid/basic here; skip rather than
            // aborting SSA for the whole package (needed for contextcheck parity).
            _ => {}
        }
    }

    /// comp_lit_struct handles the struct case of [`comp_lit`]. Each element is a
    /// field initializer, keyed (`{f: v}`) or positional (`{v0, v1}`); a
    /// composite-literal key always names a *direct* field, so the field address
    /// is a single `FieldAddr` on `addr` (no implicit embedded selection).
    fn comp_lit_struct(
        &mut self,
        addr: crate::value::Value,
        cl: &guff::ast::CompositeLit,
        typ: guff_types::TypeId,
        u_struct: guff_types::TypeId,
        mut is_zero: bool,
        sb: &mut crate::lvalue::StoreBuf,
    ) {
        let nfields = guff_types::struct_num_fields(&self.prog.type_arena, u_struct);
        if !is_zero && cl.elts.len() != nfields {
            // A partial literal of a non-zero location: clear it first (the zero
            // value of `typ`, since `addr` holds a `*typ`). (Go: memclear.)
            let zero = self.prog.emit_const(None, typ);
            let loc = Box::new(crate::lvalue::Address { addr, typ, pos: cl.lbrace, expr: None });
            sb.store(loc, zero);
            is_zero = true;
        }

        for (i, elt) in cl.elts.iter().enumerate() {
            let (field_index, value_expr, pos): (usize, &Expr, guff::Pos) = match elt {
                Expr::KeyValueExpr(kv) => {
                    let fname = match kv.key.as_ref() {
                        Expr::Ident(id) => id.name.clone(),
                        other => panic!("struct literal key is not an identifier: {other:?}"),
                    };
                    let idx = self.struct_field_index(u_struct, &fname);
                    (idx, kv.value.as_ref(), kv.colon)
                }
                _ => (i, elt, elt.pos()),
            };

            let fld = guff_types::struct_field(&self.prog.type_arena, u_struct, field_index);
            let ftype = fld
                .typ(&self.prog.object_arena)
                .expect("struct field has a type");
            let ptr_ty = guff_types::new_pointer(&mut self.prog.type_arena, ftype);
            let block = self.block.expect("no current block");
            let faddr = crate::emit::emit_with_pos(
                self.func_mut(),
                block,
                InstrData::FieldAddr(crate::instr::FieldAddr {
                    x: addr,
                    field: field_index,
                    typ: ptr_ty,
                }),
                pos,
            );
            let loc = Box::new(crate::lvalue::Address {
                addr: crate::value::Value::Instr(faddr),
                typ: ftype,
                pos,
                expr: None,
            });
            self.assign(loc, value_expr, is_zero, Some(sb));
        }
    }

    /// struct_field_index returns the position of the direct field named `name`
    /// in the struct whose underlying type is `u_struct`. (Go: the field index
    /// from `types.LookupFieldOrMethod`, which for a composite-literal key is a
    /// direct, non-promoted field.)
    fn struct_field_index(&self, u_struct: guff_types::TypeId, name: &str) -> usize {
        let n = guff_types::struct_num_fields(&self.prog.type_arena, u_struct);
        for i in 0..n {
            let fld = guff_types::struct_field(&self.prog.type_arena, u_struct, i);
            if fld.name(&self.prog.object_arena) == name {
                return i;
            }
        }
        panic!("struct field {name:?} not found");
    }

    /// comp_lit_array_slice handles the array and slice cases of [`comp_lit`].
    /// For an array the elements are written in place at `addr`; for a slice a
    /// fresh backing array is heap-allocated (`new [N]T (slicelit)`), filled,
    /// and resliced (`slice arr[:]`) into `addr`. Because a slice's backing
    /// array is unaliased its element stores need no store buffer, whereas an
    /// array is filled in place through `sb`. Elements are positional or keyed
    /// by a constant index; a keyed index resets the running position. (Go: the
    /// `*types.Array, *types.Slice` case of `builder.compLit`.)
    fn comp_lit_array_slice(
        &mut self,
        addr: crate::value::Value,
        cl: &guff::ast::CompositeLit,
        typ: guff_types::TypeId,
        u: guff_types::TypeId,
        is_zero: bool,
        sb: &mut crate::lvalue::StoreBuf,
    ) {
        use guff_types::TypeData;
        let is_slice = matches!(self.prog.type_arena.get(u), TypeData::Slice(_));

        // The array value to fill and its element type. A slice allocates a
        // fresh backing array of length `arrayLen(elts)`; an array fills `addr`
        // in place, clearing it first for a partial literal.
        let (array, elem) = if is_slice {
            let elem = guff_types::slice_elem(&self.prog.type_arena, u);
            let len = self.array_len(&cl.elts);
            let at = guff_types::new_array(&mut self.prog.type_arena, elem, len);
            let fid = self.func_id;
            let block = self.block.expect("no current block");
            let array =
                crate::emit::emit_new(self.prog, fid, block, at, cl.lbrace, "slicelit".to_string());
            (array, elem)
        } else {
            let elem = guff_types::array_elem(&self.prog.type_arena, u);
            let alen = guff_types::array_len(&self.prog.type_arena, u);
            if !is_zero && cl.elts.len() as i64 != alen {
                // A partial literal of a non-zero location: clear it first.
                let zero = self.prog.emit_const(None, typ);
                let loc = Box::new(crate::lvalue::Address { addr, typ, pos: cl.lbrace, expr: None });
                sb.store(loc, zero);
            }
            (addr, elem)
        };

        let ptr_elem = guff_types::new_pointer(&mut self.prog.type_arena, elem);

        // Fill each element, tracking the running index for positional elements
        // (a keyed element sets the index, positional ones increment it).
        let mut idx: Option<i64> = None;
        for elt in &cl.elts {
            let (cur, value_expr, pos): (i64, &Expr, guff::Pos) = match elt {
                Expr::KeyValueExpr(kv) => {
                    let key_v = self.expr(&kv.key);
                    (self.const_int64(key_v), kv.value.as_ref(), kv.colon)
                }
                _ => (idx.map_or(0, |v| v + 1), elt, elt.pos()),
            };
            idx = Some(cur);

            let index = self.int_const(cur);
            let block = self.block.expect("no current block");
            let iaddr = crate::emit::emit_index_addr(
                self.prog, self.func_id, block, array, index, ptr_elem, pos,
            );
            let loc = Box::new(crate::lvalue::Address { addr: iaddr, typ: elem, pos, expr: None });
            if is_slice {
                self.assign(loc, value_expr, true, None);
            } else {
                self.assign(loc, value_expr, true, Some(sb));
            }
        }

        if is_slice {
            // Reslice the backing array (`slice array[:]`) and store the slice
            // into `addr` through the store buffer.
            let block = self.block.expect("no current block");
            let sid = crate::emit::emit_with_pos(
                self.func_mut(),
                block,
                InstrData::Slice(crate::instr::Slice {
                    x: array,
                    low: None,
                    high: None,
                    max: None,
                    typ,
                }),
                cl.lbrace,
            );
            let loc = Box::new(crate::lvalue::Address { addr, typ, pos: cl.lbrace, expr: None });
            sb.store(loc, crate::value::Value::Instr(sid));
        }
    }

    /// comp_lit_map handles the map case of [`comp_lit`]: it makes a fresh map
    /// (`make map[K]V N`, where N is the element count reserved), updates it with
    /// each keyed entry (`m[k] = v`), and stores the completed map into `addr`.
    /// (Go: the `*types.Map` case of `builder.compLit`.)
    fn comp_lit_map(
        &mut self,
        addr: crate::value::Value,
        cl: &guff::ast::CompositeLit,
        typ: guff_types::TypeId,
        u: guff_types::TypeId,
        sb: &mut crate::lvalue::StoreBuf,
    ) {
        let key_ty = guff_types::map_key(&self.prog.type_arena, u);
        let elem_ty = guff_types::map_elem(&self.prog.type_arena, u);

        let reserve = self.int_const(cl.elts.len() as i64);
        let block = self.block.expect("no current block");
        let mid = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::MakeMap(crate::instr::MakeMap { reserve: Some(reserve), typ }),
            cl.lbrace,
        );
        let m = crate::value::Value::Instr(mid);

        for elt in &cl.elts {
            let kv = match elt {
                Expr::KeyValueExpr(kv) => kv,
                other => panic!("map literal element is not a key-value pair: {other:?}"),
            };

            // A composite-literal key whose element (location) type is a pointer
            // implies an `&`-operation, e.g. `map[*T]V{{}: …}`. (Go's
            // `isPointerCore` is approximated by `is_pointer` on the underlying
            // type, matching for all concrete key types.)
            let inner = crate::builder::unparen(&kv.key);
            let want_addr = matches!(inner, Expr::CompositeLit(_))
                && guff_types::is_pointer(&self.prog.type_arena, key_ty);
            let key = if want_addr {
                self.address(inner, true).address(self)
            } else {
                self.expr(&kv.key)
            };

            let block = self.block.expect("no current block");
            let k = crate::emit::emit_type_coercion(self.prog, self.func_id, block, key, key_ty);
            let loc = Box::new(crate::lvalue::Element { m, k, typ: elem_ty, pos: kv.colon });
            // In-place update is impossible and no store buffer is needed; assign
            // still handles any implied `&`-operation for a composite value.
            self.assign(loc, &kv.value, true, None);
        }

        let loc = Box::new(crate::lvalue::Address { addr, typ, pos: cl.lbrace, expr: None });
        sb.store(loc, m);
    }

    /// array_len computes the length of the backing array for an array or slice
    /// composite literal: one more than the highest element index, where keyed
    /// elements set the running index (from a constant key) and positional
    /// elements increment it. (Go: `builder.arrayLen`.)
    fn array_len(&mut self, elts: &[Expr]) -> i64 {
        let mut max: i64 = -1;
        let mut i: i64 = -1;
        for e in elts {
            if let Expr::KeyValueExpr(kv) = e {
                let key_v = self.expr(&kv.key);
                i = self.const_int64(key_v);
            } else {
                i += 1;
            }
            if i > max {
                max = i;
            }
        }
        max + 1
    }

    /// int_const returns an untyped-`int` SSA constant with value `i`.
    /// (Go: `intConst`.)
    pub(crate) fn int_const(&mut self, i: i64) -> crate::value::Value {
        let int_ty = self.prog.basic_type(guff_types::BasicKind::Int);
        self.prog.emit_const(Some(guff_constant::make_int64(i)), int_ty)
    }

    /// const_int64 reads the `i64` value of a constant SSA value, used for
    /// composite-literal index keys (which are always integer constants).
    /// (Go: `(*Const).Int64()` on the value returned by `b.expr(key)`.)
    fn const_int64(&self, v: crate::value::Value) -> i64 {
        match v {
            crate::value::Value::Const(id) => {
                let cv = self
                    .prog
                    .constants
                    .get(id)
                    .val
                    .as_ref()
                    .expect("composite literal index key is not a constant");
                let (i, _exact) = guff_constant::int64_val(cv);
                i
            }
            _ => panic!("composite literal index key did not fold to a constant"),
        }
    }

    fn decl_stmt(&mut self, s: &DeclStmt) {
        match &s.decl {
            Decl::GenDecl(g) => {
                // `Con, Var or Typ` — only `var` declares storage. A local
                // `const` is a compile-time value; go/ssa emits nothing for it
                // and reads of it resolve through the type info like any other
                // constant. Building a cell and a Store for one made
                // wastedassign call an unread `const a = iota + 1` a wasted
                // assignment — three times in gitea, where migrations declare a
                // nine-name `const (…)` block inside the migration function and
                // use only some of the names.
                // (Go: `builder.stmt`'s `case *ast.DeclStmt`, `d.Tok == token.VAR`.)
                if g.tok != Some(Token::VAR) {
                    return;
                }
                for spec in &g.specs {
                    match spec {
                        guff::ast::Spec::ValueSpec(v) => self.value_spec(v),
                        // DEFERRED: TypeSpec (nothing to do for SSA IR usually)
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// value_spec translates a local `var` declaration. In NaiveForm every
    /// local gets an `Alloc` cell (value type `*T`, recorded in `objects` and
    /// `locals` by [`crate::emit::emit_local_var`]); reads load and writes store
    /// through it, and lifting later promotes it where possible. It mirrors the
    /// three cases of go's `localValueSpec`. (Go: `builder.localValueSpec`.)
    fn value_spec(&mut self, v: &ValueSpec) {
        let n_names = v.names.len();
        let n_values = v.values.len();
        if n_values == n_names {
            // 1:1 assignment, e.g. `var x, y = 0, 1`. Create each cell (which
            // then holds its zero value), then assign its initializer. A freshly
            // allocated cell allows a composite-literal initializer to fill in
            // place. (Go: `localValueSpec`, case 1:1.)
            for (i, id) in v.names.iter().enumerate() {
                if !is_blank_name(id) {
                    self.local_var(id);
                }
                let lval = self.address(&Expr::Ident(id.clone()), false);
                self.assign(lval, &v.values[i], true, None);
            }
        } else if n_values == 0 {
            // No initializer, e.g. `var x, y int`. Locals are implicitly
            // zero-initialized; emit an address DebugRef for each.
            for id in &v.names {
                if !is_blank_name(id) {
                    let lhs = self.local_var(id);
                    self.emit_debug_ref(&Expr::Ident(id.clone()), lhs, true);
                }
            }
        } else {
            // n:1, e.g. `var x, y = pos()`. One call yields a tuple; project
            // each element into its cell.
            let tuple = self.expr_n(&v.values[0]);
            let block = self.block.expect("no current block");
            for (i, id) in v.names.iter().enumerate() {
                if !is_blank_name(id) {
                    self.local_var(id);
                    let lval = self.address(&Expr::Ident(id.clone()), false);
                    let elem = crate::emit::emit_extract(self.prog, self.func_id, block, tuple, i);
                    lval.store(self, elem);
                }
            }
        }
    }

    /// local_var creates the stack cell for the local variable defined by `id`
    /// and returns its address. (Go: `emitLocalVar(fn, identVar(fn, id))`.)
    fn local_var(&mut self, id: &Ident) -> crate::value::Value {
        let Some(obj_id) = self.prog.info.defs.get(&id.id).copied().flatten() else {
            return self.invalid_zero();
        };
        let block = self.block.expect("no current block");
        crate::emit::emit_local_var(self.prog, self.func_id, block, obj_id)
    }

    /// Number of values this function's signature returns.
    fn func_result_count(&self) -> usize {
        let Some(sig) = self.func().signature else {
            return 0;
        };
        let results = guff_types::signature::signature_results(&self.prog.type_arena, sig);
        guff_types::tuple::tuple_len(&self.prog.type_arena, results)
    }

    fn return_stmt(&mut self, s: &ReturnStmt) {
        let mut results = Vec::with_capacity(s.results.len());
        // `return f()` where f returns several values: go/ssa returns the
        // components, not the tuple, so a consumer reading `Return.results`
        // sees one value per result. Returning the tuple made nilerr read
        // traefik's `return r.rw.Write(p)` as a bare `return nil` — there was
        // no error-typed result in the list to say otherwise.
        // (Go: `builder.stmt`'s `len(s.Results) == 1 && sig.Results().Len() > 1`.)
        let want = self.func_result_count();
        if s.results.len() == 1 && want > 1 {
            let tuple = self.expr_n(&s.results[0]);
            for i in 0..want {
                results.push(self.emit_extract(tuple, i));
            }
        } else {
            for r in &s.results {
                results.push(self.expr(r));
            }
        }

        // If the function has named result variables, spill each returned value
        // into its result var and then reload them to form the returned tuple.
        // This makes a subsequent naked `return` (whose `results` is empty here)
        // and any deferred function observe the returned values. For a naked
        // return the store loop is a no-op and the reload alone builds the
        // tuple. (Go: the `fn.namedResults` handling in the ReturnStmt case.)
        // DEFERRED vs go/ssa: `fn.emitRunDefers()` runs between the spill and
        // the reload; defers are not modeled yet.
        let named = self.func().named_results.clone();
        if !named.is_empty() {
            for (i, &r) in results.iter().enumerate() {
                self.emit_store(named[i], r, s.return_);
            }
            let mut reloaded = Vec::with_capacity(named.len());
            for &nr in &named {
                let ptr_ty = crate::program::value_type_of(self.prog, self.func(), nr);
                let elem = guff_types::pointer::pointer_elem(&self.prog.type_arena, ptr_ty);
                reloaded.push(self.emit_load(nr, elem));
            }
            results = reloaded;
        }

        if self.func().jump_var.is_some() {
            // A `return` inside a range-over-func body still assigns the
            // enclosing function's results; only the *transfer* is deferred to
            // the `switch jump {…}` the loop lowers to. go/ssa stores them
            // through `fn.lookup(fn.returnVars[i], false)`, which reaches the
            // source function's cells as free variables. Without this the
            // values are dropped, and every consumer of `Return.results` sees
            // only the outer function's own returns — unparam read traefik's
            // `lookupMiInstances` as "result 1 is always nil" because the two
            // `return nil, fmt.Errorf(…)` inside its `for … range chunkIDs(…)`
            // were invisible.
            if let Some(src) = self.func().source_func {
                let vars = self.prog.functions.get(src).return_vars.clone();
                for (i, obj) in vars.iter().enumerate() {
                    let Some(&r) = results.get(i) else {
                        break;
                    };
                    let addr = self.lookup_result_var(*obj);
                    self.emit_store(addr, r, s.return_);
                }
            }
            let e = self.return_exit(s.return_);
            let jump = self.func().jump_var.expect("yield function has jump_var");
            let exit_id = self.int_const(e.id);
            self.store_jump_var(jump, exit_id, s.return_);
            let bool_ty = self.prog.basic_type(BasicKind::Bool);
            let v_false = self
                .prog
                .emit_const(Some(guff_constant::Value::Bool(false)), bool_ty);
            let block = self.block.expect("no current block");
            crate::emit::emit_with_pos(
                self.func_mut(),
                block,
                crate::instr::InstrData::Return(crate::instr::Return {
                    results: vec![v_false],
                }),
                s.return_,
            );
            let unreachable = self.new_basic_block("unreachable".to_string());
            self.set_block(Some(unreachable));
            return;
        }

        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(self.func_mut(), block, crate::instr::InstrData::Return(crate::instr::Return {
            results,
        }), s.return_);
        // Any statements following a return are unreachable, but the builder
        // may still translate them (e.g. the code after `if c { return }`).
        // go/ssa directs them into a fresh, predecessor-less block that block
        // optimization (delete_unreachable_blocks) later removes, rather than
        // leaving no current block. (Go: `fn.currentBlock = fn.newBasicBlock("unreachable")`)
        let unreachable = self.new_basic_block("unreachable".to_string());
        self.set_block(Some(unreachable));
    }
}

/// is_blank_name reports whether `id` is the blank identifier `_`.
/// (Go: `isBlankIdent`, on an `*ast.Ident`.)
fn is_blank_name(id: &guff::ast::Ident) -> bool {
    id.name == "_"
}

/// is_blank_ident reports whether `e` is the blank identifier `_`.
/// (Go: `isBlankIdent`.)
pub(crate) fn is_blank_ident(e: &guff::ast::Expr) -> bool {
    matches!(e, guff::ast::Expr::Ident(id) if is_blank_name(id))
}

/// Maps a compound-assignment token (`+=`, …) to the corresponding binary op.
fn compound_assign_op(tok: Token) -> Option<Token> {
    match tok {
        Token::AddAssign => Some(Token::ADD),
        Token::SubAssign => Some(Token::SUB),
        Token::MulAssign => Some(Token::MUL),
        Token::QuoAssign => Some(Token::QUO),
        Token::RemAssign => Some(Token::REM),
        Token::AndAssign => Some(Token::AND),
        Token::OrAssign => Some(Token::OR),
        Token::XorAssign => Some(Token::XOR),
        Token::ShlAssign => Some(Token::SHL),
        Token::ShrAssign => Some(Token::SHR),
        Token::AndNotAssign => Some(Token::AndNot),
        _ => None,
    }
}

/// Extracts the `TypeAssertExpr` from a type-switch assign RHS / ExprStmt.
fn type_assert_of(e: &Expr) -> &guff::ast::TypeAssertExpr {
    match crate::builder::unparen(e) {
        Expr::TypeAssertExpr(ta) => ta,
        other => panic!("type switch expects x.(type), got {other:?}"),
    }
}

/// Extracts `<-ch` from a select receive communication.
fn unparen_unary_recv(e: &Expr) -> &guff::ast::UnaryExpr {
    match crate::builder::unparen(e) {
        Expr::UnaryExpr(u) if u.op == Token::ARROW => u,
        other => panic!("select receive expects <-ch, got {other:?}"),
    }
}
