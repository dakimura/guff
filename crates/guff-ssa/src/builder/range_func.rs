//! SSA Builder — range-over-func (`for k, v := range f` where `f` is an iterator).
//!
//! Port of go/ssa's `builder.rangeFunc`, `buildYieldFunc`, and `buildYieldResume`.

use crate::builder::Builder;
use crate::builder::stmt::is_blank_ident;
use crate::function::{BuildStrategy, Exit};
use crate::ids::{BlockId, FuncId, ParamId};
use crate::instr::{Call, CallCommon, InstrData, MakeClosure, Panic, Return};
use crate::value::Value;
use guff::ast::RangeStmt;
use guff::token::Token;
use guff::{Pos, NO_POS};
use guff_types::object::var::new_var;
use guff_types::{signature, tuple, BasicKind};

/// Jump-state constants for range-over-func iterators. (Go: `jReady`/`jBusy`/`jDone`.)
fn j_ready(b: &mut Builder<'_>) -> Value {
    b.int_const(0)
}
fn j_busy(b: &mut Builder<'_>) -> Value {
    b.int_const(-1)
}
fn j_done(b: &mut Builder<'_>) -> Value {
    b.int_const(-2)
}

impl<'a> Builder<'a> {
    /// Emits code for `for … range f` where `f` is a function iterator. (Go:
    /// `builder.rangeFunc`.)
    pub(crate) fn range_func(&mut self, s: &RangeStmt, x: Value, label: Option<&str>) {
        let parent_fid = self.func_id;
        let loop_ = self.new_basic_block("rangefunc.loop".to_string());
        let done = self.new_basic_block("rangefunc.done".to_string());

        self.push_targets(done, loop_);
        if let Some(name) = label {
            self.set_label_loop_targets(name, done, loop_);
        }

        self.emit_jump(loop_);
        self.set_block(Some(loop_));

        let anon_idx = self.func().anon_funcs.len();
        let int_ty = self.prog.basic_type(BasicKind::Int);
        let jump_name = format!("jump${}", anon_idx + 1);
        let jump_obj = new_var(&mut self.prog.object_arena, jump_name.clone(), int_ty);
        let block = self.block.expect("no current block");
        let jump_addr = crate::emit::emit_local(
            self.prog,
            parent_fid,
            block,
            int_ty,
            s.for_,
            jump_name,
        );
        self.prog
            .functions
            .get_mut(parent_fid)
            .objects
            .insert(jump_obj, jump_addr);

        let x_ty = crate::program::value_type_of(self.prog, self.func(), x);
        let x_sig = x_ty.underlying(&self.prog.type_arena);
        let yield_sig = {
            let params = signature::signature_params(&self.prog.type_arena, x_sig).unwrap();
            let yield_var = tuple::tuple_at(&self.prog.type_arena, params, 0);
            yield_var
                .typ(&self.prog.object_arena)
                .expect("yield param has type")
        };
        let y_sig = yield_sig.underlying(&self.prog.type_arena);

        let (y_name, pkg, source_func) = {
            let parent = self.func();
            (
                format!("{}${}", parent.name, anon_idx + 1),
                parent.pkg,
                parent.source_func.unwrap_or(parent_fid),
            )
        };

        let y_fid = crate::create::create_function(self.prog, y_name, Some(parent_fid), pkg);
        {
            let y = self.prog.functions.get_mut(y_fid);
            y.signature = Some(y_sig);
            y.synthetic = Some("range-over-func yield".to_string());
            y.build_strategy = BuildStrategy::YieldFunc;
            y.jump_var = Some(jump_obj);
            y.source_func = Some(source_func);
            y.syntax_range = Some(s.clone());
            y.yield_label = label.map(str::to_string);
            if let Some(name) = label {
                y.lblocks.insert(
                    name.to_string(),
                    crate::function::LBlock {
                        name: name.to_string(),
                        resolved: false,
                        goto_: loop_,
                        break_: None,
                        continue_: None,
                    },
                );
            }
        }
        self.prog
            .functions
            .get_mut(parent_fid)
            .anon_funcs
            .push(y_fid);

        let unresolved = self.prog.functions.get(parent_fid).exits.len();
        self.build_yield_func(y_fid, jump_obj, done);

        let bindings: Vec<Value> = self
            .prog
            .functions
            .get(y_fid)
            .freevars
            .iter()
            .map(|(_, fv)| fv.outer)
            .collect();
        let block = self.block.expect("no current block");
        let closure_id = crate::emit::emit(
            self.func_mut(),
            block,
            InstrData::MakeClosure(MakeClosure {
                fn_: y_fid,
                bindings,
                typ: yield_sig,
            }),
        );
        let closure = Value::Instr(closure_id);

        let void_ty = self.prog.basic_type(BasicKind::Invalid);
        crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Call(Call {
                call: CallCommon {
                    value: x,
                    method: None,
                    args: vec![closure],
                },
                typ: void_ty,
            }),
            NO_POS,
        );

        let exits: Vec<Exit> = self.prog.functions.get(parent_fid).exits[unresolved..].to_vec();
        self.build_yield_resume(jump_obj, &exits, done);

        self.emit_jump(done);
        self.set_block(Some(done));
        self.pop_targets();
    }

    fn build_yield_func(&mut self, y_fid: FuncId, jump_obj: guff_types::ObjectId, done: BlockId) {
        let range = self
            .prog
            .functions
            .get(y_fid)
            .syntax_range
            .clone()
            .expect("yield function has RangeStmt syntax");
        let yield_label = self.prog.functions.get(y_fid).yield_label.clone();

        // go/ssa's `startBody` allocates the entry block first so it remains
        // Blocks[0] (CFG root). Create entry before yield-continue.
        let entry = {
            let mut yb = Builder::new(self.prog, y_fid);
            let e = yb.new_basic_block("entry".to_string());
            yb.set_block(Some(e));
            e
        };
        crate::create::create_syntactic_params(self.prog, y_fid, entry);

        let ycont = {
            let mut yb = Builder::new(self.prog, y_fid);
            yb.new_basic_block("yield-continue".to_string())
        };

        if let Some(name) = yield_label.as_deref() {
            if let Some(lb) = self.prog.functions.get_mut(y_fid).lblocks.get_mut(name) {
                lb.goto_ = ycont;
                lb.continue_ = Some(ycont);
                lb.resolved = true;
            }
        }

        {
            let mut yb = Builder::new(self.prog, y_fid);
            yb.set_block(Some(ycont));
            let ready = j_ready(&mut yb);
            yb.store_jump_var(jump_obj, ready, range.body.end());
            let bool_ty = yb.prog.basic_type(BasicKind::Bool);
            let v_true = yb
                .prog
                .emit_const(Some(guff_constant::Value::Bool(true)), bool_ty);
            yb.emit(InstrData::Return(Return { results: vec![v_true] }));
        }

        let mut yb = Builder::new(self.prog, y_fid);
        yb.set_block(Some(entry));

        let yloop = yb.new_basic_block("yield-loop".to_string());
        let invalid = yb.new_basic_block("yield-invalid".to_string());
        let int_ty = yb.prog.basic_type(BasicKind::Int);
        let jump_addr = yb.lookup_var(jump_obj);
        let jump_val = yb.emit_load(jump_addr, int_ty);
        let ready = j_ready(&mut yb);
        let ready_cond = yb.emit_compare(Token::EQL, jump_val, ready, NO_POS);
        yb.emit_if(ready_cond, yloop, invalid);

        yb.set_block(Some(invalid));
        let msg = yb.prog.emit_const(
            Some(guff_constant::make_string(
                "yield function called after range loop exit",
            )),
            yb.prog.basic_type(BasicKind::String),
        );
        yb.emit(InstrData::Panic(Panic { x: msg }));

        yb.set_block(Some(yloop));
        let busy = j_busy(&mut yb);
        yb.store_jump_var(jump_obj, busy, range.body.end());

        let want_key = range
            .key
            .as_ref()
            .is_some_and(|k| !is_blank_ident(k));
        let want_value = range
            .value
            .as_ref()
            .is_some_and(|v| !is_blank_ident(v));

        if range.tok == Some(Token::DEFINE) {
            yb.range_create_vars(&range, want_key, want_value);
        }

        let param_ids: Vec<ParamId> = yb.func().params.iter().map(|(id, _)| id).collect();
        let k_param = param_ids.first().copied().map(Value::Param);
        let v_param = param_ids.get(1).copied().map(Value::Param);

        if want_key {
            if let (Some(key), Some(k)) = (&range.key, k_param) {
                yb.address(key, false).store(&mut yb, k);
            }
        }
        if want_value {
            if let (Some(value), Some(v)) = (&range.value, v_param) {
                yb.address(value, false).store(&mut yb, v);
            }
        }

        let parent_fid = self.func_id;
        yb.push_targets_owned(
            crate::builder::TargetBlock {
                func: parent_fid,
                block: done,
            },
            crate::builder::TargetBlock {
                func: y_fid,
                block: ycont,
            },
        );
        for stmt in &range.body.list {
            yb.stmt_with_label(stmt, yield_label.as_deref());
        }
        yb.pop_targets();

        if yb.block.is_some() {
            yb.emit_jump(ycont);
        }

        let exits: Vec<Exit> = yb.prog.functions.get(y_fid).exits.clone();
        for e in exits {
            if let Some(ref label) = e.label {
                let lb = yb
                    .prog
                    .functions
                    .get(y_fid)
                    .lblocks
                    .get(label)
                    .cloned();
                if let Some(lb) = lb {
                    if lb.resolved {
                        continue;
                    }
                    yb.set_block(Some(lb.goto_));
                    let exit_id = yb.int_const(e.id);
                    yb.store_jump_var(jump_obj, exit_id, e.pos);
                    let bool_ty = yb.prog.basic_type(BasicKind::Bool);
                    let v_false = yb
                        .prog
                        .emit_const(Some(guff_constant::Value::Bool(false)), bool_ty);
                    yb.emit(InstrData::Return(Return { results: vec![v_false] }));
                }
            }
            if e.to != Some(y_fid) {
                if let Some(parent) = yb.prog.functions.get(y_fid).parent {
                    yb.prog.functions.get_mut(parent).exits.push(e);
                }
            }
        }

        drop(yb);
        self.prog.finish_function(y_fid);
    }

    fn build_yield_resume(
        &mut self,
        jump_obj: guff_types::ObjectId,
        exits: &[Exit],
        done: BlockId,
    ) {
        let int_ty = self.prog.basic_type(BasicKind::Int);
        let jump_addr = self.lookup_var(jump_obj);
        let v = self.emit_load(jump_addr, int_ty);
        let isbusy = self.new_basic_block("rangefunc.resume.busy".to_string());
        let ifready = self.new_basic_block("rangefunc.resume.ready.check".to_string());
        let busy = j_busy(self);
        let busy_cond = self.emit_compare(Token::EQL, v, busy, NO_POS);
        self.emit_if(busy_cond, isbusy, ifready);

        self.set_block(Some(isbusy));
        let msg = self.prog.emit_const(
            Some(guff_constant::make_string(
                "iterator call did not preserve panic",
            )),
            self.prog.basic_type(BasicKind::String),
        );
        self.emit(InstrData::Panic(Panic { x: msg }));

        self.set_block(Some(ifready));
        let isready = self.new_basic_block("rangefunc.resume.ready".to_string());
        let ifexit = self.new_basic_block("rangefunc.resume.exits".to_string());
        let ready = j_ready(self);
        let ready_cond = self.emit_compare(Token::EQL, v, ready, NO_POS);
        self.emit_if(ready_cond, isready, ifexit);

        self.set_block(Some(isready));
        let done_val = j_done(self);
        self.store_jump_var(jump_obj, done_val, NO_POS);
        self.emit_jump(done);

        self.set_block(Some(ifexit));
        for e in exits {
            let id = self.int_const(e.id);
            let matchb = self.new_basic_block("rangefunc.resume.match".to_string());
            let cndb = self.new_basic_block("rangefunc.resume.cnd".to_string());
            let cond = self.emit_compare(Token::EQL, v, id, e.pos);
            self.emit_if(cond, matchb, cndb);
            self.set_block(Some(matchb));

            if e.label.is_some() {
                let label = e.label.as_ref().unwrap();
                let lb = self.lblock_of(label);
                self.emit_jump(lb);
            } else if e.to != Some(self.func_id) {
                self.store_jump_var(jump_obj, id, e.pos);
                let bool_ty = self.prog.basic_type(BasicKind::Bool);
                let v_false = self
                    .prog
                    .emit_const(Some(guff_constant::Value::Bool(false)), bool_ty);
                self.emit(InstrData::Return(Return { results: vec![v_false] }));
            } else if e.block.is_none() {
                let named = self.func().named_results.clone();
                let mut results = Vec::new();
                for &nr in &named {
                    let ptr_ty = crate::program::value_type_of(self.prog, self.func(), nr);
                    let elem = guff_types::pointer::pointer_elem(&self.prog.type_arena, ptr_ty);
                    results.push(self.emit_load(nr, elem));
                }
                self.emit(InstrData::Return(Return { results }));
            } else if let Some(block) = e.block {
                self.emit_jump(block);
            }

            self.set_block(Some(cndb));
        }
    }

    pub(crate) fn store_jump_var(&mut self, jump_obj: guff_types::ObjectId, v: Value, pos: Pos) {
        let addr = self.lookup_var(jump_obj);
        self.emit_store(addr, v, pos);
    }

    fn lookup_var(&mut self, obj: guff_types::ObjectId) -> Value {
        if let Some(&v) = self.func().objects.get(&obj) {
            return v;
        }
        crate::builder::lookup(self.prog, self.func_id, obj, true)
    }

    pub(crate) fn label_exit(&mut self, label: &str, pos: Pos) -> Exit {
        let id = self.next_exit_id();
        let e = Exit {
            id,
            from: self.func_id,
            to: None,
            pos,
            block: None,
            label: Some(label.to_string()),
        };
        self.func_mut().exits.push(e.clone());
        e
    }

    pub(crate) fn block_exit(&mut self, to: FuncId, block: BlockId, pos: Pos) -> Exit {
        let id = self.next_exit_id();
        let e = Exit {
            id,
            from: self.func_id,
            to: Some(to),
            pos,
            block: Some(block),
            label: None,
        };
        self.func_mut().exits.push(e.clone());
        e
    }

    pub(crate) fn return_exit(&mut self, pos: Pos) -> Exit {
        let to = self.func().source_func;
        let id = self.next_exit_id();
        let e = Exit {
            id,
            from: self.func_id,
            to,
            pos,
            block: None,
            label: None,
        };
        self.func_mut().exits.push(e.clone());
        e
    }

    fn next_exit_id(&mut self) -> i64 {
        let source = self.func().source_func.unwrap_or(self.func_id);
        let f = self.prog.functions.get_mut(source);
        f.uniq += 1;
        f.uniq
    }
}
