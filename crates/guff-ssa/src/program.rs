//! SSA Program.

use crate::arena::Arena;
use crate::ids::{ConstId, FuncId, GlobalId, PackageId, BuiltinId};
use crate::mode::BuilderMode;
use crate::package::Package;
use crate::function::Function;
use crate::const_val::Const;
use crate::global::Global;
use crate::value::Value;
use guff_constant::Value as ConstantValue;
use guff_types::{ObjectId, TypeId, PackageId as TypePackageId};
use guff::position::FileSet;
use crate::hash::{HashMap, HashSet};
use std::sync::Arc;

/// Builtin represents a Go built-in function.
pub struct Builtin {
    pub name: String,
    pub typ: TypeId,
}

/// A Program is a partial or complete Go program converted to SSA form.
/// (Go: `Program`)
pub struct Program {
    pub mode: BuilderMode,
    pub packages: Arena<PackageId, Package>,
    pub functions: Arena<FuncId, Function>,
    pub constants: Arena<ConstId, Const>,
    pub globals: Arena<GlobalId, Global>,
    pub builtins: Arena<BuiltinId, Builtin>,
    /// maps each type-checker package to its SSA package
    pub package_map: HashMap<TypePackageId, PackageId>,
    // Type information from guff-types (Arc-shared with package / buildir snapshot).
    pub info: Arc<guff_types::Info>,
    pub type_arena: guff_types::TypeArena,
    pub object_arena: guff_types::ObjectArena,
    pub package_arena: guff_types::PackageArena,
    /// the FileSet that positions are relative to, used by the disassembler to
    /// render DebugRef `@ line:col` annotations. `None` until set via
    /// [`Program::set_fset`]. (Go: `Program.Fset`)
    pub fset: Option<Arc<FileSet>>,
    /// Memoized "does this type contain a free type parameter?" cache, consulted
    /// by [`Program::is_parameterized`]. (Go: `Program.hasParams`, a
    /// `typeparams.Free`.)
    pub has_params: crate::has_params::HasParams,
    /// Canonicalizes types and type-argument lists so generic instances with
    /// structurally-identical arguments share one SSA `Function`. (Go:
    /// `Program.canon`.)
    pub canon: crate::canon::Canonizer,
    /// Shared instantiation context, speeding up and deduplicating repeated
    /// `orig[targs...]` instantiations. (Go: `Program.ctxt`, a `types.Context`.)
    pub ctxt: guff_types::Context,
    /// Cache of type-checker method sets. (Go: `Program.MethodSets`.)
    pub method_set_cache: crate::methods::MethodSetCache,
    /// Maps concrete receiver types to lazily-built SSA method implementations.
    /// (Go: `Program.methodSets`.)
    pub concrete_method_sets: HashMap<TypeId, crate::methods::ConcreteMethodSet>,
    /// Memoization of [`Program::object_method`] for methods without package
    /// syntax. (Go: `Program.objectMethods`.)
    pub object_methods: HashMap<ObjectId, FuncId>,
    /// Functions waiting for [`crate::instantiate::Program::build_instance`].
    /// (Go: the builder's enqueue list, drained by `iterate`.)
    pub(crate) pending_builds: Vec<FuncId>,
    /// Types converted to `interface{}` during SSA construction, used by
    /// [`Program::runtime_types`]. Populated when interface conversions are
    /// emitted (currently empty until `MakeInterface` is fully modeled).
    /// (Go: `Program.makeInterfaceTypes`.)
    pub(crate) make_interface_types: HashSet<TypeId>,
}

/// Returns the type of value `v`, interpreted in function `f`'s value-space.
pub fn value_type_of(prog: &Program, f: &Function, v: Value) -> TypeId {
    match v {
        Value::Param(p) => f.params.get(p).typ,
        Value::FreeVar(fv) => f.freevars.get(fv).typ,
        Value::Instr(i) => f
            .instrs
            .get(i)
            .result_type()
            .expect("value instruction has a result type"),
        Value::Global(g) => prog.globals.get(g).typ,
        Value::Const(c) => prog.constants.get(c).typ,
        Value::Builtin(b) => prog.builtins.get(b).typ,
        Value::Function(ff) => prog
            .functions
            .get(ff)
            .signature
            .expect("function value has a signature"),
    }
}

impl Program {
    pub fn new(
        mode: BuilderMode,
        info: impl Into<Arc<guff_types::Info>>,
        type_arena: guff_types::TypeArena,
        object_arena: guff_types::ObjectArena,
        package_arena: guff_types::PackageArena,
    ) -> Self {
        Self {
            mode,
            packages: Arena::new(),
            functions: Arena::new(),
            constants: Arena::new(),
            globals: Arena::new(),
            builtins: Arena::new(),
            package_map: HashMap::default(),
            info: info.into(),
            type_arena,
            object_arena,
            package_arena,
            fset: None,
            has_params: crate::has_params::HasParams::default(),
            canon: crate::canon::Canonizer::default(),
            ctxt: guff_types::Context::new(),
            method_set_cache: crate::methods::MethodSetCache::default(),
            concrete_method_sets: HashMap::default(),
            object_methods: HashMap::default(),
            pending_builds: Vec::new(),
            make_interface_types: HashSet::default(),
        }
    }

    /// Reports whether any of `types` contains a free type parameter. (Go:
    /// `(*Program).isParameterized`.)
    ///
    /// The SSA builder uses this to distinguish a fully-concrete generic
    /// instantiation (which can be built directly under `InstantiateGenerics`)
    /// from one that still mentions type parameters (which needs a wrapper).
    pub fn is_parameterized(&mut self, types: &[TypeId]) -> bool {
        types
            .iter()
            .any(|&t| self.has_params.has(&self.type_arena, &self.object_arena, t))
    }

    /// set_fset records the FileSet used to resolve source positions for
    /// disassembly (DebugRef `@ line:col`).
    pub fn set_fset(&mut self, fset: Arc<FileSet>) {
        self.fset = Some(fset);
    }

    /// basic_type returns the [`TypeId`] of the predeclared `Basic` type of the
    /// given `kind` (e.g. `bool`), looked up in the type arena. This is the SSA
    /// analog of Go's `types.Typ[kind]`; the builder needs it to synthesize
    /// values such as the `init$guard` flag whose types are not present in any
    /// source expression.
    ///
    /// # Panics
    /// Panics if the arena has no such basic type. Any arena seeded from the
    /// type-checker universe contains all predeclared basics.
    /// The source position of a function's declaration. (Go: `Function.Pos()`)
    ///
    /// A function literal carries it on the function itself; a named function's
    /// lives on the type-checker object it was declared from.
    pub fn func_pos(&self, fid: FuncId) -> guff::Pos {
        let f = self.functions.get(fid);
        if f.decl_pos.is_valid() {
            return f.decl_pos;
        }
        f.object
            .map(|obj| guff::Pos(obj.pos(&self.object_arena) as i64))
            .unwrap_or(guff::NO_POS)
    }

    pub fn basic_type(&self, kind: guff_types::BasicKind) -> TypeId {
        guff_types::lookup_basic(&self.type_arena, kind)
            .unwrap_or_else(|| panic!("no predeclared basic type {:?} in arena", kind))
    }

    /// Returns a new slice of all SSA packages created in this program, in
    /// unspecified order. (Go: `(*Program).AllPackages`.)
    pub fn all_packages(&self) -> Vec<PackageId> {
        self.packages.iter().map(|(id, _)| id).collect()
    }

    /// Records that a runtime type descriptor may be needed for `t`. (Go:
    /// membership in `makeInterfaceTypes`.)
    pub fn note_runtime_type(&mut self, t: TypeId) {
        self.make_interface_types.insert(t);
    }

    /// Returns types for which a runtime type descriptor may be required.
    /// (Go: `(*Program).RuntimeTypes`, whose contents `needMethodsOf` builds.)
    ///
    /// Boxing a value into an interface makes its *whole shape* reachable
    /// through reflection, not just the boxed type: go/ssa's `needMethods`
    /// recurses into element, field, key, parameter and result types, and into
    /// `*T` for every named `T`. That closure is the difference between
    /// "`*CLI` was converted to an interface" and "every method of every type
    /// hanging off `CLI` is reachable" — which is what
    /// [`crate::ssautil::all_functions`] turns into call-graph nodes.
    /// syncthing's `(*serveCmd).monitorMain` is reached only this way: nothing
    /// boxes `serveCmd`, but the kong CLI struct that holds it is boxed.
    ///
    /// The `skip` flag mirrors go/ssa: a named type's *underlying* is visited
    /// so its components are reached, but is not itself a runtime type (its
    /// method set is empty by construction, and `RuntimeTypes` omits it).
    pub fn runtime_types(&mut self) -> Vec<TypeId> {
        let seeds: Vec<TypeId> = self.make_interface_types.iter().copied().collect();
        // `visited` maps a type to the `skip` it was recorded with, so a type
        // first seen as an underlying (skip) is re-visited if it is later
        // reached in its own right.
        let mut visited: crate::hash::HashMap<TypeId, bool> = Default::default();
        let mut out: Vec<TypeId> = Vec::new();
        let mut stack: Vec<(TypeId, bool)> = seeds.into_iter().map(|t| (t, false)).collect();
        // A pathological generic instantiation can grow this without bound;
        // the cap is far above any real package and keeps the walk linear.
        const MAX_RUNTIME_TYPES: usize = 100_000;
        while let Some((t, skip)) = stack.pop() {
            if visited.len() > MAX_RUNTIME_TYPES {
                break;
            }
            match visited.get(&t) {
                // Already visited at least as thoroughly as this.
                Some(&prev) if !prev || skip => continue,
                _ => {}
            }
            visited.insert(t, skip);
            if !skip {
                out.push(t);
            }
            let resolved = guff_types::alias::unalias_readonly(&self.type_arena, t);
            if resolved != t {
                stack.push((resolved, skip));
                continue;
            }
            match self.type_arena.get(t) {
                guff_types::TypeData::Basic(_)
                | guff_types::TypeData::Interface(_)
                | guff_types::TypeData::TypeParam(_)
                | guff_types::TypeData::Union(_)
                | guff_types::TypeData::Alias(_) => {}
                guff_types::TypeData::Pointer(p) => stack.push((p.elem(), false)),
                guff_types::TypeData::Slice(sl) => stack.push((sl.elem(), false)),
                guff_types::TypeData::Array(a) => stack.push((a.elem(), false)),
                guff_types::TypeData::Chan(c) => stack.push((c.elem(), false)),
                guff_types::TypeData::Map(m) => {
                    stack.push((m.key(), false));
                    stack.push((m.elem(), false));
                }
                guff_types::TypeData::Struct(st) => {
                    let fields: Vec<guff_types::ObjectId> =
                        (0..st.num_fields()).map(|i| st.field(i)).collect();
                    for f in fields {
                        if let Some(ft) = f.typ(&self.object_arena) {
                            stack.push((ft, false));
                        }
                    }
                }
                guff_types::TypeData::Tuple(tp) => {
                    let vars: Vec<guff_types::ObjectId> =
                        (0..tp.len()).map(|i| tp.at(i)).collect();
                    for v in vars {
                        if let Some(vt) = v.typ(&self.object_arena) {
                            stack.push((vt, false));
                        }
                    }
                }
                guff_types::TypeData::Signature(sig) => {
                    let params = sig.params();
                    let results = sig.results();
                    for tup in [params, results].into_iter().flatten() {
                        stack.push((tup, false));
                    }
                }
                guff_types::TypeData::Named(_) => {
                    let under = t.underlying(&self.type_arena);
                    let ptr = guff_types::new_pointer(&mut self.type_arena, t);
                    stack.push((ptr, false));
                    stack.push((under, true));
                }
            }
        }
        // Deterministic order for callers that walk these (ssautil::all_functions).
        // TypeId has no public Ord; Debug is stable for a given arena id.
        // `sort_by_cached_key` renders each id once rather than at every
        // comparison — the closure now runs over the whole reflective closure,
        // not just the handful of directly-boxed types.
        out.sort_by_cached_key(|t| format!("{t:?}"));
        out.dedup();
        out
    }

    /// emit_const returns a constant with the specified value and type.
    /// (Go: `Program.Const`)
    pub fn emit_const(&mut self, val: Option<ConstantValue>, typ: TypeId) -> Value {
        // TODO: deduplication
        let id = self.constants.alloc(Const::new(val, typ));
        Value::Const(id)
    }

    /// Applies the building function's type-parameter substitution to `t`, if any.
    /// (Go: `(*Function).typ`.)
    pub fn function_typ(&mut self, fid: FuncId, t: TypeId) -> TypeId {
        match self.functions.get_mut(fid).subst.as_mut() {
            Some(subst) => subst.typ(&mut self.type_arena, &mut self.object_arena, t),
            None => t,
        }
    }

    /// finish_function performs post-construction passes on a function.
    pub fn finish_function(&mut self, func_id: FuncId) {
        let f = self.functions.get_mut(func_id);
        if f.blocks.is_empty() {
            return;
        }

        // Remove from f.locals any Allocs that escape to the heap.
        // (Go: `Function.finishBody`.) A cell whose address was taken can be
        // written through that pointer, so it is no longer a plain frame slot;
        // `wastedassign` is the analyzer that reads `locals` and it must not
        // reason about such a cell from its stores alone.
        let heap: Vec<crate::ids::InstrId> = f
            .locals
            .iter()
            .copied()
            .filter(|&id| matches!(f.instrs.get(id), crate::instr::InstrData::Alloc(a) if a.heap))
            .collect();
        if !heap.is_empty() {
            f.locals.retain(|id| !heap.contains(id));
        }

        crate::blockopt::optimize_blocks(f);
        crate::dom::build_dom_tree(f);

        if !self.mode.contains(crate::mode::BuilderMode::NAIVE_FORM) {
            let lifted = crate::lift::lift(self, func_id);
            // Re-run blockopt/dom only when lifting rewrote the CFG.
            if lifted {
                let f = self.functions.get_mut(func_id);
                crate::blockopt::optimize_blocks(f);
                crate::dom::build_dom_tree(f);
            }
        }

        // Assign register numbers ("tN") once all transformation passes are done.
        self.functions.get_mut(func_id).number_registers();
    }
}
