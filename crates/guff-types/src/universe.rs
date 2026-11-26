//! Port of `cmd/compile/internal/types2/universe.go`.
//!
//! Builds the Go universe: predeclared types, the `byte`/`rune` aliases,
//! the predeclared `any` / `error` / `comparable` interfaces, the `true`
//! / `false` / `iota` consts, the predeclared `nil`, and the built-in
//! functions.
//!
//! As of chunk 7, the universe owns a proper [`Scope`](crate::scope::Scope)
//! and a separate [`Package`](crate::package::Package) for `unsafe`,
//! matching Go's two-scope layout. The [`Universe::lookup`] convenience
//! searches both scopes (callers wanting strict Go-style behaviour can use
//! [`Universe::lookup_universe`] or [`Universe::lookup_unsafe`]).
//!
//! Chunk-7 deferrals:
//! - **`assert` and `trace`** test-only builtins are not pre-registered
//!   (matching `defPredeclaredFuncs`'s skip); a follow-up function can
//!   register them on demand.
//! - **`lazyObject`** importer-mode lazy resolution.
//! - **`Universe.Lookup` hijack for `any`** (gotypesalias legacy, D04) —
//!   intentionally not ported. Go 1.22+ always enables type aliases, so the
//!   hijack is obsolete for our supported language levels.

use crate::hash::HashMap;

use guff_constant::{make_bool, make_int64};

use crate::alias::new_alias;
use crate::arena::{ObjectArena, PackageArena, ScopeArena, TypeArena};
use crate::basic::{
    init_alias_basics, init_universe as init_basic_universe, BasicKind, BASIC_KIND_COUNT,
};
use crate::interface::{interface_compute_typeset, interface_set_comparable, new_interface_type};
use crate::named::{new_named, set_underlying};
use crate::object::builtin::{new_builtin, BuiltinId, PREDECLARED_FUNCS};
use crate::object::const_::new_const;
use crate::object::func::new_func;
use crate::object::nil_::new_nil;
use crate::object::type_name::new_type_name;
use crate::object::var::new_var;
use crate::package::new_package;
use crate::scope::{insert as scope_insert, lookup as scope_lookup, new_scope};
use crate::signature::new_signature_type;
use crate::tuple::new_tuple;

use crate::{ObjectId, PackageId, ScopeId, TypeId};

/// The Go universe — predeclared types, constants, and functions.
///
/// Owns all four arenas plus the universe `Scope` and the `unsafe` `Package`.
///
/// Equivalent to `types2.Universe` plus the `Unsafe` package.
#[derive(Debug)]
pub struct Universe {
    pub type_arena: TypeArena,
    pub object_arena: ObjectArena,
    pub scope_arena: ScopeArena,
    pub package_arena: PackageArena,

    /// Predeclared `Basic` types indexed by `BasicKind as usize`.
    pub typ: [TypeId; BASIC_KIND_COUNT],

    /// The universe scope. Holds non-exported predeclared identifiers
    /// (`int`, `bool`, `byte`, `error`, `any`, `comparable`, `nil`,
    /// `true`/`false`/`iota`, the lowercase builtins like `len`/`append`).
    pub universe_scope: ScopeId,

    /// The `unsafe` package. Holds exported builtins (`Add`, `Sizeof`, …)
    /// and the `unsafe.Pointer` TypeName.
    pub unsafe_pkg: PackageId,

    /// Direct handles to commonly-used predeclared items.
    pub byte_typename: ObjectId,
    pub rune_typename: ObjectId,
    pub error: TypeId,
    pub error_typename: ObjectId,
    pub any: TypeId,
    pub any_typename: ObjectId,
    pub comparable: TypeId,
    pub comparable_typename: ObjectId,
    pub true_const: ObjectId,
    pub false_const: ObjectId,
    pub iota_const: ObjectId,
    pub nil: ObjectId,
    pub builtins: HashMap<BuiltinId, ObjectId>,
}

impl Universe {
    /// Look up a name in the universe scope first, then the unsafe-package
    /// scope. Convenient for tests / tooling that doesn't care which
    /// scope contains the identifier.
    pub fn lookup(&self, name: &str) -> Option<ObjectId> {
        if let Some(o) = scope_lookup(&self.scope_arena, self.universe_scope, name) {
            return Some(o);
        }
        let unsafe_scope = self.package_arena.get(self.unsafe_pkg).scope();
        scope_lookup(&self.scope_arena, unsafe_scope, name)
    }

    /// Look up a name only in the universe scope (Go's strict
    /// `Universe.Lookup`).
    pub fn lookup_universe(&self, name: &str) -> Option<ObjectId> {
        scope_lookup(&self.scope_arena, self.universe_scope, name)
    }

    /// Look up a name only in the `unsafe` package scope.
    pub fn lookup_unsafe(&self, name: &str) -> Option<ObjectId> {
        let unsafe_scope = self.package_arena.get(self.unsafe_pkg).scope();
        scope_lookup(&self.scope_arena, unsafe_scope, name)
    }
}

/// Build the full Go universe.
///
/// Equivalent to the `init()` block in `universe.go`.
pub fn init_universe_full() -> Universe {
    let (mut t_arena, typ) = init_basic_universe();
    let mut o_arena = ObjectArena::new();
    let mut s_arena = ScopeArena::new();
    let mut p_arena = PackageArena::new();

    // Universe scope (no parent).
    let universe_scope = new_scope(&mut s_arena, None, None, 0, 0, "universe");

    // Unsafe package — its scope is parented at universe_scope.
    let unsafe_pkg = new_package(
        &mut p_arena,
        &mut s_arena,
        universe_scope,
        "unsafe",
        "unsafe",
    );
    p_arena.get_mut(unsafe_pkg).mark_complete();

    // Helper: insert obj into the right scope based on whether its name
    // is exported. Sets pkg on Unsafe-bound objects.
    let def = |obj: ObjectId,
               o_arena: &mut ObjectArena,
               s_arena: &mut ScopeArena,
               p_arena: &PackageArena| {
        let name = obj.name(o_arena).to_string();
        if name.contains(' ') {
            return; // names with spaces (e.g. "untyped int") aren't entered
        }
        if crate::object::is_exported(&name) {
            obj.set_pkg(o_arena, unsafe_pkg);
            let scope = p_arena.get(unsafe_pkg).scope();
            scope_insert(s_arena, o_arena, scope, obj);
        } else {
            scope_insert(s_arena, o_arena, universe_scope, obj);
        }
    };

    // Predeclared Basic TypeNames.
    {
        let basics: [(BasicKind, &str); 25] = [
            (BasicKind::Bool, "bool"),
            (BasicKind::Int, "int"),
            (BasicKind::Int8, "int8"),
            (BasicKind::Int16, "int16"),
            (BasicKind::Int32, "int32"),
            (BasicKind::Int64, "int64"),
            (BasicKind::Uint, "uint"),
            (BasicKind::Uint8, "uint8"),
            (BasicKind::Uint16, "uint16"),
            (BasicKind::Uint32, "uint32"),
            (BasicKind::Uint64, "uint64"),
            (BasicKind::Uintptr, "uintptr"),
            (BasicKind::Float32, "float32"),
            (BasicKind::Float64, "float64"),
            (BasicKind::Complex64, "complex64"),
            (BasicKind::Complex128, "complex128"),
            (BasicKind::String, "string"),
            // unsafe.Pointer — capital "P" → goes to unsafe package.
            (BasicKind::UnsafePointer, "Pointer"),
            // Untyped basics carry space-prefixed names in Go ("untyped X"),
            // which `def` skips. We follow the same convention.
            (BasicKind::UntypedBool, "untyped bool"),
            (BasicKind::UntypedInt, "untyped int"),
            (BasicKind::UntypedRune, "untyped rune"),
            (BasicKind::UntypedFloat, "untyped float"),
            (BasicKind::UntypedComplex, "untyped complex"),
            (BasicKind::UntypedString, "untyped string"),
            (BasicKind::UntypedNil, "untyped nil"),
        ];
        for (kind, name) in basics {
            let id = new_type_name(&mut o_arena, name, Some(typ[kind as usize]));
            def(id, &mut o_arena, &mut s_arena, &p_arena);
        }
    }

    // `byte` and `rune` are their own Basic values, `identical` to uint8 /
    // int32 but keeping their own names — go/types' `aliases` array. Pointing
    // the TypeNames at the canonical entries instead would be simpler and is
    // what this did before, at the cost of losing which spelling the source
    // used; see `basic::init_alias_basics`.
    let (byte_typ, rune_typ) = init_alias_basics(&mut t_arena);
    let byte_typename = new_type_name(&mut o_arena, "byte", Some(byte_typ));
    def(byte_typename, &mut o_arena, &mut s_arena, &p_arena);
    let rune_typename = new_type_name(&mut o_arena, "rune", Some(rune_typ));
    def(rune_typename, &mut o_arena, &mut s_arena, &p_arena);

    // any = alias of interface{}.
    let empty_iface = new_interface_type(&mut t_arena, vec![], vec![]);
    interface_compute_typeset(&mut t_arena, &o_arena, &p_arena, empty_iface);
    let any_typename = new_type_name(&mut o_arena, "any", None);
    let any = new_alias(&mut t_arena, &mut o_arena, any_typename, Some(empty_iface));
    def(any_typename, &mut o_arena, &mut s_arena, &p_arena);

    // error = type with underlying `interface { Error() string }`.
    let (error, error_typename) = build_error(
        &mut t_arena,
        &mut o_arena,
        &p_arena,
        typ[BasicKind::String as usize],
    );
    def(error_typename, &mut o_arena, &mut s_arena, &p_arena);

    // comparable = type with comparable-flagged empty interface as underlying.
    let (comparable, comparable_typename) = build_comparable(&mut t_arena, &mut o_arena, &p_arena);
    def(comparable_typename, &mut o_arena, &mut s_arena, &p_arena);

    // Predeclared consts.
    let bool_typ = typ[BasicKind::UntypedBool as usize];
    let int_typ = typ[BasicKind::UntypedInt as usize];
    let true_const = new_const(&mut o_arena, "true", bool_typ, make_bool(true));
    let false_const = new_const(&mut o_arena, "false", bool_typ, make_bool(false));
    let iota_const = new_const(&mut o_arena, "iota", int_typ, make_int64(0));
    def(true_const, &mut o_arena, &mut s_arena, &p_arena);
    def(false_const, &mut o_arena, &mut s_arena, &p_arena);
    def(iota_const, &mut o_arena, &mut s_arena, &p_arena);

    // Predeclared nil.
    let nil_obj = new_nil(&mut o_arena, typ[BasicKind::UntypedNil as usize]);
    def(nil_obj, &mut o_arena, &mut s_arena, &p_arena);

    // Predeclared builtin functions (skip test-only assert/trace).
    let invalid_typ = typ[BasicKind::Invalid as usize];
    let all_ids = [
        BuiltinId::Append,
        BuiltinId::Cap,
        BuiltinId::Clear,
        BuiltinId::Close,
        BuiltinId::Complex,
        BuiltinId::Copy,
        BuiltinId::Delete,
        BuiltinId::Imag,
        BuiltinId::Len,
        BuiltinId::Make,
        BuiltinId::Max,
        BuiltinId::Min,
        BuiltinId::New,
        BuiltinId::Panic,
        BuiltinId::Print,
        BuiltinId::Println,
        BuiltinId::Real,
        BuiltinId::Recover,
        BuiltinId::Add,
        BuiltinId::Alignof,
        BuiltinId::Offsetof,
        BuiltinId::Sizeof,
        BuiltinId::Slice,
        BuiltinId::SliceData,
        BuiltinId::String,
        BuiltinId::StringData,
    ];
    let mut builtins = HashMap::default();
    for id in all_ids {
        let obj = new_builtin(&mut o_arena, id, invalid_typ);
        builtins.insert(id, obj);
        def(obj, &mut o_arena, &mut s_arena, &p_arena);
        // Sanity: ensure PREDECLARED_FUNCS table matches.
        let _ = &PREDECLARED_FUNCS[id as usize];
    }

    Universe {
        type_arena: t_arena,
        object_arena: o_arena,
        scope_arena: s_arena,
        package_arena: p_arena,
        typ,
        universe_scope,
        unsafe_pkg,
        byte_typename,
        rune_typename,
        error,
        error_typename,
        any,
        any_typename,
        comparable,
        comparable_typename,
        true_const,
        false_const,
        iota_const,
        nil: nil_obj,
        builtins,
    }
}

/// Build `type error interface { Error() string }`.
fn build_error(
    t_arena: &mut TypeArena,
    o_arena: &mut ObjectArena,
    p_arena: &PackageArena,
    string_typ: TypeId,
) -> (TypeId, ObjectId) {
    let typename = new_type_name(o_arena, "error", None);
    let named = new_named(t_arena, o_arena, typename, None, vec![]);
    let recv = new_var(o_arena, "", named);
    let res = new_var(o_arena, "", string_typ);
    let results = new_tuple(t_arena, &[res]);
    let sig = new_signature_type(t_arena, Some(recv), &[], &[], None, results, false);
    let err_func = new_func(o_arena, "Error", Some(sig));
    let iface = new_interface_type(t_arena, vec![err_func], vec![]);
    interface_compute_typeset(t_arena, o_arena, p_arena, iface);
    set_underlying(t_arena, named, iface);
    (named, typename)
}

/// Build `type comparable interface{}` with comparable=true.
fn build_comparable(
    t_arena: &mut TypeArena,
    o_arena: &mut ObjectArena,
    p_arena: &PackageArena,
) -> (TypeId, ObjectId) {
    let typename = new_type_name(o_arena, "comparable", None);
    let named = new_named(t_arena, o_arena, typename, None, vec![]);
    let iface = new_interface_type(t_arena, vec![], vec![]);
    interface_set_comparable(t_arena, o_arena, p_arena, iface);
    set_underlying(t_arena, named, iface);
    (named, typename)
}
