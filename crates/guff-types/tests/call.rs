//! Tests for `call.rs` — selector resolution `x.f` (chunk 26) and call
//! expressions `fun(args...)` (chunk 27).
//!
//! Each test parses a small Go source, collects objects, forces the relevant
//! declarations, then builds the expression node by hand and calls
//! `Checker::selector` / `Checker::call_expr`.

use guff::ast::{BasicLit, CallExpr, Expr, Ident, SelectorExpr};
use guff::parser::{parse_file, Mode};
use guff::position::{FileSet, Pos};
use guff::token::Token;

use guff_types::basic::BasicKind;
use guff_types::object::var::{new_field, new_var};
use guff_types::operand::OperandMode;
use guff_types::r#struct::new_struct;
use guff_types::scope::insert as scope_insert;
use guff_types::signature::{signature_params, signature_recv, signature_results};
use guff_types::{Checker, Config, Operand, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Collect objects, set the lookup scope to the package scope, and force
/// `obj_decl` on every package-scope object so vars/types/methods get real
/// types.
fn checker(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.files = vec![parse(src)];
    check.collect_objects();
    check.sort_objects();
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    // Declare every collected package-scope object.
    let objs: Vec<_> = check.obj_list.clone();
    for obj in objs {
        check.obj_decl(obj);
    }
    check
}

fn ident(name: &str) -> Ident {
    Ident {
        name: name.to_string(),
        ..Default::default()
    }
}

/// Build the selector expression `<base>.<sel>` where `base` is an identifier.
fn selector_expr(base: &str, sel: &str) -> SelectorExpr {
    SelectorExpr {
        id: 0,
        x: Box::new(Expr::Ident(ident(base))),
        sel: ident(sel),
    }
}

/// An integer-literal expression.
fn int_lit(v: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        id: 0,
        value_pos: Pos(1),
        value_end: Pos(1),
        kind: Some(Token::INT),
        value: v.to_string(),
    })
}

/// Build the call expression `<fun_ident>(args...)`.
fn call(fun: &str, args: Vec<Expr>) -> CallExpr {
    CallExpr {
        id: 0,
        fun: Box::new(Expr::Ident(ident(fun))),
        lparen: Pos(1),
        args,
        ellipsis: Pos(0),
        rparen: Pos(1),
    }
}

#[test]
fn field_selection() {
    // Struct type expressions aren't supported by `typexpr` yet (chunk-21
    // deferral), so build the struct + variable directly in the arena:
    //   var x struct { f int }
    let mut check = checker("package p\n");
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);

    let int_t = check.basic(BasicKind::Int);
    let f = new_field(&mut check.objects, "f", int_t, false);
    // Unexported field — its package must match the checker's package for the
    // lookup's same-id comparison to succeed.
    f.set_pkg(&mut check.objects, check.pkg);
    let st = new_struct(&mut check.types, vec![f], vec![]);
    let x = new_var(&mut check.objects, "x", st);
    scope_insert(&mut check.scopes, &mut check.objects, pkg_scope, x);

    let e = selector_expr("x", "f");
    let mut op = Operand::invalid();
    check.selector(&mut op, &e, false);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // x is a variable, so x.f is an addressable variable of type int.
    assert_eq!(op.mode, OperandMode::Variable);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Basic);
}

#[test]
fn method_value() {
    let mut check = checker("package p\ntype T int\nfunc (t T) M() int { return 0 }\nvar x T\n");
    let e = selector_expr("x", "M");
    let mut op = Operand::invalid();
    check.selector(&mut op, &e, false);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // A method value is a (non-addressable) value whose type is the method
    // signature with the receiver removed.
    assert_eq!(op.mode, OperandMode::Value);
    let sig = op.typ.unwrap();
    assert_eq!(sig.kind(&check.types), TypeKind::Signature);
    assert!(
        signature_recv(&check.types, sig).is_none(),
        "method value drops the receiver"
    );
    // No parameters, one result.
    assert!(signature_params(&check.types, sig).is_none());
    assert!(signature_results(&check.types, sig).is_some());
}

#[test]
fn method_expression() {
    let mut check = checker("package p\ntype T int\nfunc (t T) M() int { return 0 }\n");
    // `T.M` — the base is a type name, producing a method expression.
    let e = selector_expr("T", "M");
    let mut op = Operand::invalid();
    check.selector(&mut op, &e, false);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    let sig = op.typ.unwrap();
    assert_eq!(sig.kind(&check.types), TypeKind::Signature);
    // The receiver is promoted to the first parameter; the new signature has
    // no receiver.
    assert!(signature_recv(&check.types, sig).is_none());
    let params = signature_params(&check.types, sig).expect("method expr has a receiver param");
    let n = match check.types.get(params) {
        guff_types::arena::TypeData::Tuple(t) => t.len(),
        _ => panic!("params must be a tuple"),
    };
    assert_eq!(n, 1, "receiver becomes the single parameter");
}

#[test]
fn undefined_field_or_method() {
    let mut check = checker("package p\ntype T int\nvar x T\n");
    let e = selector_expr("x", "nope");
    let mut op = Operand::invalid();
    check.selector(&mut op, &e, false);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "an undefined selector must report an error"
    );
}

// ---------------------------------------------------------------------------
// chunk 27 — call expressions

#[test]
fn simple_call() {
    let mut check = checker("package p\nfunc f(a int, b int) int { return 0 }\n");
    let c = call("f", vec![int_lit("1"), int_lit("2")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Basic);
}

#[test]
fn call_no_results_is_no_value() {
    let mut check = checker("package p\nfunc f() {}\n");
    let c = call("f", vec![]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::NoValue);
}

#[test]
fn call_wrong_arg_count() {
    let mut check = checker("package p\nfunc f(a int, b int) int { return 0 }\n");
    let c = call("f", vec![int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "argument count mismatch must report an error"
    );
}

#[test]
fn variadic_call() {
    let mut check = checker("package p\nfunc g(a ...int) int { return 0 }\n");
    // g(1, 2, 3) — three arguments spread over the variadic parameter.
    let c = call("g", vec![int_lit("1"), int_lit("2"), int_lit("3")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Basic);
}

#[test]
fn variadic_call_zero_args() {
    let mut check = checker("package p\nfunc g(a ...int) int { return 0 }\n");
    let c = call("g", vec![]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
}

#[test]
fn conversion_call() {
    // T(5): a conversion of an untyped constant to a named integer type.
    let mut check = checker("package p\ntype T int\n");
    let c = call("T", vec![int_lit("5")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    // A constant operand stays constant under conversion; its type is T.
    assert_eq!(op.mode, OperandMode::Constant);
    assert_eq!(op.typ.unwrap().kind(&check.types), TypeKind::Named);
}

#[test]
fn call_non_function() {
    let mut check = checker("package p\nvar x int\n");
    let c = call("x", vec![int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        !check.errors.is_empty(),
        "calling a non-function must report an error"
    );
}

// ---------------------------------------------------------------------------
// chunk 35d — generic call type inference

fn arg(name: &str) -> Expr {
    Expr::Ident(ident(name))
}

#[test]
fn generic_call_infers_type_arg_from_argument() {
    // func Id[T any](x T) T — Id(a) with a:int infers T=int, result int.
    let mut check = checker("package p\nfunc Id[T any](x T) T { return x }\nvar a int = 5\n");
    let c = call("Id", vec![arg("a")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Int));
}

#[test]
fn generic_call_two_params_infers_each() {
    // func Pair[A, B any](a A, b B) B — Pair(i, s) infers A=int, B=string; result string.
    let mut check = checker(
        "package p\nfunc Pair[A, B any](a A, b B) B { return b }\nvar i int\nvar s string\n",
    );
    let c = call("Pair", vec![arg("i"), arg("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::String));
}

#[test]
fn generic_variadic_call_infers_elem() {
    // func First[T any](xs ...T) T — First(a, b) with int args infers T=int.
    let mut check = checker(
        "package p\nfunc First[T any](xs ...T) T { var z T; return z }\nvar a int\nvar b int\n",
    );
    let c = call("First", vec![arg("a"), arg("b")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Int));
}

#[test]
fn generic_call_unconstrained_param_cannot_infer() {
    // T doesn't appear in the parameters -> inference must fail.
    let mut check = checker("package p\nfunc Zero[T any]() T { var z T; return z }\n");
    let c = call("Zero", vec![]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert_eq!(op.mode, OperandMode::Invalid);
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::CannotInferTypeArgs),
        "expected CannotInferTypeArgs, got: {:?}",
        check.errors
    );
}

#[test]
fn generic_call_untyped_constant_defaults_to_int() {
    // func Id[T any](x T) T — Id(1) with an untyped int constant infers T=int
    // (chunk 62, infer step 3: untyped-argument default-type promotion).
    let mut check = checker("package p\nfunc Id[T any](x T) T { return x }\n");
    let c = call("Id", vec![int_lit("1")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.mode, OperandMode::Value);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Int));
}

#[test]
fn generic_call_infers_through_interface_param() {
    // func F[T any](x interface{ Get() T }) T — F(s) where S has Get() int
    // infers T=int via shared-method interface inference (chunk 63/64).
    let mut check = checker(
        "package p\n\
         type S struct{}\n\
         func (s S) Get() int { return 0 }\n\
         func F[T any](x interface{ Get() T }) T { var z T; return z }\n\
         var s S\n",
    );
    let c = call("F", vec![arg("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(check.errors.is_empty(), "no errors: {:?}", check.errors);
    assert_eq!(op.typ.unwrap(), check.basic(BasicKind::Int));
}

#[test]
fn generic_call_conflicting_args_cannot_infer() {
    // func Eq[T any](a T, b T) bool — Eq(i, s) can't unify T with int and string.
    let mut check = checker(
        "package p\nfunc Eq[T any](a T, b T) bool { return true }\nvar i int\nvar s string\n",
    );
    let c = call("Eq", vec![arg("i"), arg("s")]);
    let mut op = Operand::invalid();
    check.call_expr(&mut op, &c);
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::CannotInferTypeArgs),
        "expected CannotInferTypeArgs for conflicting args, got: {:?}",
        check.errors
    );
}
