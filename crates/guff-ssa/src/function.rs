//! SSA Function.

use crate::hash::HashMap;
use crate::arena::{Arena, ArenaId};
use crate::canon::CanonListId;
use crate::ids::{BlockId, FreeVarId, FuncId, InstrId, PackageId, ParamId};
use crate::block::BasicBlock;
use crate::instr::InstrData;
use crate::subst::Subster;
use crate::value::Value;
use guff::{Pos, NO_POS};
use guff_types::{ObjectId, TypeId};

/// Selects which builder routine constructs a function's body. Replaces
/// go/ssa's `Function.build` function pointer (`buildFunc`), which cannot be a
/// value in Rust. Only the strategies produced by generic instantiation are
/// modelled so far; wrapper/bound/thunk strategies land with `wrappers.rs`.
/// (Go: the `build buildFunc` field, set to `(*builder).buildXxx`.)
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum BuildStrategy {
    /// Not an instance / built through the direct build orchestration path
    /// (`build_function`) rather than a recorded strategy.
    #[default]
    Unset,
    /// Build a fully-concrete generic instance from the origin's syntax,
    /// substituting type arguments. (Go: `(*builder).buildFromSyntax`.)
    FromSyntax,
    /// Build only the parameters of an instance with no available syntax
    /// (imported generic origin). (Go: `(*builder).buildParamsOnly`.)
    ParamsOnly,
    /// Build a wrapper that coerces arguments/results and tail-calls the
    /// generic origin. (Go: `(*builder).buildInstantiationWrapper`.)
    InstantiationWrapper,
    /// Build a promotion/indirection method wrapper. (Go: `(*builder).buildWrapper`.)
    Wrapper,
    /// Build a bound-method closure target. (Go: `(*builder).buildBound`.)
    Bound,
    /// Build a synthetic yield function for range-over-func. (Go:
    /// `(*builder).buildYieldFunc`.)
    YieldFunc,
}

/// A control-flow exit from a range-over-func yield function to an ancestor
/// function (break, continue, goto, or return). (Go: `exit`)
#[derive(Debug, Clone)]
pub struct Exit {
    pub id: i64,
    pub from: FuncId,
    /// Destination function; `None` while unresolved (forward goto).
    pub to: Option<FuncId>,
    pub pos: Pos,
    pub block: Option<crate::ids::BlockId>,
    pub label: Option<String>,
}

/// Parameter represents a function parameter.
/// (Go: `Parameter`)
pub struct Parameter {
    pub name: String,
    pub typ: TypeId,
    pub parent: FuncId,
    /// the type-checker `Var` object this parameter was declared from; `None`
    /// for synthetic/hand-built parameters. Used by `Program::var_value` to
    /// match a defining identifier to its parameter. (Go: `Parameter.object`)
    pub object: Option<ObjectId>,
}

/// Destinations associated with a labelled statement. Populated as labels are
/// encountered in forward gotos or labelled statements. (Go: `lblock`)
#[derive(Debug, Clone)]
pub struct LBlock {
    pub name: String,
    /// `_goto` block was entered (back jump or resolved forward jump).
    pub resolved: bool,
    pub goto_: BlockId,
    pub break_: Option<BlockId>,
    pub continue_: Option<BlockId>,
}

/// FreeVar represents a free variable captured by a closure.
/// (Go: `FreeVar`)
pub struct FreeVar {
    pub name: String,
    pub typ: TypeId,
    pub parent: FuncId,
    /// the value captured from the enclosing function (in that function's
    /// value-space); supplies the corresponding `MakeClosure` binding. (Go:
    /// `FreeVar.outer`)
    pub outer: Value,
}

/// Function represents a Go function or method.
/// (Go: `Function`)
pub struct Function {
    pub name: String,
    /// enclosing function if anon; None if global
    pub parent: Option<FuncId>,
    /// enclosing package; None for shared funcs
    pub pkg: Option<PackageId>,
    /// the function's type (a *types.Signature); None until recorded
    pub signature: Option<TypeId>,
    /// the type-checker `Func` object this function was created from; `None` for
    /// synthetic functions (e.g. the package initializer) and hand-built test
    /// functions. Used by source-level lookups to recover a function's
    /// declaring position/object. (Go: `Function.object`)
    pub object: Option<ObjectId>,
    /// Source position of the function's declaration: the `func` token of a
    /// function literal, and the declared identifier of a named function.
    /// (Go: the `pos token.Pos` field behind `Function.Pos()`.)
    ///
    /// Only literals need it stored — a named function's position is reachable
    /// through `object`, so [`crate::program::Program::func_pos`] is what
    /// callers should ask.
    pub decl_pos: Pos,
    /// basic blocks of the function; empty => external
    pub blocks: Arena<BlockId, BasicBlock>,
    /// instructions in this function
    pub instrs: Arena<InstrId, InstrData>,
    /// parameters of this function
    pub params: Arena<ParamId, Parameter>,
    /// free variables of this function
    pub freevars: Arena<FreeVarId, FreeVar>,
    /// local variables (Alloc instructions)
    pub locals: Vec<InstrId>,
    /// the stack-local `Alloc` for each named result variable, in result order;
    /// empty if the function has no named results. A `return` spills its result
    /// operands into these cells and reloads them to form the returned tuple, so
    /// deferred functions and naked returns observe the latest values.
    /// (Go: `Function.namedResults`.)
    pub named_results: Vec<Value>,
    /// maps each type-checker object to its SSA value
    pub objects: HashMap<ObjectId, Value>,
    /// maps each value to instructions that use it
    pub referrers: Option<HashMap<Value, Vec<InstrId>>>,
    /// register numbers ("tN") for value-producing instructions, assigned by
    /// number_registers after all transformation passes. (Go: `register.num`)
    pub reg_nums: HashMap<InstrId, u32>,
    /// source position of each instruction, set at emit time. Instructions
    /// absent from the map have no position (`NO_POS`). (Go: the per-instruction
    /// `pos token.Pos` field, embedded in `register`/`anInstruction`.)
    pub instr_pos: HashMap<InstrId, Pos>,
    /// if non-empty, a description of the reason this function was synthesized
    /// rather than derived from syntax (e.g. `"package initializer"`). Mirrors
    /// go/ssa's `Function.Synthetic`, printed by the disassembler preamble.
    pub synthetic: Option<String>,
    /// anonymous functions (`FuncLit`s) directly enclosed by this function, in
    /// source order. Their SSA names are `parent$1`, `parent$2`, … (Go:
    /// `Function.AnonFuncs`).
    pub anon_funcs: Vec<FuncId>,
    /// Labelled branch targets keyed by label name. (Go: `Function.lblocks`.)
    pub lblocks: HashMap<String, LBlock>,

    /// Range-over-func jump-state variable (`*int` local in the enclosing
    /// function). Set on yield functions and their ancestors during
    /// `rangeFunc` lowering. (Go: `Function.jump`.)
    pub jump_var: Option<guff_types::ObjectId>,
    /// Exits recorded while building a yield function. (Go: `Function.exits`.)
    pub exits: Vec<Exit>,
    /// Outermost source function for return exits from yield bodies. (Go:
    /// `Function.source`.)
    pub source_func: Option<FuncId>,
    /// `RangeStmt` syntax for a synthetic yield function. (Go: `Function.syntax`.)
    pub syntax_range: Option<guff::ast::RangeStmt>,
    /// Label on the enclosing `range` statement, if any.
    pub yield_label: Option<String>,
    /// Monotonic counter for unique exit ids within this function tree. (Go:
    /// `Function.uniq`.)
    pub uniq: i64,

    // -- generics (instantiation) ------------------------------------------
    /// True if this function was derived from syntax (a `FuncDecl`/`FuncLit`
    /// body is available), as opposed to reconstructed from type information.
    /// Proxy for go/ssa's `fn.syntax != nil`, consulted when choosing a generic
    /// instance's build strategy. Set when a `FuncDecl` body is recorded.
    pub from_syntax: bool,
    /// The function declaration AST when built from syntax; shared with generic
    /// instances of this origin. (Go: `Function.syntax`.)
    pub syntax_decl: Option<guff::ast::FuncDecl>,
    /// The origin (uninstantiated) generic function this function is an
    /// instance of, or `None` if this is not a generic instance. (Go:
    /// `Function.topLevelOrigin`.)
    pub top_level_origin: Option<FuncId>,
    /// This function's type parameters (shared with the origin for instances).
    /// (Go: `Function.typeparams`.)
    pub type_params: Vec<TypeId>,
    /// The type arguments this instance was instantiated with; empty for a
    /// non-instance. (Go: `Function.typeargs`.)
    pub type_args: Vec<TypeId>,
    /// Receiver type parameters, for a method on a generic type. (Go:
    /// `Function.recvtypeparams`.)
    pub recv_type_params: Vec<TypeId>,
    /// Receiver type arguments, for a method instance. (Go:
    /// `Function.recvtypeargs`.)
    pub recv_type_args: Vec<TypeId>,
    /// Type-argument substitution applied when building a concrete instance
    /// from the origin's syntax; `None` unless the build strategy is
    /// [`BuildStrategy::FromSyntax`]. (Go: `Function.subst`, a `*subster`.)
    pub subst: Option<Subster>,
    /// Which builder routine constructs this function's body. (Go:
    /// `Function.build`.)
    pub build_strategy: BuildStrategy,
    /// For a generic origin function, the cache of already-created instances,
    /// keyed by the canonical concatenation of receiver + regular type
    /// arguments. Populated lazily by [`crate::program::Program::instance`].
    /// (Go: `Function.generic.instances`, a `map[*typeList]*Function`.)
    pub generic_instances: HashMap<CanonListId, FuncId>,
    /// For a promotion/indirection wrapper or method-expression thunk, the
    /// selection that describes the wrapped method. (Go: `Function.method`.)
    pub method: Option<crate::wrappers::WrapperSelection>,

    // DEFERRED: Recover, etc.
}

impl Function {
    pub fn new(name: String, parent: Option<FuncId>, pkg: Option<PackageId>) -> Self {
        Self {
            name,
            parent,
            pkg,
            signature: None,
            object: None,
            decl_pos: NO_POS,
            blocks: Arena::new(),
            instrs: Arena::new(),
            params: Arena::new(),
            freevars: Arena::new(),
            locals: Vec::new(),
            named_results: Vec::new(),
            objects: HashMap::default(),
            referrers: None,
            reg_nums: HashMap::default(),
            instr_pos: HashMap::default(),
            synthetic: None,
            anon_funcs: Vec::new(),
            lblocks: HashMap::default(),
            jump_var: None,
            exits: Vec::new(),
            source_func: None,
            syntax_range: None,
            yield_label: None,
            uniq: 0,
            from_syntax: false,
            syntax_decl: None,
            top_level_origin: None,
            type_params: Vec::new(),
            type_args: Vec::new(),
            recv_type_params: Vec::new(),
            recv_type_args: Vec::new(),
            subst: None,
            build_strategy: BuildStrategy::Unset,
            generic_instances: HashMap::default(),
            method: None,
        }
    }

    /// pos returns the source position of instruction `id`, or `NO_POS` if none
    /// was recorded. (Go: `Instruction.Pos`)
    pub fn pos(&self, id: InstrId) -> Pos {
        self.instr_pos.get(&id).copied().unwrap_or(NO_POS)
    }

    /// set_pos records the source position of instruction `id`. Recording
    /// `NO_POS` is a no-op so the map stays sparse.
    pub fn set_pos(&mut self, id: InstrId, pos: Pos) {
        if pos.is_valid() {
            self.instr_pos.insert(id, pos);
        }
    }

    /// finish_body is called when the function's body has been fully built.
    /// (Go: `Function.finishBody`)
    pub fn finish_body(&mut self) {
        if self.blocks.is_empty() {
            return;
        }
        // DEFERRED: Milestone C/D: optimizeBlocks, computeLiveness, etc.
    }

    pub fn dom_preorder(&self) -> Vec<BlockId> {
        let mut ids: Vec<_> = self.blocks.iter().map(|(id, _)| id).collect();
        ids.sort_by_key(|&id| self.blocks.get(id).dom.pre);
        ids
    }

    pub fn dom_postorder(&self) -> Vec<BlockId> {
        let mut ids: Vec<_> = self.blocks.iter().map(|(id, _)| id).collect();
        ids.sort_by_key(|&id| self.blocks.get(id).dom.post);
        ids
    }

    /// number_registers assigns sequential numbers ("t0", "t1", ...) to all
    /// value-producing instructions, in block order then instruction order,
    /// skipping deleted blocks. (Go: `numberRegisters`)
    pub fn number_registers(&mut self) {
        self.reg_nums.clear();
        let mut n = 0u32;
        // Blocks are visited in their semantic order, which (after
        // remove_deleted_blocks renumbering) matches arena order among the
        // surviving blocks.
        let ids: Vec<BlockId> = self.blocks.iter().map(|(id, _)| id).collect();
        for id in ids {
            if self.blocks.get(id).deleted {
                continue;
            }
            let instrs = self.blocks.get(id).instrs.clone();
            for instr_id in instrs {
                if self.instrs.get(instr_id).is_value() {
                    self.reg_nums.insert(instr_id, n);
                    n += 1;
                }
            }
        }
    }

    pub fn compute_referrers(&mut self) {
        let mut referrers: HashMap<Value, Vec<InstrId>> = HashMap::default();
        for (_, block) in self.blocks.iter() {
            if block.deleted {
                continue;
            }
            for &instr_id in &block.instrs {
                let instr = self.instrs.get(instr_id);
                instr.for_each_operand(|val| {
                    referrers.entry(*val).or_default().push(instr_id);
                });
            }
        }
        self.referrers = Some(referrers);
    }

    /// Iterate non-deleted blocks (blockopt leaves deleted entries in the arena).
    pub fn live_blocks(&self) -> impl Iterator<Item = (BlockId, &BasicBlock)> {
        self.blocks.iter().filter(|(_, b)| !b.deleted)
    }
}
