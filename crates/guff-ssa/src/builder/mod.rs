//! SSA Builder.
//!
//! Port of go/ssa's `builder.go`.

use crate::program::Program;
use crate::ids::{BlockId, FuncId, InstrId, PackageId};
use crate::instr::InstrData;
use crate::mode::BuilderMode;
use crate::value::Value;
use crate::block::BasicBlock;
use crate::lvalue::{LValue, Address};
use guff_types::{signature_type_params, BasicKind, ObjectId, TypeId};
use guff::ast::{Decl, Expr, File, FuncDecl, Ident, Spec, Stmt};
use guff::{Pos, NO_POS};

pub mod expr;
pub mod stmt;
pub mod call;
pub mod cond;
pub mod labels;
pub mod range_func;

/// build_function builds the SSA body of `fid` from its syntax declaration
/// `fd`, then runs the post-construction passes. It is the sequential analog of
/// go/ssa's `(*builder).buildFromSyntax`:
///   1. create the receiver/parameters from the signature ([`crate::create::create_params`]);
///   2. open the `entry` block and translate the function body;
///   3. [`Program::finish_function`] (block optimization, dominators, lifting,
///      register numbering).
///
/// If `Function.signature` is not yet set it is filled from the declaring
/// object (`Info.defs[fd.name]`). A function with no body (`fd.body == None`,
/// i.e. an external/asm function) is left with no blocks.
///
/// DEFERRED vs go/ssa: named-result locals, the defer stack, `recover`
/// handling, and anonymous-function (`FuncLit`) descent — see the build-phase
/// deferrals in the migration plan.
pub fn build_function(prog: &mut Program, fid: FuncId, fd: &FuncDecl) {
    // Ensure the signature is recorded (createFunction leaves it None for
    // hand-built functions; populate_package_members sets it).
    if prog.functions.get(fid).signature.is_none() {
        let sig = prog
            .info
            .defs
            .get(&fd.name.id)
            .copied()
            .flatten()
            .and_then(|o| o.typ(&prog.object_arena));
        prog.functions.get_mut(fid).signature = sig;
    }

    {
        let f = prog.functions.get_mut(fid);
        f.from_syntax = fd.body.is_some();
        f.syntax_decl = Some(fd.clone());
    }
    record_generic_params(prog, fid);

    build_syntactic_body(prog, fid, Some(fd), fd.body.as_ref());
}

/// Copies a function's receiver and type parameter lists from its signature onto
/// the `Function` record (shared with instances). (Go: `createFunction` sets
/// `recvtypeparams` / `typeparams`.)
pub(crate) fn record_generic_params(prog: &mut Program, fid: FuncId) {
    let sig = match prog.functions.get(fid).signature {
        Some(s) => s,
        None => return,
    };
    let rtparams = guff_types::signature::signature_recv_type_params(&prog.type_arena, sig)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    let tparams = signature_type_params(&prog.type_arena, sig)
        .map(|l| l.list().to_vec())
        .unwrap_or_default();
    let f = prog.functions.get_mut(fid);
    f.recv_type_params = rtparams;
    f.type_params = tparams;
}

/// build_syntactic_body creates a function's parameters and, if it has a body,
/// translates that body into SSA and runs the post-construction passes. It is
/// the shared core of both [`build_function`] (for `FuncDecl`s) and the
/// anonymous-function path ([`Builder::func_lit`], for `FuncLit`s), analogous to
/// the body of go/ssa's `(*builder).buildFromSyntax`.
///
/// `Function.signature` must already be set (so [`crate::create::create_params`]
/// can build the parameters). A `None` body (external/asm function) yields a
/// parameterized function with no basic blocks.
///
/// When `fd` is `Some`, parameters are bound from the declaration syntax via
/// [`crate::create::create_syntactic_params_from_decl`] (go:
/// `createSyntacticParams`); otherwise they come from the signature tuple
/// ([`crate::create::create_syntactic_params`], used for `FuncLit`s).
pub(crate) fn build_syntactic_body(
    prog: &mut Program,
    fid: FuncId,
    fd: Option<&FuncDecl>,
    body: Option<&guff::ast::BlockStmt>,
) {
    let body = match body {
        // External function (no body): create params by value, no entry block.
        None => {
            if let Some(fd) = fd {
                // Params-only external with syntax: still need param cells if we
                // ever model asm bodies; for now fall through to signature path.
                let _ = fd;
            }
            crate::create::create_params(prog, fid);
            return;
        }
        Some(b) => b.clone(),
    };

    // go/ssa order: startBody (create the entry block) → createSyntacticParams
    // (spill named params into it) → build the body.
    let entry = {
        let mut builder = Builder::new(prog, fid);
        let e = builder.new_basic_block("entry".to_string());
        builder.set_block(Some(e));
        e
    };
    if let Some(fd) = fd {
        crate::create::create_syntactic_params_from_decl(prog, fid, entry, fd);
    } else {
        crate::create::create_syntactic_params(prog, fid, entry);
    }

    let mut builder = Builder::new(prog, fid);
    builder.set_block(Some(entry));
    builder.stmt(&Stmt::BlockStmt(body));
    // Control fell off the end of the function body. (go/ssa buildFromSyntax)
    if let Some(block) = builder.block {
        let func = builder.func();
        let b = func.blocks.get(block);
        if block == entry || !b.preds.is_empty() {
            builder.emit(InstrData::Return(crate::instr::Return {
                results: Vec::new(),
            }));
        }
    }
    drop(builder);

    prog.functions.get_mut(fid).source_func = Some(fid);
    prog.finish_function(fid);
}

/// build_package_init builds the body of a package's synthesized `init`
/// function (allocated by [`crate::create::create_package`]). It is the
/// sequential analog of go/ssa's `(*builder).buildPackageInit`:
///
///   1. guard the initializer against re-entry with `init$guard` (skipped under
///      `BARE_INITS`): load the flag, branch to `init.done` if already set,
///      otherwise set it in `init.start` and continue;
///   2. initialize package-level variables in dependency order (`Info.init_order`);
///   3. call each declared `init` function in source order;
///   4. jump to `init.done` and `return`, then run the post-construction passes.
///
/// DEFERRED vs go/ssa: calling the `init` of imported packages (needs import
/// resolution and prerequisite SSA packages), and the transient per-initializer
/// goversion switch.
pub fn build_package_init(prog: &mut Program, pkg_id: PackageId, files: &[File]) {
    let fid = prog
        .packages
        .get(pkg_id)
        .init
        .expect("create_package synthesizes the init function");
    let bare = prog.mode.contains(BuilderMode::BARE_INITS);
    let bool_ty = prog.basic_type(BasicKind::Bool);
    // The void result of an init() call; go/ssa uses an empty *types.Tuple,
    // which the disassembler renders as "()".
    let empty_tuple = guff_types::empty_tuple(&mut prog.type_arena);

    let mut b = Builder::new(prog, fid);
    let entry = b.new_basic_block("entry".to_string());
    b.set_block(Some(entry));

    let mut done: Option<BlockId> = None;
    if !bare {
        let guard = b
            .prog
            .packages
            .get(pkg_id)
            .init_guard
            .expect("init$guard exists unless BareInits");
        let guard_v = Value::Global(guard);
        let doinit = b.new_basic_block("init.start".to_string());
        let d = b.new_basic_block("init.done".to_string());
        done = Some(d);

        // if *init$guard { goto init.done } else { goto init.start }
        let loaded = b.emit_load(guard_v, bool_ty);
        b.emit_if(loaded, d, doinit);

        b.set_block(Some(doinit));
        let vtrue = b.prog.emit_const(Some(guff_constant::Value::Bool(true)), bool_ty);
        b.emit_store(guard_v, vtrue, NO_POS);
        // DEFERRED: call the init() of each imported package.
    }

    // Initialize package-level vars in dependency order.
    let init_order = b.prog.info.init_order.clone();
    for varinit in &init_order {
        let rhs = find_pkg_level_expr(files, varinit.rhs.as_u32())
            .expect("init_order rhs must resolve in package files");
        if varinit.lhs.len() == 1 {
            // 1:1 initialization: var x = a()
            let obj = varinit.lhs[0];
            let is_blank = obj.name(&b.prog.object_arena) == "_";
            let pos = rhs.pos();
            let rval = b.expr(rhs);
            if !is_blank {
                let g = *b
                    .prog
                    .packages
                    .get(pkg_id)
                    .objects
                    .get(&obj)
                    .expect("SSA Global for package-level var");
                b.emit_store(g, rval, pos);
            }
        } else {
            // n:1 initialization: var x, y = f()
            let tuple = b.expr_n(rhs);
            let block = b.block.expect("no current block");
            let pos = rhs.pos();
            for (i, &obj) in varinit.lhs.iter().enumerate() {
                if obj.name(&b.prog.object_arena) == "_" {
                    continue;
                }
                let g = *b
                    .prog
                    .packages
                    .get(pkg_id)
                    .objects
                    .get(&obj)
                    .expect("SSA Global for package-level var");
                let elem = crate::emit::emit_extract(b.prog, b.func_id, block, tuple, i);
                b.emit_store(g, elem, pos);
            }
        }
    }

    // Call all declared init() functions in source order. Collect first to keep
    // the borrow of `files`/`prog` separate from the emission below.
    let mut declared_inits: Vec<FuncId> = Vec::new();
    for file in files {
        for decl in &file.decls {
            if let Decl::FuncDecl(fd) = decl {
                if fd.name.name == "init" && fd.name.name != "_" && fd.recv.is_none() {
                    if let Some(Some(obj)) = b.prog.info.defs.get(&fd.name.id) {
                        if let Some(Value::Function(ifid)) =
                            b.prog.packages.get(pkg_id).objects.get(obj)
                        {
                            declared_inits.push(*ifid);
                        }
                    }
                }
            }
        }
    }
    for ifid in declared_inits {
        b.emit(InstrData::Call(crate::instr::Call {
            call: crate::instr::CallCommon {
                value: Value::Function(ifid),
                method: None,
                args: Vec::new(),
                ellipsis: false,
            },
            typ: empty_tuple,
        }));
    }

    // Finish up init().
    if !bare {
        let d = done.expect("done block created when not BareInits");
        b.emit_jump(d);
        b.set_block(Some(d));
    }
    b.emit(InstrData::Return(crate::instr::Return {
        results: Vec::new(),
    }));
    drop(b);

    prog.finish_function(fid);
}

/// Resolve a stamped expression id from package-level `var`/`const` specs.
/// Used for [`guff_types::Initializer::rhs`] after C-1 Phase 2 (NodeId).
fn find_pkg_level_expr(files: &[File], id: u32) -> Option<&Expr> {
    for file in files {
        for decl in &file.decls {
            let Decl::GenDecl(gd) = decl else {
                continue;
            };
            for spec in &gd.specs {
                let Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                if let Some(ty) = &vs.ty {
                    if ty.id() == id {
                        return Some(ty);
                    }
                }
                for v in &vs.values {
                    if v.id() == id {
                        return Some(v);
                    }
                }
            }
        }
    }
    None
}

/// build_package builds the SSA bodies of every function declared in package
/// `pkg_id` plus its synthesized initializer. It is the sequential analog of
/// go/ssa's `(*Package).build` driving `(*builder).iterate`: build every
/// created-but-unbuilt function of the package.
///
/// In go/ssa each created `Function` carries its own build strategy and syntax
/// (`buildFromSyntax` for declared funcs/methods, `buildPackageInit` for the
/// initializer). Here we recover the same set from the package's syntax: every
/// top-level `FuncDecl` (a package-level function or a method) drives
/// [`build_function`], and the synthesized `init` is driven by
/// [`build_package_init`]. Declared functions are built before `init` so that
/// the `init#N` functions it calls already have bodies.
///
/// Prerequisites: [`crate::create::create_package`] and
/// [`crate::create::populate_package_members`] must have run for `pkg_id`, so
/// each declared function has been created (and mapped in `Package.objects`) and
/// the initializer synthesized.
///
/// DEFERRED vs go/ssa: methods created on demand for imported packages and
/// anonymous functions (`FuncLit`) discovered during body construction. go's
/// `iterate` loops until the created set converges; here we build only the
/// functions present in the package's own syntax.
pub fn build_package(prog: &mut Program, pkg_id: PackageId, files: &[File]) {
    // Build every function declared in syntax (package-level funcs and methods).
    for file in files {
        for decl in &file.decls {
            if let Decl::FuncDecl(fd) = decl {
                let fid = prog
                    .info
                    .defs
                    .get(&fd.name.id)
                    .copied()
                    .flatten()
                    .and_then(|obj| match prog.packages.get(pkg_id).objects.get(&obj) {
                        Some(Value::Function(fid)) => Some(*fid),
                        _ => None,
                    });
                if let Some(fid) = fid {
                    build_function(prog, fid, fd);
                }
            }
        }
    }

    // Build the synthesized package initializer.
    build_package_init(prog, pkg_id, files);

    // Build any instances/wrappers created on demand during the passes above.
    prog.drain_build_queue();

    // go/ssa runs sanityCheckPackage over the whole package under
    // SanityCheckFunctions; we check the whole program's functions.
    if prog.mode.contains(BuilderMode::SANITY_CHECK_FUNCTIONS) {
        crate::sanity::sanity_check(prog);
    }
}

/// lookup returns the SSA value denoting the variable `obj` as seen from
/// function `fid`: either a value already local to `fid` (a parameter, local, or
/// previously-captured free variable) or a fresh [`crate::function::FreeVar`]
/// plumbed through the enclosing functions, creating one FreeVar in each
/// intervening function. It is the analog of go/ssa's `(*Function).lookup`.
///
/// Each created FreeVar records the enclosing value it captures in its `outer`
/// field, which [`Builder::func_lit`] later reads to build the `MakeClosure`
/// bindings.
///
/// DEFERRED vs go/ssa: the `escaping` flag (which marks a captured `Alloc`
/// heap-allocated) is accepted for parity but currently unused — our parameters
/// are not spilled to `Alloc`s, so a captured parameter is bound by value rather
/// than by reference. Local-variable capture and heap marking arrive with
/// parameter spilling (and the checker recording local-var `Info.defs`).
pub(crate) fn lookup(prog: &mut Program, fid: FuncId, obj: ObjectId, escaping: bool) -> Value {
    lookup_depth(prog, fid, obj, escaping, 0)
}

fn lookup_depth(
    prog: &mut Program,
    fid: FuncId,
    obj: ObjectId,
    escaping: bool,
    depth: u32,
) -> Value {
    if depth > 64 {
        let typ = prog.basic_type(BasicKind::Invalid);
        return prog.emit_const(None, typ);
    }
    if let Some(&v) = prog.functions.get(fid).objects.get(&obj) {
        return v; // local to fid (or already captured)
    }
    // The definition is in an enclosing function; plumb it through.
    let Some(parent) = prog.functions.get(fid).parent else {
        // No enclosing function — incomplete type/object info (seen under
        // hybrid source mode). Prefer a placeholder over aborting the build.
        let typ = prog.basic_type(BasicKind::Invalid);
        return prog.emit_const(None, typ);
    };
    let outer = lookup_depth(prog, parent, obj, true, depth + 1);
    let typ = value_type(prog, parent, outer);
    let name = obj.name(&prog.object_arena).to_string();
    let f = prog.functions.get_mut(fid);
    let fv_id = f.freevars.alloc(crate::function::FreeVar {
        name,
        typ,
        parent: fid,
        outer,
    });
    let v = Value::FreeVar(fv_id);
    f.objects.insert(obj, v);
    v
}

/// value_type returns the type of `v` interpreted in function `fid`'s
/// value-space. Used to give a captured FreeVar the type of the value it
/// captures (go: `outer.Type()`).
fn value_type(prog: &Program, fid: FuncId, v: Value) -> TypeId {
    crate::program::value_type_of(prog, prog.functions.get(fid), v)
}

/// A basic block that may belong to this function or an ancestor (range-over-func
/// yield functions store parent `break` targets). BlockIds are function-local,
/// so the owning [`FuncId`] must travel with them. (Go: `*BasicBlock` pointers
/// carry their parent Function intrinsically.)
#[derive(Debug, Clone, Copy)]
pub(crate) struct TargetBlock {
    pub func: FuncId,
    pub block: BlockId,
}

/// Break / continue / fallthrough targets for the innermost breakable statement.
/// (Go: `targets` in `builder.go`.)
#[derive(Debug, Clone, Copy)]
pub(crate) struct LoopTargets {
    pub break_: TargetBlock,
    pub continue_: Option<TargetBlock>,
    pub fallthrough_: Option<TargetBlock>,
}

/// Builder is the state used during construction of a single function's SSA IR.
pub struct Builder<'a> {
    /// The program containing the function and type information.
    pub prog: &'a mut Program,
    /// The ID of the function being built.
    pub func_id: FuncId,
    /// The current basic block to which instructions are being emitted.
    pub block: Option<BlockId>,
    /// Stack of break/continue/fallthrough targets for nested loops/switches.
    pub(crate) targets: Vec<LoopTargets>,
}

impl<'a> Builder<'a> {
    pub fn new(prog: &'a mut Program, func_id: FuncId) -> Self {
        Self {
            prog,
            func_id,
            block: None,
            targets: Vec::new(),
        }
    }

    pub(crate) fn push_targets(&mut self, break_: BlockId, continue_: BlockId) {
        let fid = self.func_id;
        self.targets.push(LoopTargets {
            break_: TargetBlock {
                func: fid,
                block: break_,
            },
            continue_: Some(TargetBlock {
                func: fid,
                block: continue_,
            }),
            fallthrough_: None,
        });
    }

    /// Like [`push_targets`], but break/continue may belong to different
    /// functions (yield body: parent `done` + local `ycont`).
    pub(crate) fn push_targets_owned(
        &mut self,
        break_: TargetBlock,
        continue_: TargetBlock,
    ) {
        self.targets.push(LoopTargets {
            break_,
            continue_: Some(continue_),
            fallthrough_: None,
        });
    }

    /// Push break (+ optional fallthrough) targets for a switch / select case.
    pub(crate) fn push_break_targets(
        &mut self,
        break_: BlockId,
        fallthrough_: Option<BlockId>,
    ) {
        let fid = self.func_id;
        self.targets.push(LoopTargets {
            break_: TargetBlock {
                func: fid,
                block: break_,
            },
            continue_: None,
            fallthrough_: fallthrough_.map(|block| TargetBlock { func: fid, block }),
        });
    }

    pub(crate) fn pop_targets(&mut self) {
        self.targets.pop();
    }

    pub(crate) fn emit_extract(&mut self, tuple: Value, index: usize) -> Value {
        let block = self.block.expect("no current block");
        crate::emit::emit_extract(self.prog, self.func_id, block, tuple, index)
    }

    /// func returns a reference to the function being built.
    pub fn func(&self) -> &crate::function::Function {
        self.prog.functions.get(self.func_id)
    }

    /// func_mut returns a mutable reference to the function being built.
    pub fn func_mut(&mut self) -> &mut crate::function::Function {
        self.prog.functions.get_mut(self.func_id)
    }

    /// Returns the locally instantiated type of `t` for the function being
    /// built. (Go: `(*Function).typ`.)
    pub(crate) fn typ_type(&mut self, t: TypeId) -> TypeId {
        self.prog.function_typ(self.func_id, t)
    }

    /// Returns the locally instantiated type recorded for AST node `id`.
    /// (Go: `(*Function).typeOf` applied to the node's checker type.)
    ///
    /// When hybrid source-checking left the node untyped, returns the Invalid
    /// basic type instead of panicking — incomplete type info must not abort
    /// the SSA builder (or the whole lint process via stack-overflow-on-unwind).
    pub(crate) fn type_of(&mut self, id: u32) -> TypeId {
        match self.prog.info.types.get(&id) {
            Some(tv) => self.typ_type(tv.typ),
            None => self.prog.basic_type(BasicKind::Invalid),
        }
    }

    /// A zero value of the Invalid basic type — used when checker info is
    /// missing and continuing with a placeholder is preferable to panicking.
    pub(crate) fn invalid_zero(&mut self) -> Value {
        let typ = self.prog.basic_type(BasicKind::Invalid);
        self.prog.emit_const(None, typ)
    }

    /// set_block sets the current basic block.
    pub fn set_block(&mut self, block: Option<BlockId>) {
        self.block = block;
    }

    /// current_block returns the current basic block.
    pub fn current_block(&self) -> Option<BlockId> {
        self.block
    }

    /// emit adds the instruction `data` to the current block.
    pub fn emit(&mut self, data: InstrData) -> InstrId {
        let block = self.block.expect("no current block");
        crate::emit::emit(self.func_mut(), block, data)
    }

    /// emit_pos adds `data` to the current block with source position `pos`.
    pub fn emit_pos(&mut self, data: InstrData, pos: Pos) -> InstrId {
        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(self.func_mut(), block, data, pos)
    }

    /// emit_load emits a load instruction (`*addr`) to the current block.
    pub fn emit_load(&mut self, addr: Value, typ: TypeId) -> Value {
        let block = self.block.expect("no current block");
        crate::emit::emit_load(self.func_mut(), block, addr, typ)
    }

    /// emit_store emits a store instruction (`*addr = val`) to the current
    /// block at source position `pos`.
    pub fn emit_store(&mut self, addr: Value, val: Value, pos: Pos) {
        let block = self.block.expect("no current block");
        crate::emit::emit_store(self.func_mut(), block, addr, val, pos);
    }

    /// new_basic_block creates a new basic block and adds it to the function.
    pub fn new_basic_block(&mut self, comment: String) -> BlockId {
        let func_id = self.func_id;
        let func = self.func_mut();
        let index = func.blocks.len() as i32;
        let mut b = BasicBlock::new(index, func_id);
        b.comment = comment;
        func.blocks.alloc(b)
    }

    /// add_edge adds a control-flow edge from `from` to `to`.
    pub fn add_edge(&mut self, from: BlockId, to: BlockId) {
        let func = self.func_mut();
        func.blocks.get_mut(from).succs.push(to);
        func.blocks.get_mut(to).preds.push(from);
    }

    /// emit_jump emits a jump instruction to `target`.
    pub fn emit_jump(&mut self, target: BlockId) {
        let block = self.block.expect("no current block");
        self.emit(InstrData::Jump(crate::instr::Jump {}));
        self.add_edge(block, target);
        self.block = None;
    }

    /// emit_if emits an if instruction with `cond`, `t_block`, and `f_block`.
    pub fn emit_if(&mut self, cond: Value, t_block: BlockId, f_block: BlockId) {
        let block = self.block.expect("no current block");
        self.emit(InstrData::If(crate::instr::If { cond }));
        self.add_edge(block, t_block);
        self.add_edge(block, f_block);
        self.block = None;
    }

    /// debug_info reports whether debug info (DebugRef pseudo-instructions) is
    /// wanted for the function being built. It follows the declaring package's
    /// `debug` flag, walking up to the enclosing package for anonymous
    /// functions. (Go: `Function.debugInfo` via `declaredPackage`)
    pub fn debug_info(&self) -> bool {
        let mut fid = self.func_id;
        loop {
            let f = self.prog.functions.get(fid);
            if let Some(pkg) = f.pkg {
                return self.prog.packages.get(pkg).debug;
            }
            match f.parent {
                Some(parent) => fid = parent,
                None => return false,
            }
        }
    }

    /// object_of returns the type-checker object denoted by an identifier,
    /// preferring a definition (`Info.defs`) over a use (`Info.uses`).
    /// (Go: `Function.objectOf`)
    fn object_of(&self, id: &Ident) -> Option<ObjectId> {
        if let Some(Some(obj)) = self.prog.info.defs.get(&id.id) {
            return Some(*obj);
        }
        self.prog.info.uses.get(&id.id).copied()
    }

    /// emit_debug_ref emits a DebugRef pseudo-instruction associating the
    /// source expression `e` with the SSA value (or address) `v`. It is a no-op
    /// unless debug info is enabled for this function. DebugRefs are not emitted
    /// for blank identifiers, for identifiers denoting constants / nil /
    /// builtins, or for parenthesized expressions (which are unwrapped first).
    /// (Go: `emitDebugRef`)
    pub fn emit_debug_ref(&mut self, e: &Expr, v: Value, is_addr: bool) {
        if !self.debug_info() {
            return;
        }
        let e = unparen(e);
        let mut object = None;
        if let Expr::Ident(id) = e {
            if id.name == "_" {
                return; // blank identifier
            }
            object = self.object_of(id);
            if let Some(obj) = object {
                use guff_types::ObjectData;
                match self.prog.object_arena.get(obj) {
                    // trivial and numerous — skip, as go/ssa does.
                    ObjectData::Nil(_) | ObjectData::Const(_) | ObjectData::Builtin(_) => return,
                    _ => {}
                }
            }
        }
        let expr_descr = expr_reflect_name(e).to_string();
        let expr_id = e.id();
        let pos = e.pos();
        let block = self.block.expect("no current block");
        crate::emit::emit_with_pos(
            self.func_mut(),
            block,
            InstrData::DebugRef(crate::instr::DebugRef {
                x: v,
                is_addr,
                object,
                expr_id,
                expr_descr,
            }),
            pos,
        );
    }

    /// address translates an expression to an lvalue (an assignable location).
    /// `escaping` reports whether the address may outlive the current function
    /// activation (e.g. it is the result of `&e`); it selects heap vs. stack
    /// allocation for composite literals and marks captured cells as escaping.
    /// (Go: `builder.addr`.)
    ///
    /// DEFERRED vs go/ssa: `CompositeLit`, `IndexExpr`, and `StarExpr` lvalues.
    pub fn address(&mut self, e: &Expr, escaping: bool) -> Box<dyn LValue> {
        match e {
            Expr::Ident(id) => {
                let v = self.ident(id);
                // The location's type is the pointee of the address value's
                // type. Deriving it from the address (rather than Info.types)
                // also covers a `:=`/`var` define ident, which has no recorded
                // expression type. (Go: `address.typ()` = `mustDeref(addr.Type())`,
                // which always holds because addressable idents resolve to a
                // spilled `*T` cell.) As a fallback for a resolved value that is
                // not a pointer (an unspilled parameter in some test harnesses),
                // use the recorded expression type.
                let addr_ty = crate::program::value_type_of(self.prog, self.func(), v);
                let typ = if guff_types::is_pointer(&self.prog.type_arena, addr_ty) {
                    guff_types::pointer_elem(&self.prog.type_arena, addr_ty)
                } else {
                    self.type_of(id.id)
                };
                Box::new(Address {
                    addr: v,
                    typ,
                    pos: e.pos(),
                    expr: Some(e.clone()),
                })
            }
            Expr::ParenExpr(p) => self.address(&p.x, escaping),
            Expr::SelectorExpr(se) => self.addr_selector(se, escaping),
            Expr::IndexExpr(ie) => self.addr_index(ie, escaping),
            Expr::CompositeLit(cl) => self.addr_composite_lit(cl, escaping),
            // Dereference lvalue: `*p` denotes the variable pointed to by `p`.
            // (Go: `*ast.StarExpr` case of `builder.addr`.)
            Expr::StarExpr(star) => {
                let ptr = self.expr(&star.x);
                let typ = self.type_of(star.id);
                Box::new(crate::lvalue::Address {
                    addr: ptr,
                    typ,
                    pos: star.star,
                    expr: Some(e.clone()),
                })
            }
            _ => todo!("address for {:?}", e),
        }
    }

    /// addr_selector translates a selector `x.f` used as an lvalue. A selector
    /// with no recorded [`guff_types::Selection`] is a qualified identifier
    /// (`pkg.Name`); it resolves like the address of `sel`. Otherwise only the
    /// field case (`FieldVal`) is valid as an lvalue: the receiver `x` is
    /// evaluated eagerly (its address, since a field is being addressed), and a
    /// [`crate::lvalue::LazyAddress`] defers the field-address instruction to
    /// store/load/address time. (Go: the `*ast.SelectorExpr` case of
    /// `builder.addr`.)
    fn addr_selector(
        &mut self,
        se: &guff::ast::SelectorExpr,
        escaping: bool,
    ) -> Box<dyn LValue> {
        let sel = match self.prog.info.selections.get(&se.id) {
            None => {
                // Qualified identifier: address of `pkg.Name`.
                return self.address(&Expr::Ident(se.sel.clone()), escaping);
            }
            Some(sel) => sel.clone(),
        };
        assert!(
            sel.kind() == guff_types::SelectionKind::FieldVal,
            "non-field selector used as lvalue"
        );

        let want_addr = true;
        let recv = self.receiver(&se.x, want_addr, escaping, &sel);
        let index = *sel.index().last().expect("selection has a path") as usize;

        // The receiver is an address (`*struct`); the field's type is the type
        // of the location. (Go: `fieldOf(MustDeref(v.Type()), index)`.)
        let recv_ty = crate::program::value_type_of(self.prog, self.func(), recv);
        if !guff_types::is_pointer(&self.prog.type_arena, recv_ty) {
            // Incomplete hybrid info left a non-pointer receiver — placeholder.
            let typ = self.prog.basic_type(BasicKind::Invalid);
            return Box::new(crate::lvalue::Address {
                addr: self.invalid_zero(),
                typ: guff_types::new_pointer(&mut self.prog.type_arena, typ),
                pos: se.sel.name_pos,
                expr: Some(Expr::Ident(se.sel.clone())),
            });
        }
        let pointee = guff_types::pointer_elem(&self.prog.type_arena, recv_ty);
        let Some(fld) = crate::emit::field_of(self.prog, pointee, index) else {
            // Incomplete hybrid info left a non-struct pointee — placeholder.
            let typ = self.prog.basic_type(BasicKind::Invalid);
            return Box::new(crate::lvalue::Address {
                addr: self.invalid_zero(),
                typ: guff_types::new_pointer(&mut self.prog.type_arena, typ),
                pos: se.sel.name_pos,
                expr: Some(Expr::Ident(se.sel.clone())),
            });
        };
        let fld_ty = fld
            .typ(&self.prog.object_arena)
            .expect("field has a type");

        Box::new(crate::lvalue::LazyAddress {
            recv,
            field: index,
            typ: fld_ty,
            pos: se.sel.name_pos,
            // go/ssa uses the field Ident (`e.Sel`), not the whole SelectorExpr.
            expr: Some(Expr::Ident(se.sel.clone())),
        })
    }

    /// addr_index translates an index expression `x[i]` used as an lvalue. A map
    /// index is an [`Element`](crate::lvalue::Element) (Lookup/MapUpdate; not
    /// addressable). An addressable array (`x` is an addressable variable) takes
    /// the array's address; a slice or `*array` uses its value directly; either
    /// way the element address is a deferred [`IndexAddr`](crate::instr::IndexAddr)
    /// wrapped in a [`LazyIndexAddr`](crate::lvalue::LazyIndexAddr), so that an
    /// out-of-bounds/nil panic follows evaluation of the stored value (the two
    /// phases of `AssignStmt`). (Go: the `*ast.IndexExpr` case of `builder.addr`.)
    ///
    /// DEFERRED vs go/ssa: the untyped-index → int conversion (a constant index
    /// retypes without emitting an instruction).
    fn addr_index(
        &mut self,
        ie: &guff::ast::IndexExpr,
        escaping: bool,
    ) -> Box<dyn LValue> {
        use crate::typeset::{index_type, IndexMode};
        let xt = self.type_of(ie.x.id());
        let (elem, mode) = index_type(
            &mut self.prog.type_arena,
            &self.prog.object_arena,
            &self.prog.package_arena,
            xt,
        );
        let Some(elem) = elem else {
            // Incomplete type info / non-indexable — placeholder address.
            let typ = self.prog.basic_type(BasicKind::Invalid);
            let ptr = guff_types::new_pointer(&mut self.prog.type_arena, typ);
            return Box::new(crate::lvalue::Address {
                addr: self.invalid_zero(),
                typ: ptr,
                pos: ie.lbrack,
                expr: Some(Expr::IndexExpr(ie.clone())),
            });
        };
        let pos = ie.lbrack;
        match mode {
            IndexMode::Map => {
                let u = xt.underlying(&self.prog.type_arena);
                let key = guff_types::map_key(&self.prog.type_arena, u);
                let m = self.expr(&ie.x);
                let k_raw = self.expr(&ie.index);
                let fid = self.func_id;
                let block = self.block.expect("no current block");
                let k = crate::emit::emit_type_coercion(self.prog, fid, block, k_raw, key);
                Box::new(crate::lvalue::Element { m, k, typ: elem, pos })
            }
            IndexMode::ArrVar => {
                // Array in an addressable variable: take the array's address.
                let x = self.address(&ie.x, escaping).address(self);
                let et = guff_types::new_pointer(&mut self.prog.type_arena, elem);
                let index = self.expr(&ie.index);
                Box::new(crate::lvalue::LazyIndexAddr {
                    x,
                    index,
                    et,
                    typ: elem,
                    pos,
                    expr: Some(Expr::IndexExpr(ie.clone())),
                })
            }
            IndexMode::Var => {
                // Slice or `*array`: the container value is already a pointer/slice.
                let x = self.expr(&ie.x);
                let et = guff_types::new_pointer(&mut self.prog.type_arena, elem);
                let index = self.expr(&ie.index);
                Box::new(crate::lvalue::LazyIndexAddr {
                    x,
                    index,
                    et,
                    typ: elem,
                    pos,
                    expr: Some(Expr::IndexExpr(ie.clone())),
                })
            }
            IndexMode::Value | IndexMode::Invalid => {
                // String indices aren't addressable; Invalid means incomplete
                // hybrid info. Prefer a placeholder over aborting the build.
                let typ = self.prog.basic_type(BasicKind::Invalid);
                let ptr = guff_types::new_pointer(&mut self.prog.type_arena, typ);
                Box::new(crate::lvalue::Address {
                    addr: self.invalid_zero(),
                    typ: ptr,
                    pos,
                    expr: Some(Expr::IndexExpr(ie.clone())),
                })
            }
        }
    }

    /// addr_composite_lit translates a composite literal `T{…}` used as an
    /// lvalue (or evaluated for its address). It allocates storage for the
    /// aggregate — on the heap (`new`) if the address may escape (`&T{…}`), else
    /// a stack local — fills it via [`comp_lit`](Builder::comp_lit) into a store
    /// buffer, flushes the buffer, and returns the aggregate's address. (Go: the
    /// `*ast.CompositeLit` case of `builder.addr`.)
    fn addr_composite_lit(
        &mut self,
        cl: &guff::ast::CompositeLit,
        escaping: bool,
    ) -> Box<dyn LValue> {
        let raw = self.type_of(cl.id);
        let raw = self.typ_type(raw);
        let typ = if guff_types::is_pointer(&self.prog.type_arena, raw) {
            guff_types::pointer_elem(&self.prog.type_arena, raw)
        } else {
            raw
        };
        let fid = self.func_id;
        let block = self.block.expect("no current block");
        let v = if escaping {
            crate::emit::emit_new(self.prog, fid, block, typ, cl.lbrace, "complit".to_string())
        } else {
            crate::emit::emit_local(self.prog, fid, block, typ, cl.lbrace, "complit".to_string())
        };
        let mut sb = crate::lvalue::StoreBuf::new();
        self.comp_lit(v, cl, true, &mut sb);
        sb.emit(self);
        Box::new(Address {
            addr: v,
            typ,
            pos: cl.lbrace,
            expr: Some(Expr::CompositeLit(cl.clone())),
        })
    }
}

/// unparen strips any enclosing parentheses from an expression.
/// (Go: `ast.Unparen`)
pub(crate) fn unparen(e: &Expr) -> &Expr {
    let mut e = e;
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

/// Reports whether `expr` is a simple or qualified identifier that denotes a
/// generic instantiation (present in [`Info::instances`](guff_types::Info::instances)).
/// (Go: `instance` in `go/ssa/util.go`.)
pub(crate) fn is_instance(info: &guff_types::Info, expr: &Expr) -> bool {
    let id = match unparen(expr) {
        Expr::Ident(id) => id.id,
        Expr::SelectorExpr(sel) => sel.sel.id,
        _ => return false,
    };
    info.instances.contains_key(&id)
}

/// Returns the type arguments recorded for the instantiated identifier `id`
/// (the `T` in `T[int]`, or the `Sel` of `pkg.T[int]`). Empty when the
/// identifier is not a recorded instance. (Go: `instanceArgs`.)
pub(crate) fn instance_args(info: &guff_types::Info, id: u32) -> Vec<TypeId> {
    info.instances
        .get(&id)
        .map(|inst| inst.type_args.clone())
        .unwrap_or_default()
}

/// expr_reflect_name returns the Go `reflect.TypeOf` name of an AST expression
/// node (e.g. `*ast.CallExpr`), matching what go/ssa's disassembler prints for
/// a DebugRef whose expression is not an identifier.
pub(crate) fn expr_reflect_name(e: &Expr) -> &'static str {
    match e {
        Expr::BadExpr(_) => "*ast.BadExpr",
        Expr::Ident(_) => "*ast.Ident",
        Expr::Ellipsis(_) => "*ast.Ellipsis",
        Expr::BasicLit(_) => "*ast.BasicLit",
        Expr::FuncLit(_) => "*ast.FuncLit",
        Expr::CompositeLit(_) => "*ast.CompositeLit",
        Expr::ParenExpr(_) => "*ast.ParenExpr",
        Expr::SelectorExpr(_) => "*ast.SelectorExpr",
        Expr::IndexExpr(_) => "*ast.IndexExpr",
        Expr::IndexListExpr(_) => "*ast.IndexListExpr",
        Expr::SliceExpr(_) => "*ast.SliceExpr",
        Expr::TypeAssertExpr(_) => "*ast.TypeAssertExpr",
        Expr::CallExpr(_) => "*ast.CallExpr",
        Expr::StarExpr(_) => "*ast.StarExpr",
        Expr::UnaryExpr(_) => "*ast.UnaryExpr",
        Expr::BinaryExpr(_) => "*ast.BinaryExpr",
        Expr::KeyValueExpr(_) => "*ast.KeyValueExpr",
        Expr::ArrayType(_) => "*ast.ArrayType",
        Expr::StructType(_) => "*ast.StructType",
        Expr::FuncType(_) => "*ast.FuncType",
        Expr::InterfaceType(_) => "*ast.InterfaceType",
        Expr::MapType(_) => "*ast.MapType",
        Expr::ChanType(_) => "*ast.ChanType",
    }
}
