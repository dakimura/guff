//! guff-types — a Rust port of Go's `go/types` package.
//!
//! Ported from the source of truth at `cmd/compile/internal/types2` (the Go
//! compiler's type-checker). Public `go/types` API conventions can be layered
//! on top later.
//!
//! ## Data model
//!
//! Types and objects form a cyclic, mutable graph in `go/types`. We model
//! that with two arenas — see [`arena`] — and use [`arena::TypeId`] /
//! [`arena::ObjectId`] indices in place of Go's `*T` pointers. Constructors
//! take `&mut TypeArena` (or `&mut ObjectArena`) and return an ID;
//! accessors take a shared arena reference.
//!
//! ## Status (chunks 1 + 2 + 3)
//!
//! All Go type-kind variants are now represented in the arena: `Basic`,
//! `Array`, `Slice`, `Pointer`, `Map`, `Chan`, `Tuple`, `Struct`, `Signature`,
//! `Interface`, `Union`, `Named`, `Alias`, `TypeParam`. Object kinds: `Var`,
//! `Func`, `TypeName`. Several Checker-dependent helpers are still stubbed —
//! see per-module docs for what's deferred (typeset.go-derived predicates on
//! `Interface` and `TypeParam`, variadic last-param validation, generic
//! instantiation, the `resolveUnderlying` chain-walker, etc.). The type
//! checker proper arrives in subsequent chunks.

pub mod alias;
pub mod api;
pub mod api_predicates;
pub mod arena;
pub mod array;
pub mod assignments;
pub mod basic;
pub mod builtins;
pub mod call;
pub mod chan;
pub mod check;
pub mod check_assign;
pub mod check_expr_const;
pub mod check_lookup;
pub mod context;
pub mod conversions;
pub mod cycles;
pub mod decl;
pub mod errors;
pub mod expr;
pub mod format;
pub mod importer;
pub mod index;
pub mod infer;
pub mod initorder;
pub mod instantiate;
pub mod interface;
pub mod interface_check;
pub mod labels;
pub mod literals;
pub mod lookup;
pub mod map;
pub mod mono;
pub mod named;
pub mod object;
pub mod objectpath;
pub mod objset;
pub mod operand;
pub mod package;
pub mod pointer;
pub mod predicates;
pub mod recording;
pub mod resolver;
pub mod return_check;
pub mod scope;
pub mod selection;
pub mod signature;
pub mod signature_check;
pub mod sizes;
pub mod slice;
pub mod stmt;
pub mod r#struct;
pub mod struct_check;
pub mod subst;
pub mod termlist;
pub mod tuple;
pub mod type_;
pub mod typelists;
pub mod typeparam;
pub mod typeset;
pub mod typestring;
pub mod typeterm;
pub mod typexpr;
pub mod under;
pub mod unify;
pub mod union;
pub mod universe;
pub mod util;
pub mod validtype;
pub mod version;

pub use alias::{
    alias_obj, alias_origin, alias_rhs, alias_set_type_params, new_alias, unalias,
    unalias_readonly, Alias,
};
pub use api::{Config, Info, TypeAndValue, TypeCheckError};
pub use api_predicates::{
    api_assertable_to, api_assignable_to, api_convertible_to, api_identical,
    api_identical_ignore_tags, api_implements, api_satisfies,
};
pub use arena::{
    ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, ScopeArena, ScopeId, TypeArena,
    TypeData, TypeId,
};
pub use array::{array_elem, array_len, new_array, Array};
pub use assignments::{assignable_to, AssignableResult};
pub use basic::{
    basic_info, basic_kind, basic_name, init_universe, lookup_basic, Basic, BasicInfo, BasicKind,
    BASIC_KIND_COUNT, BYTE, IS_BOOLEAN, IS_COMPLEX, IS_CONST_TYPE, IS_FLOAT, IS_INTEGER,
    IS_NUMERIC, IS_ORDERED, IS_STRING, IS_UNSIGNED, IS_UNTYPED, RUNE,
};
pub use chan::{chan_dir, chan_elem, new_chan, Chan, ChanDir};
pub use check::{Action, Checker, Environment, ExportSeed};
pub use check_expr_const::representable_const;
pub use check_lookup::MissingMethod;
pub use context::Context;
pub use conversions::{
    convertible_to, is_bytes_or_runes, is_pointer, is_uintptr, is_unsafe_pointer,
};
pub use format::{ndigits, qualifier, strip_annotations};
pub use importer::{ImportCtx, Importer};
pub use infer::{
    core_term, infer, is_parameterized, kill_cycles, rename_tparams, CoreTerm, InferResult,
};
pub use instantiate::{
    instantiate, new_alias_instance, new_named_instance, new_signature_instance,
};
pub use interface::{
    interface_compute_typeset, interface_embedded_type, interface_empty, interface_explicit_method,
    interface_is_comparable, interface_is_implicit, interface_is_method_set,
    interface_mark_implicit, interface_method, interface_num_embeddeds,
    interface_num_explicit_methods, interface_num_methods, interface_set_comparable,
    interface_typeset, new_interface_type, Interface,
};
pub use lookup::{
    as_named, concat, deref, deref_struct_ptr, field_index, has_invalid_embedded_fields,
    is_interface_ptr, lookup_field_or_method, lookup_field_or_method_fold, lookup_selection,
    method_index, LookupResult,
};
pub use map::{map_elem, map_key, new_map, Map};
pub use named::{
    add_method, named_lookup_method, named_method, named_num_methods, named_obj, named_origin,
    named_set_type_params, named_type_args, named_underlying, new_named, set_underlying, Instance,
    Named,
};
pub use object::builtin::{
    builtin_info, new_builtin, Builtin, BuiltinId, BuiltinInfo, ExprKind, PREDECLARED_FUNCS,
};
pub use object::const_::{new_const, Const};
pub use object::func::{func_has_ptr_recv, new_func, Func};
pub use object::nil_::{new_nil, Nil};
pub use object::pkgname::{new_pkg_name, PkgName};
pub use object::type_name::{new_type_name, type_name_set_typ, TypeName};
pub use object::var::{new_field, new_param, new_var, Var, VarKind};
pub use object::{cmp as object_cmp, id as object_id, is_exported, ObjectMeta};
pub use objectpath::{
    for_object as objectpath_for, object as objectpath_object, Path as ObjectPath,
};
pub use objset::ObjSet;
pub use operand::{composite_kind, operand_string, Operand, OperandMode};
pub use package::{new_package, Package};
pub use pointer::{new_pointer, pointer_elem, Pointer};
pub use predicates::{
    comparable, comparable_type, default_type, has_empty_typeset, has_name, has_nil, identical,
    identical_origin, identical_with, is_basic, is_boolean, is_complex, is_const_type, is_float,
    is_generic, is_integer, is_integer_or_float, is_interface, is_non_type_param_interface,
    is_numeric, is_string, is_type_lit, is_type_param, is_typed, is_unsigned, is_untyped,
    is_untyped_numeric, is_valid, is_valid_name, max_type, same_pkg, IdenticalCfg,
};
pub use r#struct::{new_struct, struct_field, struct_num_fields, struct_tag, Struct};
pub use resolver::DeclInfo;
pub use scope::{
    insert as scope_insert, lookup as scope_lookup, lookup_chain, lookup_ignoring_case, new_scope,
    Scope,
};
pub use selection::{selection_string, selection_type, Selection, SelectionKind};
pub use signature::{
    new_signature_type, signature_params, signature_recv, signature_recv_type_params,
    signature_results, signature_set_recv_type_params, signature_set_type_params,
    signature_type_params, signature_variadic, Signature,
};
pub use sizes::{align, default_sizes, is_sync_atomic_align64, sizes_for, Sizes, SizesKind};
pub use slice::{new_slice, slice_elem, Slice};
pub use stmt::StmtContext;
pub use subst::{make_subst_map, subst, SubstMap};
pub use tuple::{empty_tuple, new_tuple, tuple_at, tuple_len, Tuple};
pub use type_::TypeKind;
pub use typelists::{
    bind_tparams, new_type_list, type_list_len, type_param_list_len, TypeList, TypeParamList,
};
pub use typeparam::{
    new_type_param, set_constraint, type_param_constraint, type_param_iface, type_param_index,
    type_param_obj, type_param_underlying_full, TypeParam,
};
pub use typeset::TypeSet;
pub use typestring::{signature_string, type_string, Qualifier};
pub use under::{all, common_under, type_errorf, typeset_iter, under_is, TypeError};
pub use unify::{
    as_interface, unify, Unifier, UnifyMode, ENABLE_CORE_TYPE_UNIFICATION, UNIFICATION_DEPTH_LIMIT,
};
pub use union::{new_term, new_union, union_len, union_term, Term, Union};
pub use universe::{init_universe_full, Universe};
pub use util::{cmp_pos, end_pos, has_dots, is_ddd_array, start_pos};
pub use validtype::{make_obj_list, valid_type, ValidResult};
pub use version::{
    as_go_version, go1_13, go1_14, go1_17, go1_18, go1_20, go1_21, go1_22, go1_23, go1_26, go1_9,
    go_current, GoVersion,
};
