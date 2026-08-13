//! Chunk-35a tests: generic type instantiation `T[A]` / `T[A, B]` in
//! `Checker::typ` (the `IndexExpr` / `IndexListExpr` cases of `typInternal`).
//!
//! The first group builds the generic `Named` directly in the arena (the
//! chunk-26 selector-test precedent). The second group (chunk 35a-decl)
//! drives whole-source generic type *declarations* (`type Vec[T any] []T`)
//! now that `decl.rs::collect_type_params` is wired.

use guff::ast::{Expr, Ident, IndexExpr, IndexListExpr};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::Pos;

use guff_types::arena::TypeData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{
    bind_tparams, named_set_type_params, named_type_args, new_named, new_slice, new_type_name,
    new_type_param, scope_insert, BasicKind, Checker, Config,
};
use guff_types_errors::Code;

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
    check
}

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident::new_ident(name))
}

fn index1(x: Expr, arg: Expr) -> Expr {
    Expr::IndexExpr(IndexExpr {
        id: 0,
        x: Box::new(x),
        lbrack: Pos::default(),
        index: Box::new(arg),
        rbrack: Pos::default(),
    })
}

/// Build a generic `Named` `Vec[T any] []T` in the Checker's arenas and insert
/// it into the universe scope under `name`. Returns the generic origin TypeId.
fn declare_generic_vec(c: &mut Checker, name: &str) -> guff_types::TypeId {
    let tn_t = new_type_name(&mut c.objects, "T", None);
    let tp = new_type_param(&mut c.types, tn_t, None);
    let tlist = bind_tparams(&mut c.types, vec![tp]).unwrap();

    let tn_vec = new_type_name(&mut c.objects, name, None);
    let slice_of_t = new_slice(&mut c.types, tp);
    let vec_named = new_named(
        &mut c.types,
        &mut c.objects,
        tn_vec,
        Some(slice_of_t),
        vec![],
    );
    named_set_type_params(&mut c.types, vec_named, tlist);

    scope_insert(&mut c.scopes, &mut c.objects, c.universe_scope, tn_vec);
    vec_named
}

#[test]
fn instantiates_generic_named_single_arg() {
    let mut c = Checker::new(Config::default());
    declare_generic_vec(&mut c, "Vec");

    let int = c.basic(BasicKind::Int);
    let t = c.typ(&index1(ident("Vec"), ident("int")));

    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
    // The result is an instantiated Named carrying the type argument `int`.
    assert!(matches!(c.types.get(t), TypeData::Named(_)));
    let targs = named_type_args(&c.types, t).expect("instance should have type args");
    assert_eq!(targs.list(), &[int], "instance should record targ = int");
}

#[test]
fn instantiation_dedups_via_context() {
    let mut c = Checker::new(Config::default());
    declare_generic_vec(&mut c, "Vec");

    let a = c.typ(&index1(ident("Vec"), ident("int")));
    let b = c.typ(&index1(ident("Vec"), ident("int")));
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
    assert_eq!(a, b, "identical instantiations should dedup to one TypeId");

    let s = c.typ(&index1(ident("Vec"), ident("string")));
    assert_ne!(a, s, "different type args → different instance");
}

#[test]
fn non_generic_type_instantiation_is_error() {
    let mut c = Checker::new(Config::default());
    // `int[int]` — int is not a generic type.
    let t = c.typ(&index1(ident("int"), ident("int")));
    assert!(!guff_types::is_valid(&c.types, t));
    assert!(
        c.errors.iter().any(|e| e.code == Code::NotAGenericType),
        "expected NotAGenericType, got: {:?}",
        c.errors
    );
}

#[test]
fn too_many_type_args_is_error() {
    let mut c = Checker::new(Config::default());
    declare_generic_vec(&mut c, "Vec");

    // Vec has 1 type param; give it 2.
    let e = Expr::IndexListExpr(IndexListExpr {
        id: 0,
        x: Box::new(ident("Vec")),
        lbrack: Pos::default(),
        indices: vec![ident("int"), ident("string")],
        rbrack: Pos::default(),
    });
    let t = c.typ(&e);
    assert!(!guff_types::is_valid(&c.types, t));
    assert!(
        c.errors.iter().any(|e| e.code == Code::WrongTypeArgCount),
        "expected WrongTypeArgCount, got: {:?}",
        c.errors
    );
}

#[test]
fn invalid_type_arg_yields_invalid_instance() {
    let mut c = Checker::new(Config::default());
    declare_generic_vec(&mut c, "Vec");

    // `Vec[Bogus]` — Bogus is undefined, so the type arg is invalid and the
    // whole instantiation collapses to Typ[Invalid].
    let t = c.typ(&index1(ident("Vec"), ident("Bogus")));
    assert!(!guff_types::is_valid(&c.types, t));
    assert!(
        c.errors.iter().any(|e| e.code == Code::UndeclaredName),
        "expected UndeclaredName for the bad type arg, got: {:?}",
        c.errors
    );
}

// ----------------------------------------------------------------------------
// chunk 35a-decl: whole-source generic type declarations + instantiation.

#[test]
fn source_generic_named_decl_and_instantiation() {
    // `type Vec[T any] []T` declared, then instantiated as `Vec[int]`.
    let c = check_src("package p\ntype Vec[T any] []T\nvar v Vec[int]\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);

    let pkg_scope = c.packages.get(c.pkg).scope();
    let vec = scope_lookup(&c.scopes, pkg_scope, "Vec").expect("Vec");
    // Vec is a generic Named type.
    let vec_typ = vec.typ(&c.objects).unwrap();
    assert!(
        guff_types::is_generic(&c.types, vec_typ),
        "Vec should be generic"
    );

    // v : Vec[int] is an instantiated Named carrying the type arg `int`.
    let v = scope_lookup(&c.scopes, pkg_scope, "v").expect("v");
    let vt = v.typ(&c.objects).unwrap();
    assert!(matches!(c.types.get(vt), TypeData::Named(_)));
    let int = c.basic(BasicKind::Int);
    assert_eq!(
        named_type_args(&c.types, vt)
            .expect("instance targs")
            .list(),
        &[int]
    );
}

#[test]
fn source_generic_multi_param_decl() {
    // Grouped + multi-param: `type Pair[K comparable, V any] struct{...}`.
    let c = check_src(
        "package p\n\
         type Pair[K comparable, V any] struct { key K; val V }\n\
         var p Pair[int, string]\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);

    let pkg_scope = c.packages.get(c.pkg).scope();
    let p = scope_lookup(&c.scopes, pkg_scope, "p").expect("p");
    let pt = p.typ(&c.objects).unwrap();
    assert!(matches!(c.types.get(pt), TypeData::Named(_)));
    let int = c.basic(BasicKind::Int);
    let string = c.basic(BasicKind::String);
    assert_eq!(
        named_type_args(&c.types, pt)
            .expect("instance targs")
            .list(),
        &[int, string]
    );
}

#[test]
fn source_grouped_type_params_share_bound() {
    // `[A, B any]` — one Field, two names sharing the `any` bound.
    let c = check_src("package p\ntype P[A, B any] struct { a A; b B }\nvar x P[int, bool]\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
    let pkg_scope = c.packages.get(c.pkg).scope();
    let x = scope_lookup(&c.scopes, pkg_scope, "x").expect("x");
    let xt = x.typ(&c.objects).unwrap();
    assert!(matches!(c.types.get(xt), TypeData::Named(_)));
}

#[test]
fn source_wrong_targ_count_is_error() {
    // Vec has one type parameter; supplying two is an error.
    let c = check_src("package p\ntype Vec[T any] []T\nvar v Vec[int, string]\n");
    assert!(
        c.errors.iter().any(|e| e.code == Code::WrongTypeArgCount),
        "expected WrongTypeArgCount, got: {:?}",
        c.errors
    );
}

// ----------------------------------------------------------------------------
// chunk 35b: constraint satisfaction (Checker.verify -> implements(constraint)).

#[test]
fn comparable_constraint_satisfied() {
    // int is comparable -> Set[int] is fine.
    let c = check_src("package p\ntype Set[T comparable] struct { x T }\nvar s Set[int]\n");
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn comparable_constraint_violated() {
    // A slice type is not comparable -> Set[[]int] violates the constraint.
    let c = check_src("package p\ntype Set[T comparable] struct { x T }\nvar s Set[[]int]\n");
    assert!(
        c.errors.iter().any(|e| e.code == Code::InvalidTypeArg),
        "expected InvalidTypeArg (not comparable), got: {:?}",
        c.errors
    );
}

#[test]
fn union_constraint_satisfied() {
    // int is in the type set {~int | ~string} -> ok.
    let c = check_src(
        "package p\n\
         type Ordered interface { ~int | ~string }\n\
         type Box[T Ordered] struct { v T }\n\
         var b Box[int]\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn union_constraint_violated() {
    // bool is not in {~int | ~string} -> InvalidTypeArg.
    let c = check_src(
        "package p\n\
         type Ordered interface { ~int | ~string }\n\
         type Box[T Ordered] struct { v T }\n\
         var b Box[bool]\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::InvalidTypeArg),
        "expected InvalidTypeArg (bool not in type set), got: {:?}",
        c.errors
    );
}

#[test]
fn inline_union_constraint_violated() {
    // Bare `~int | ~string` constraint literal wrapped in an implicit interface.
    let c = check_src(
        "package p\n\
         type Box[T ~int | ~string] struct { v T }\n\
         var b Box[bool]\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::InvalidTypeArg),
        "expected InvalidTypeArg for inline union constraint, got: {:?}",
        c.errors
    );
}

// ----------------------------------------------------------------------------
// Generic instance method selection (chunk 67 — D05): the origin's methods are
// found on an instance, and their signatures are instantiated with the
// instance's type arguments at selection time.
// ----------------------------------------------------------------------------

#[test]
fn instance_method_value_returns_instantiated_type() {
    // b.Get() on Box[int] returns int, so `var x int = b.Get()` type-checks.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         func f() { var b Box[int]; var x int = b.Get(); _ = x }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn instance_method_result_type_mismatch_is_error() {
    // b.Get() returns int, so assigning to a string must fail.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         func f() { var b Box[int]; var x string = b.Get(); _ = x }\n",
    );
    assert!(!c.errors.is_empty(), "expected a type error, got none");
}

#[test]
fn instance_method_argument_type_is_instantiated() {
    // s.Add(3) on Set[int] takes an int argument (T → int).
    let ok = check_src(
        "package p\n\
         type Set[T any] struct{}\n\
         func (s Set[T]) Add(x T) {}\n\
         func f() { var s Set[int]; s.Add(3) }\n",
    );
    assert!(ok.errors.is_empty(), "unexpected errors: {:?}", ok.errors);

    // s.Add(\"x\") must fail: argument should be int, not string.
    let bad = check_src(
        "package p\n\
         type Set[T any] struct{}\n\
         func (s Set[T]) Add(x T) {}\n\
         func f() { var s Set[int]; s.Add(\"x\") }\n",
    );
    assert!(
        !bad.errors.is_empty(),
        "expected an argument type error, got none"
    );
}

#[test]
fn instance_method_expression_promotes_receiver() {
    // Box[int].Get is a func(Box[int]) int.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         func f() { var b Box[int]; g := Box[int].Get; var x int = g(b); _ = x }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn instance_method_via_pointer_receiver() {
    // Pointer-receiver method on an addressable instance value.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b *Box[T]) Get() T { return b.v }\n\
         func f() { var b Box[int]; var x int = b.Get(); _ = x }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn embedded_generic_field_promotes_instantiated_method() {
    // Outer embeds Box[int]; the promoted method o.Get() returns int.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         type Outer struct { Box[int] }\n\
         func f() { var o Outer; var x int = o.Get(); _ = x }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn embedded_generic_field_promoted_method_type_mismatch() {
    // The promoted o.Get() returns int, not string.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         type Outer struct { Box[int] }\n\
         func f() { var o Outer; var x string = o.Get(); _ = x }\n",
    );
    assert!(!c.errors.is_empty(), "expected a type error, got none");
}

// ----------------------------------------------------------------------------
// Interface satisfaction of generic instances (chunk 69 — D05): a generic
// instance's methods are expanded (with substituted signatures) before the
// interface-satisfaction check, so `Box[int]` satisfies `interface{ Get() int }`.
// ----------------------------------------------------------------------------

#[test]
fn instance_satisfies_interface() {
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         type Getter interface { Get() int }\n\
         func f() { var b Box[int]; var g Getter = b; _ = g }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn instance_does_not_satisfy_interface_on_type_mismatch() {
    // Box[string].Get() returns string, so it must NOT satisfy Getter{ Get() int }.
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b Box[T]) Get() T { return b.v }\n\
         type Getter interface { Get() int }\n\
         func f() { var b Box[string]; var g Getter = b; _ = g }\n",
    );
    assert!(
        !c.errors.is_empty(),
        "expected an interface-assign error, got none"
    );
}

#[test]
fn pointer_to_instance_satisfies_interface() {
    let c = check_src(
        "package p\n\
         type Box[T any] struct { v T }\n\
         func (b *Box[T]) Get() T { return b.v }\n\
         type Getter interface { Get() int }\n\
         func f() { var b Box[int]; var g Getter = &b; _ = g }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

// ----------------------------------------------------------------------------
// Explicit generic function instantiation `f[targs]` (D21/D22 remainder)

#[test]
fn explicit_func_instantiation_call_result_type() {
    // Id[int](3) yields int, so `var x int = Id[int](3)` type-checks.
    let c = check_src(
        "package p\n\
         func Id[T any](x T) T { return x }\n\
         func f() { var x int = Id[int](3); _ = x }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn explicit_func_instantiation_result_type_mismatch() {
    // Id[int](3) is int, so assigning to a string must fail.
    let c = check_src(
        "package p\n\
         func Id[T any](x T) T { return x }\n\
         func f() { var x string = Id[int](3); _ = x }\n",
    );
    assert!(!c.errors.is_empty(), "expected a type error, got none");
}

#[test]
fn explicit_func_instantiation_argument_type_checked() {
    // Id[int] takes an int argument; passing a string must fail.
    let c = check_src(
        "package p\n\
         func Id[T any](x T) T { return x }\n\
         func f() { _ = Id[int](\"x\") }\n",
    );
    assert!(
        !c.errors.is_empty(),
        "expected an argument type error, got none"
    );
}

#[test]
fn explicit_func_instantiation_value_form() {
    // A partially-applied generic function value: `var g func(int) int = Id[int]`.
    let c = check_src(
        "package p\n\
         func Id[T any](x T) T { return x }\n\
         func f() { var g func(int) int = Id[int]; _ = g }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn explicit_func_instantiation_two_type_args() {
    // Pair[int, string] — multi-index (IndexListExpr) explicit instantiation.
    let c = check_src(
        "package p\n\
         func Pair[A any, B any](a A, b B) B { return b }\n\
         func f() { var s string = Pair[int, string](1, \"x\"); _ = s }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn explicit_func_instantiation_too_many_type_args() {
    let c = check_src(
        "package p\n\
         func Id[T any](x T) T { return x }\n\
         func f() { _ = Id[int, string](3) }\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::WrongTypeArgCount),
        "expected WrongTypeArgCount, got {:?}",
        c.errors
    );
}

#[test]
fn explicit_func_instantiation_constraint_violation() {
    // T is constrained to `comparable`; instantiating with `[]int` violates it.
    let c = check_src(
        "package p\n\
         func Keys[T comparable](x T) T { return x }\n\
         func f() { _ = Keys[[]int] }\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::InvalidTypeArg),
        "expected InvalidTypeArg, got {:?}",
        c.errors
    );
}

// --- partially explicit type arguments -------------------------------------
//
// `f[int](x)` for a two-parameter `f`: what the call writes is not enough on
// its own, and the rest comes from the arguments. Upstream keeps the signature
// generic and hands both halves to one `infer` (`callExpr` → `arguments(call,
// sig, targs, …)`); guff used to report `CannotInferTypeArgs` on sight, which
// cost kubernetes' `util/sets` — and, through it, every package importing it.

#[test]
fn partial_explicit_targs_are_completed_from_the_arguments() {
    // The shape from kubernetes: `sets.KeySet[string](m)`, where M and V are
    // only knowable from the argument.
    let c = check_src(
        "package p\n\
         type Set[T comparable] map[T]struct{}\n\
         func KeySet[T comparable, M ~map[T]V, V any](m M) Set[T] { var s Set[T]; return s }\n\
         func f() { m := map[string]int{}; _ = KeySet[string](m) }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn partial_explicit_targs_bind_the_parameters_they_were_written_for() {
    // `two[int](1, "s")` must fix A = int and infer B = string, not the other
    // way round: a partial list is positional.
    let c = check_src(
        "package p\n\
         func two[A any, B any](a A, b B) B { return b }\n\
         func f() { var s string = two[int](1, \"s\"); _ = s }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn a_written_type_argument_still_type_checks_its_own_parameter() {
    // Seeding inference must not turn the explicit half into a free variable:
    // A is int, so a string argument in that position is still an error.
    let c = check_src(
        "package p\n\
         func two[A any, B any](a A, b B) B { return b }\n\
         func f() { _ = two[int](\"x\", \"s\") }\n",
    );
    assert!(!c.errors.is_empty(), "expected an argument type error");
}

#[test]
fn partial_explicit_targs_that_the_arguments_contradict_still_fail() {
    // M is written as `map[string]int` but the argument is a different map:
    // inference has nothing to reconcile and the call must not pass.
    let c = check_src(
        "package p\n\
         func get[K comparable, V any](m map[K]V, k K) V { return m[k] }\n\
         func f() { m := map[string]int{}; _ = get[int](m, 1) }\n",
    );
    assert!(!c.errors.is_empty(), "expected a type error");
}

#[test]
fn partial_explicit_targs_in_value_position_are_still_an_error() {
    // No argument list to learn from, and the assignment-target path is not
    // ported — same as upstream with no target.
    let c = check_src(
        "package p\n\
         func two[A any, B any](a A, b B) B { return b }\n\
         func f() { _ = two[int] }\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::CannotInferTypeArgs),
        "expected CannotInferTypeArgs, got {:?}",
        c.errors
    );
}

#[test]
fn too_many_type_arguments_is_still_counted_first() {
    let c = check_src(
        "package p\n\
         func two[A any, B any](a A, b B) B { return b }\n\
         func f() { _ = two[int, string, bool](1, \"s\") }\n",
    );
    assert!(
        c.errors.iter().any(|e| e.code == Code::WrongTypeArgCount),
        "expected WrongTypeArgCount, got {:?}",
        c.errors
    );
}
