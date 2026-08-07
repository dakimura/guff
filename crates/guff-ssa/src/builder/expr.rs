//! SSA Builder — Expressions.
//!
//! Port of go/ssa's `builder.go` (expr part).

use crate::builder::Builder;
use crate::value::Value;
use crate::instr::InstrData;
use guff::ast::{Expr, BasicLit, FuncLit, Ident, IndexListExpr, UnaryExpr, BinaryExpr, IndexExpr, SliceExpr, TypeAssertExpr, SelectorExpr};
use guff_types::{BasicKind, OperandMode, SelectionKind};
use std::cell::Cell;

thread_local! {
    static EXPR_DEPTH: Cell<u32> = const { Cell::new(0) };
}

impl<'a> Builder<'a> {
    /// expr translates an expression to an SSA value. When debug info is
    /// enabled it also emits a DebugRef associating `e` with the resulting
    /// value (except for constant/builtin results, which go/ssa skips).
    /// (Go: `builder.expr`)
    pub fn expr(&mut self, e: &Expr) -> Value {
        let depth = EXPR_DEPTH.with(|d| {
            let n = d.get().saturating_add(1);
            d.set(n);
            n
        });
        if depth > 512 {
            EXPR_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
            return self.invalid_zero();
        }
        let result = self.expr_inner(e);
        EXPR_DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        result
    }

    fn expr_inner(&mut self, e: &Expr) -> Value {
        // Parenthesized expressions carry no value of their own; unwrap them
        // without emitting a second DebugRef (the inner expression emits one).
        if let Expr::ParenExpr(p) = e {
            return self.expr(&p.x);
        }
        // Constant expressions (e.g. `1 + 2`, a named const, an untyped literal
        // used in constant context) are folded to a Const by the type checker;
        // go/ssa returns `NewConst` here before descending into expr0, and emits
        // no DebugRef for them. (Go: the `tv.Value != nil` check at the top of
        // `builder.expr`.)
        let mode = match self.prog.info.types.get(&e.id()) {
            Some(tv) => {
                if let Some(val) = &tv.val {
                    let val = val.clone();
                    let typ = self.typ_type(tv.typ);
                    return self.prog.emit_const(Some(val), typ);
                }
                Some(tv.mode)
            }
            None => None,
        };
        // Addressable expressions (an addressable variable, or a map-index)
        // prefer pointer arithmetic ({Index,Field}Addr) followed by Load over
        // subelement extraction (Index, Field), to avoid large copies. This is
        // what makes `a[i]` on an addressable array/slice use `&a[i]` + load and
        // `s.f` on an addressable struct use `&s.f` + load. (Go: the
        // `tv.Addressable()` branch of `builder.expr`.)
        let addressable =
            matches!(mode, Some(OperandMode::Variable) | Some(OperandMode::MapIndex));
        let v = if addressable {
            self.address(e, false).load(self)
        } else {
            self.expr0(e)
        };
        // go/ssa returns early for constant expressions before emitting a
        // DebugRef; constant and builtin results stand in for that here.
        if !matches!(v, Value::Const(_) | Value::Builtin(_)) {
            self.emit_debug_ref(e, v, false);
        }
        v
    }

    /// expr_n translates an expression that yields a tuple of two or more
    /// values (a multiple-result call, or a comma-ok map lookup, type assertion,
    /// or channel receive). The resulting value has the tuple type, and callers
    /// project out its elements with [`crate::emit::emit_extract`]. (Go:
    /// `builder.exprN`.)
    ///
    /// The multiple-result `CallExpr` case (its recorded type is already the
    /// result tuple, so this is just [`emit_call`]) and the three comma-ok forms
    /// are handled. Each comma-ok form's recorded type is the 2-tuple `(v, ok)`
    /// that the type checker installed for the expression.
    ///
    /// DEFERRED vs go/ssa: the `IndexExpr` map-key conversion uses
    /// [`emit_type_coercion`](crate::emit::emit_type_coercion) (ChangeType or
    /// pass-through) in place of go's fuller `emitConv`; the two coincide when
    /// the index already has the key type (the common case).
    pub(crate) fn expr_n(&mut self, e: &Expr) -> Value {
        let inner = crate::builder::unparen(e);
        // The checker records the comma-ok expression's type as a 2-tuple.
        let typ = self.type_of(inner.id());
        match inner {
            Expr::CallExpr(call) => self.emit_call(call),
            Expr::IndexExpr(idx) => {
                // comma-ok map lookup `v, ok := m[k]`. (Go: exprN IndexExpr.)
                let x = self.expr(&idx.x);
                let map_ty = crate::program::value_type_of(self.prog, self.func(), x)
                    .underlying(&self.prog.type_arena);
                let key_ty = guff_types::map_key(&self.prog.type_arena, map_ty);
                let index_raw = self.expr(&idx.index);
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                let index =
                    crate::emit::emit_type_coercion(self.prog, fid, block, index_raw, key_ty);
                let id = crate::emit::emit_with_pos(
                    self.func_mut(),
                    block,
                    InstrData::Lookup(crate::instr::Lookup {
                        x,
                        index,
                        comma_ok: true,
                        typ,
                    }),
                    idx.lbrack,
                );
                Value::Instr(id)
            }
            Expr::UnaryExpr(un) => {
                // comma-ok channel receive `v, ok := <-ch`. (Go: exprN UnaryExpr,
                // "must be receive <-".)
                let x = self.expr(&un.x);
                let block = self.block.expect("no current block");
                let id = crate::emit::emit_with_pos(
                    self.func_mut(),
                    block,
                    InstrData::UnOp(crate::instr::UnOp {
                        op: guff::token::Token::ARROW,
                        x,
                        comma_ok: true,
                        typ,
                    }),
                    un.op_pos,
                );
                Value::Instr(id)
            }
            Expr::TypeAssertExpr(ta) => {
                // comma-ok type assertion `v, ok := x.(T)`. (Go: exprN
                // TypeAssertExpr, via emitTypeTest with the tuple's value type.)
                // Incomplete hybrid info may leave `typ` as Typ[Invalid] rather
                // than a 2-tuple — soft-fail with a placeholder.
                let Some(value_ty) = (match self.prog.type_arena.get(typ) {
                    guff_types::arena::TypeData::Tuple(_) => guff_types::tuple_at(
                        &self.prog.type_arena,
                        typ,
                        0,
                    )
                    .typ(&self.prog.object_arena),
                    _ => None,
                }) else {
                    return self.invalid_zero();
                };
                let x = self.expr(&ta.x);
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                crate::emit::emit_type_test(self.prog, fid, block, x, value_ty, ta.lparen)
            }
            other => todo!("exprN form: {:?}", other),
        }
    }

    /// expr0 is the dispatch core of [`expr`], without DebugRef emission.
    /// (Go: `builder.expr0`)
    fn expr0(&mut self, e: &Expr) -> Value {
        match e {
            Expr::BasicLit(lit) => self.basic_lit(lit),
            Expr::Ident(id) => self.ident_rvalue(id),
            Expr::UnaryExpr(un) => self.unary_expr(un),
            Expr::BinaryExpr(bin) => self.binary_expr(bin),
            Expr::CallExpr(call) => self.emit_call(call),
            Expr::IndexExpr(idx) => {
                // `f[T]` may be a one-argument generic instantiation rather than
                // an index. (Go: IndexExpr case of expr0, `instance` guard.)
                if crate::builder::is_instance(&self.prog.info, &idx.x) {
                    self.expr(&idx.x)
                } else {
                    self.index_expr(e, idx)
                }
            }
            // `f[X, Y]` is always a generic function instantiation; peel and let
            // Ident/SelectorExpr perform the instance lookup. (Go: IndexListExpr
            // case of expr0.) Soft-fail without Instances: still resolve via X.
            Expr::IndexListExpr(IndexListExpr { x, .. }) => self.expr(x),
            Expr::SliceExpr(sl) => self.slice_expr(sl),
            Expr::TypeAssertExpr(ta) => self.type_assert_expr(ta),
            Expr::FuncLit(fl) => self.func_lit(fl),
            Expr::SelectorExpr(se) => self.selector_expr(se),
            // An addressable-type composite literal: build it in fresh storage
            // and load the aggregate. (Go: expr0's `*ast.CompositeLit` case.)
            Expr::CompositeLit(_) => self.address(e, false).load(self),
            Expr::StarExpr(star) => {
                let x = self.expr(&star.x);
                let typ = self.type_of(star.id);
                self.emit_load(x, typ)
            }
            // Type expressions must not reach value lowering (they belong in
            // `T(x)` / `make` paths). Incomplete hybrid type info can still
            // route them here — prefer a placeholder over aborting the build.
            Expr::ArrayType(_)
            | Expr::StructType(_)
            | Expr::FuncType(_)
            | Expr::InterfaceType(_)
            | Expr::MapType(_)
            | Expr::ChanType(_) => self.invalid_zero(),
            // DEFERRED: other expr forms.
            _ => todo!("unimplemented expr: {:?}", e),
        }
    }

    /// func_lit translates a function literal. It creates an anonymous
    /// [`crate::function::Function`] enclosed by the current function, records it
    /// in the parent's `anon_funcs`, and builds its body immediately (go/ssa
    /// builds the literal eagerly because it may cause the parent's locals to
    /// escape). (Go: the `*ast.FuncLit` case of `builder.expr0`.)
    ///
    /// A literal that captures no enclosing variable evaluates directly to the
    /// function value (go/ssa returns the bare `*Function` when
    /// `anon.FreeVars == nil`). Otherwise a `MakeClosure` is emitted, binding
    /// each captured `outer` value. (Go: the `*ast.FuncLit` case of
    /// `builder.expr0`.)
    fn func_lit(&mut self, fl: &FuncLit) -> Value {
        let parent_fid = self.func_id;
        let (anon_name, pkg) = {
            let parent = self.func();
            (
                format!("{}${}", parent.name, 1 + parent.anon_funcs.len()),
                parent.pkg,
            )
        };
        // The literal's recorded type is its *Signature (Go: `fn.typeOf(e.Type)`).
        let raw_sig = self
            .prog
            .info
            .types
            .get(&fl.id)
            .map(|tv| tv.typ);
        let sig = raw_sig.map(|t| self.typ_type(t));

        let anon_fid = crate::create::create_function(self.prog, anon_name, Some(parent_fid), pkg);
        self.prog.functions.get_mut(anon_fid).signature = sig;
        self.prog.functions.get_mut(parent_fid).anon_funcs.push(anon_fid);

        // Build the anonymous function eagerly (re-borrows the program; the
        // parent Builder's current block is preserved in `self.block`). Its body
        // may capture enclosing variables, populating `anon.freevars`.
        crate::builder::build_syntactic_body(self.prog, anon_fid, None, Some(&fl.body));

        // No captures: the literal is just the function value.
        if self.prog.functions.get(anon_fid).freevars.is_empty() {
            return Value::Function(anon_fid);
        }

        // Capturing literal: emit `make closure anon [bindings...]`, one binding
        // per free variable, in declaration order.
        let bindings: Vec<Value> = self
            .prog
            .functions
            .get(anon_fid)
            .freevars
            .iter()
            .map(|(_, fv)| fv.outer)
            .collect();
        // A literal with no recorded type means the checker left this node
        // untyped (hybrid source-checking on a package with errors). Fall back
        // to Invalid rather than aborting the build, as `type_of` does — a
        // panic here unwinds a worker and takes the package's findings with it.
        let typ = sig.unwrap_or_else(|| self.prog.basic_type(BasicKind::Invalid));
        let block = self.block.expect("no current block");
        let iid = crate::emit::emit(
            self.func_mut(),
            block,
            InstrData::MakeClosure(crate::instr::MakeClosure {
                fn_: anon_fid,
                bindings,
                typ,
            }),
        );
        Value::Instr(iid)
    }

    pub(crate) fn basic_lit(&mut self, lit: &BasicLit) -> Value {
        let typ = self.type_of(lit.id);
        let val = self
            .prog
            .info
            .types
            .get(&lit.id)
            .map(|tv| tv.val.clone())
            .unwrap_or_default();
        self.prog.emit_const(val, typ)
    }

    /// ident_rvalue reads the *value* of an identifier. Variables live in
    /// memory (spilled parameters, locals, package-level vars, and captured
    /// free variables are all addresses), so reading one emits a load; functions,
    /// builtins, constants, and unspilled anonymous parameters are already
    /// values and are returned directly. (Go: the `*ast.Ident` case of
    /// `builder.expr0`, which emits `emitLoad(fn.lookup(obj))` for variables.)
    fn ident_rvalue(&mut self, id: &Ident) -> Value {
        let v = self.ident(id);
        match v {
            Value::Instr(_) | Value::Global(_) | Value::FreeVar(_) => {
                let typ = self.type_of(id.id);
                self.emit_load(v, typ)
            }
            _ => v,
        }
    }

    /// ident resolves an identifier to the SSA entity that *represents* its
    /// object: the address for variables (locals, spilled params, globals,
    /// captured free vars), or the value itself for functions, builtins,
    /// constants, and unspilled anonymous parameters. Used directly as an lvalue
    /// (via [`Builder::address`]); rvalue reads go through [`ident_rvalue`].
    pub(crate) fn ident(&mut self, id: &Ident) -> Value {
        // Prefer a definition (short var decl `:=` LHS) over a use, matching
        // go's `fn.objectOf`. A freshly defined local has already had its cell
        // created (emit_local_var) and recorded in `objects`.
        let Some(obj_id) = self.object_of(id) else {
            return self.invalid_zero();
        };
        
        // 1. Check local objects (params, freevars, locals)
        if let Some(&v) = self.func().objects.get(&obj_id) {
            return v;
        }

        // 2. Constants, Nil, Builtins
        use guff_types::ObjectData;
        match self.prog.object_arena.get(obj_id) {
            ObjectData::Const(c) => return self.prog.emit_const(Some(c.val().clone()), c.typ()),
            ObjectData::Nil(n) => return self.prog.emit_const(None, n.typ()),
            ObjectData::Builtin(b) => {
                // TODO: deduplicate builtins in Program
                let b_ssa = crate::program::Builtin {
                    name: b.name().to_string(),
                    typ: b.typ(),
                };
                let id = self.prog.builtins.alloc(b_ssa);
                return Value::Builtin(id);
            },
            _ => {}
        }
        
        // 3. Check package-level objects (Globals, Functions)
        if obj_id.pkg(&self.prog.object_arena).is_some() {
            if crate::create::is_package_level_object(self.prog, obj_id) {
                if let Some(v) = crate::create::ensure_package_member(self.prog, obj_id) {
                    return self.maybe_instantiate_generic_func(id, v);
                }
            }
        }

        // 4. A variable that is neither local nor package-level must be defined
        // in an enclosing function: capture it as a free variable. (Go: the
        // addressable-ident path routes through `fn.lookup`.)
        if matches!(self.prog.object_arena.get(obj_id), ObjectData::Var(_)) {
            return crate::builder::lookup(self.prog, self.func_id, obj_id, false);
        }

        // Unresolved identifier (incomplete hybrid info) — placeholder value.
        self.invalid_zero()
    }

    fn unary_expr(&mut self, un: &UnaryExpr) -> Value {
        use guff::token::Token;
        match un.op {
            Token::AND => {
                // Address-of `&X` — potentially escaping, so its address may
                // outlive this activation. (Go: `b.addr(fn, e.X, true)`.)
                let addr = self.address(&un.x, true);
                // `&*p` must panic if `p` is nil; rely on a load's side effect
                // rather than a dedicated nil check. (Go: the StarExpr check.)
                if matches!(crate::builder::unparen(&un.x), Expr::StarExpr(_)) {
                    addr.load(self);
                }
                return addr.address(self);
            }
            Token::MUL => {
                // pointer dereference (load)
                let x = self.expr(&un.x);
                let typ = self.type_of(un.id);
                return self.emit_load(x, typ);
            }
            _ => {}
        }

        let x = self.expr(&un.x);
        let typ = self.type_of(un.id);

        match un.op {
            Token::ADD => return x, // unary + is a no-op
            _ => {}
        }

        let block = self.block.expect("no current block");
        let id = crate::emit::emit(self.func_mut(), block, InstrData::UnOp(crate::instr::UnOp {
            op: un.op,
            x,
            comma_ok: false,
            typ,
        }));
        Value::Instr(id)
    }

    fn binary_expr(&mut self, bin: &BinaryExpr) -> Value {
        let x = self.expr(&bin.x);
        let y = self.expr(&bin.y);
        let raw_typ = self.prog.info.types.get(&bin.id).map(|tv| tv.typ);
        let typ = raw_typ
            .map(|t| self.typ_type(t))
            .unwrap_or_else(|| crate::program::value_type_of(self.prog, self.func(), x));
        
        let block = self.block.expect("no current block");
        let id = crate::emit::emit(self.func_mut(), block, InstrData::BinOp(crate::instr::BinOp {
            op: bin.op,
            x,
            y,
            typ,
        }));
        Value::Instr(id)
    }

    /// If `v` is a package-level generic function and `id` has recorded type
    /// arguments in [`Info::instances`](guff_types::Info::instances), return the
    /// corresponding SSA instance; otherwise return `v` unchanged. (Go: the
    /// `callee.typeparams.Len() > 0` branch of the Ident case in `expr0`.)
    fn maybe_instantiate_generic_func(&mut self, id: &Ident, v: Value) -> Value {
        let Value::Function(fid) = v else {
            return v;
        };
        crate::builder::record_generic_params(self.prog, fid);
        if self.prog.functions.get(fid).type_params.is_empty() {
            return v;
        }
        let raw = crate::builder::instance_args(&self.prog.info, id.id);
        if raw.is_empty() {
            return v;
        }
        let targs: Vec<_> = raw.iter().map(|&t| self.typ_type(t)).collect();
        Value::Function(self.prog.instance(fid, &[], &targs))
    }

    /// index_expr translates a non-addressable index expression `x[i]` used as
    /// an rvalue: an array held in a register (`Index`), a string (`Index`
    /// yielding a byte), or a map (`Lookup`). Addressable slices/arrays and map
    /// indices are routed through [`address`](Builder::address)`.load()` by the
    /// caller's addressability dispatch, so those modes are handled defensively.
    /// (Go: the `*ast.IndexExpr` case of `builder.expr0`.)
    ///
    /// DEFERRED vs go/ssa: the untyped-index → int conversion (a constant index
    /// retypes without emitting an instruction).
    fn index_expr(&mut self, e: &Expr, ie: &IndexExpr) -> Value {
        use crate::typeset::{index_type, IndexMode};
        let xt = self.type_of(ie.x.id());
        let (elem, mode) = index_type(
            &mut self.prog.type_arena,
            &self.prog.object_arena,
            &self.prog.package_arena,
            xt,
        );
        let Some(elem) = elem else {
            // Incomplete hybrid info / mis-routed generic instantiation — prefer
            // a placeholder over aborting the SSA build for the whole package.
            return self.invalid_zero();
        };
        match mode {
            // Addressable slice/array: prefer IndexAddr + Load (reached only if
            // the checker's mode disagrees with the addressability dispatch).
            IndexMode::Var => self.address(e, false).load(self),
            // Array in a register (ixArrVar) or string (ixValue): Index.
            IndexMode::ArrVar | IndexMode::Value => {
                let index = self.expr(&ie.index);
                let x = self.expr(&ie.x);
                let block = self.block.expect("no current block");
                let id = crate::emit::emit_with_pos(
                    self.func_mut(),
                    block,
                    InstrData::Index(crate::instr::Index { x, index, typ: elem }),
                    ie.lbrack,
                );
                Value::Instr(id)
            }
            // Map read (single value): Lookup with the key converted to the
            // map's key type.
            IndexMode::Map => {
                let u = xt.underlying(&self.prog.type_arena);
                let key = guff_types::map_key(&self.prog.type_arena, u);
                let x = self.expr(&ie.x);
                let k_raw = self.expr(&ie.index);
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                let index = crate::emit::emit_type_coercion(self.prog, fid, block, k_raw, key);
                let id = crate::emit::emit_with_pos(
                    self.func_mut(),
                    block,
                    InstrData::Lookup(crate::instr::Lookup {
                        x,
                        index,
                        comma_ok: false,
                        typ: elem,
                    }),
                    ie.lbrack,
                );
                Value::Instr(id)
            }
            IndexMode::Invalid => self.invalid_zero(),
        }
    }

    fn slice_expr(&mut self, e: &SliceExpr) -> Value {
        let x = self.expr(&e.x);
        let low = e.low.as_ref().map(|l| self.expr(l));
        let high = e.high.as_ref().map(|h| self.expr(h));
        let max = e.max.as_ref().map(|m| self.expr(m));

        let typ = self.type_of(e.id);
        let block = self.block.expect("no current block");
        // Match IndexExpr: record `[` so analyzers (e.g. gosec G602) can report
        // slice-bounds findings at the slice expression site.
        let id = crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::Slice(crate::instr::Slice {
                x,
                low,
                high,
                max,
                typ,
            }),
            e.lbrack,
        );
        Value::Instr(id)
    }

    fn type_assert_expr(&mut self, e: &TypeAssertExpr) -> Value {
        let x = self.expr(&e.x);
        let assert_type = if let Some(ty_expr) = &e.ty {
            self.type_of(ty_expr.id())
        } else {
            // Type-switch `x.(type)` — not modeled yet; use Invalid so hybrid
            // incomplete info does not abort the SSA build.
            self.prog.basic_type(BasicKind::Invalid)
        };

        let block = self.block.expect("no current block");
        let id = crate::emit::emit(self.func_mut(), block, InstrData::TypeAssert(crate::instr::TypeAssert {
            x,
            assert_type,
            comma_ok: false,
            // single-value form: the result type is the asserted type
            typ: assert_type,
        }));
        Value::Instr(id)
    }

    /// selector_expr translates a selector `x.f` used as an rvalue.
    /// (Go: the `*ast.SelectorExpr` case of `builder.expr0`.)
    ///
    /// A selector with no recorded [`guff_types::Selection`] is a qualified
    /// identifier (`pkg.Name`); it resolves like a bare identifier use of `sel`.
    /// Otherwise the selection kind drives the translation. Only the field case
    /// (`FieldVal`) is implemented: the implicit embedded-field chain is emitted
    /// followed by the final explicit field, as a value (`Field`) or a load
    /// through a pointer (`FieldAddr` + load).
    ///
    /// DEFERRED vs go/ssa: `MethodVal` (bound-method closures) and `MethodExpr`
    /// (method-expression thunks) need methods.rs/wrappers.rs; the lvalue path
    /// (`x.f = …`, `&x.f`) is added in a follow-up (go: `builder.addr`).
    fn selector_expr(&mut self, e: &SelectorExpr) -> Value {
        let sel = match self.prog.info.selections.get(&e.id) {
            None => {
                // Qualified identifier: resolve `pkg.Name` as an ident use.
                return self.ident_rvalue(&e.sel);
            }
            Some(sel) => sel.clone(),
        };
        match sel.kind() {
            SelectionKind::FieldVal => {
                if selector_on_pkg_name(self, &e.x) {
                    return self.ident_rvalue(&e.sel);
                }
                let indices: Vec<usize> = sel.index().iter().map(|&i| i as usize).collect();
                let last = indices.len() - 1;
                let pos = e.sel.name_pos;
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                let mut v = self.expr(&e.x);
                v = crate::emit::emit_implicit_selections(
                    self.prog, fid, block, v, &indices[..last], pos,
                );
                crate::emit::emit_field_selection(
                    self.prog, fid, block, v, indices[last], false, pos,
                )
            }
            SelectionKind::MethodVal => {
                let obj = sel.obj();
                let Some(rt) = crate::methods::recv_type(self.prog, obj) else {
                    // Selection tagged MethodVal but object has no receiver
                    // (typechecker quirk / non-method Func). Resolve as a
                    // package-level or local ident of the same name.
                    return self.ident_rvalue(&e.sel);
                };
                let want_addr = guff_types::is_pointer(&self.prog.type_arena, rt);
                let v = self.receiver(&e.x, want_addr, true, &sel);
                let bound = crate::wrappers::create_bound(self.prog, obj, &[]);
                self.prog.build_bound(bound);
                let bound_sig = self.prog.functions.get(bound).signature.expect("bound sig");
                let fv_typ = self
                    .prog
                    .functions
                    .get(bound)
                    .freevars
                    .iter()
                    .next()
                    .map(|(_, fv)| fv.typ)
                    .expect("bound has recv freevar");
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                let v = crate::emit::emit_type_coercion(self.prog, fid, block, v, fv_typ);
                let id = crate::emit::emit(
                    self.func_mut(),
                    block,
                    crate::instr::InstrData::MakeClosure(crate::instr::MakeClosure {
                        fn_: bound,
                        bindings: vec![v],
                        typ: bound_sig,
                    }),
                );
                Value::Instr(id)
            }
            SelectionKind::MethodExpr => {
                let ws = crate::wrappers::WrapperSelection::from_selection(self.prog, &sel);
                let thunk = crate::wrappers::create_thunk(self.prog, &ws, &[]);
                self.prog.build_wrapper(thunk);
                Value::Function(thunk)
            }
        }
    }

    /// receiver evaluates the receiver expression `e` of a selection `sel` and
    /// walks its implicit embedded-field chain, returning the value or address
    /// on which the final (explicit) selection or method call operates.
    ///
    /// When `want_addr` is set and `e` is an addressable non-pointer that the
    /// path reaches without indirection, its *address* is taken (so a field can
    /// be assigned or its address returned); otherwise its value is used. An
    /// interface receiver is never loaded (the method is invoked on it); a
    /// pointer-core value receiver is loaded when its value (not address) is
    /// wanted. (Go: `builder.receiver`.)
    ///
    /// DEFERRED vs go/ssa: `is_pointer` (underlying-is-pointer) stands in for
    /// go's `isPointerCore` (core-type-is-pointer); they coincide for concrete
    /// types, differing only for type parameters, which the builder does not
    /// yet reach.
    pub(crate) fn receiver(
        &mut self,
        e: &Expr,
        want_addr: bool,
        escaping: bool,
        sel: &guff_types::Selection,
    ) -> Value {
        let e_ty = self.type_of(e.id());
        let mut v = if want_addr
            && !sel.indirect()
            && !guff_types::is_pointer(&self.prog.type_arena, e_ty)
        {
            self.address(e, escaping).address(self)
        } else {
            self.expr(e)
        };

        // Emit the implicit selection of embedded fields, up to (but excluding)
        // the last, explicit selection index.
        let last = sel.index().len() - 1;
        let indices: Vec<usize> = sel.index()[..last].iter().map(|&i| i as usize).collect();
        let pos = e.pos();
        let fid = self.func_id;
        let block = self.block.expect("no current block");
        v = crate::emit::emit_implicit_selections(self.prog, fid, block, v, &indices, pos);

        let vt = crate::program::value_type_of(self.prog, self.func(), v);
        if guff_types::is_interface(&self.prog.type_arena, vt) {
            // Interface receiver: kept as-is (the method is invoked on it),
            // even if it has a pointer core type.
        } else if !want_addr && guff_types::is_pointer(&self.prog.type_arena, vt) {
            let pointee = guff_types::pointer_elem(&self.prog.type_arena, vt);
            v = self.emit_load(v, pointee);
        }
        v
    }
}

fn selector_on_pkg_name(b: &Builder<'_>, x: &Expr) -> bool {
    let Expr::Ident(id) = crate::builder::unparen(x) else {
        return false;
    };
    let Some(obj) = b.object_of(id) else {
        return false;
    };
    matches!(
        b.prog.object_arena.get(obj),
        guff_types::ObjectData::PkgName(_)
    )
}
