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
use std::collections::HashMap;
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
    // Type information from guff-types
    pub info: guff_types::Info,
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
    pub concrete_method_sets: std::collections::HashMap<
        TypeId,
        crate::methods::ConcreteMethodSet,
    >,
    /// Memoization of [`Program::object_method`] for methods without package
    /// syntax. (Go: `Program.objectMethods`.)
    pub object_methods: std::collections::HashMap<ObjectId, FuncId>,
    /// Functions waiting for [`crate::instantiate::Program::build_instance`].
    /// (Go: the builder's enqueue list, drained by `iterate`.)
    pub(crate) pending_builds: Vec<FuncId>,
    /// Types converted to `interface{}` during SSA construction, used by
    /// [`Program::runtime_types`]. Populated when interface conversions are
    /// emitted (currently empty until `MakeInterface` is fully modeled).
    /// (Go: `Program.makeInterfaceTypes`.)
    pub(crate) make_interface_types: std::collections::HashSet<TypeId>,
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
        info: guff_types::Info,
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
            package_map: HashMap::new(),
            info,
            type_arena,
            object_arena,
            package_arena,
            fset: None,
            has_params: crate::has_params::HasParams::default(),
            canon: crate::canon::Canonizer::default(),
            ctxt: guff_types::Context::new(),
            method_set_cache: crate::methods::MethodSetCache::default(),
            concrete_method_sets: std::collections::HashMap::new(),
            object_methods: std::collections::HashMap::new(),
            pending_builds: Vec::new(),
            make_interface_types: std::collections::HashSet::new(),
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
    /// (Go: `(*Program).RuntimeTypes`.)
    ///
    /// The full go/ssa implementation also walks element types via reflection;
    /// we return the recorded conversion operands until `MakeInterface` is
    /// fully wired.
    pub fn runtime_types(&self) -> Vec<TypeId> {
        self.make_interface_types.iter().copied().collect()
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

        crate::blockopt::optimize_blocks(f);
        crate::dom::build_dom_tree(f);

        if !self.mode.contains(crate::mode::BuilderMode::NAIVE_FORM) {
            crate::lift::lift(self, func_id);
            // Re-run block optimizations after lifting to cleanup any new opportunities.
            let f = self.functions.get_mut(func_id);
            crate::blockopt::optimize_blocks(f);
            crate::dom::build_dom_tree(f);
        }

        // Assign register numbers ("tN") once all transformation passes are done.
        self.functions.get_mut(func_id).number_registers();
    }
}
