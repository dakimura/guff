//! Tests for `signature_check.rs` (chunk 24) — `Checker::func_type`.
//!
//! Parses a function declaration, runs `func_type` on its AST, and inspects
//! the resulting Signature (params / results / variadic / receiver).

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::signature::{
    signature_params, signature_recv, signature_results, signature_variadic,
};
use guff_types::tuple::tuple_len;
use guff_types::{Checker, Config, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Collect objects from `src`, then build the signature of the named func.
fn sig_of(src: &str, fname: &str) -> (Checker, guff_types::TypeId) {
    let mut check = Checker::new(Config::default());
    let file = parse(src);
    check.files = vec![file.clone()];
    check.collect_objects();
    // Resolve type names against the package scope (the real flow sets this
    // from the declaring file's scope during funcDecl/objDecl).
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    // Find the FuncDecl and build its signature.
    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == fname => Some(fd.clone()),
            _ => None,
        })
        .expect("func decl found");
    let sig = check.func_type(fd.recv.as_ref(), &fd.ty);
    (check, sig)
}

#[test]
fn simple_func_params_and_result() {
    let (check, sig) = sig_of(
        "package p\nfunc f(a int, b int) bool { return true }\n",
        "f",
    );
    let params = signature_params(&check.types, sig);
    let results = signature_results(&check.types, sig);
    assert_eq!(tuple_len(&check.types, params), 2);
    assert_eq!(tuple_len(&check.types, results), 1);
    assert!(!signature_variadic(&check.types, sig));
    assert!(signature_recv(&check.types, sig).is_none());
}

#[test]
fn no_params_no_results() {
    let (check, sig) = sig_of("package p\nfunc f() {}\n", "f");
    assert_eq!(
        tuple_len(&check.types, signature_params(&check.types, sig)),
        0
    );
    assert_eq!(
        tuple_len(&check.types, signature_results(&check.types, sig)),
        0
    );
}

#[test]
fn variadic_last_param_becomes_slice() {
    let (check, sig) = sig_of("package p\nfunc f(a int, b ...string) {}\n", "f");
    assert!(signature_variadic(&check.types, sig));
    let params = signature_params(&check.types, sig);
    assert_eq!(tuple_len(&check.types, params), 2);
    // The last parameter's type is []string (a Slice).
    let last = guff_types::tuple::tuple_at(&check.types, params.unwrap(), 1);
    let last_ty = last.typ(&check.objects).unwrap();
    assert_eq!(last_ty.kind(&check.types), TypeKind::Slice);
}

#[test]
fn method_receiver_is_collected() {
    let (check, sig) = sig_of(
        "package p\ntype T int\nfunc (t T) M(x int) int { return x }\n",
        "M",
    );
    let recv = signature_recv(&check.types, sig).expect("receiver present");
    assert_eq!(recv.name(&check.objects), "t");
    // params/results don't include the receiver.
    assert_eq!(
        tuple_len(&check.types, signature_params(&check.types, sig)),
        1
    );
    assert_eq!(
        tuple_len(&check.types, signature_results(&check.types, sig)),
        1
    );
}

#[test]
fn pointer_receiver_is_collected() {
    let (check, sig) = sig_of("package p\ntype T int\nfunc (t *T) P() {}\n", "P");
    let recv = signature_recv(&check.types, sig).expect("receiver present");
    let rty = recv.typ(&check.objects).unwrap();
    assert_eq!(rty.kind(&check.types), TypeKind::Pointer);
}

// ---------------------------------------------------------------------------
// chunk 35c — generic function declarations.

#[test]
fn generic_func_collects_type_params_and_uses_them() {
    // func F[T any](x T) T { ... } — the parameter and result types are the
    // type parameter T (declared in the function scope while collecting params).
    let (check, sig) = sig_of("package p\nfunc F[T any](x T) T { return x }\n", "F");

    let tps = guff_types::signature_type_params(&check.types, sig).expect("type params");
    assert_eq!(tps.len(), 1);
    let t_param = tps.list()[0];
    assert_eq!(t_param.kind(&check.types), TypeKind::TypeParam);

    // param x : T and the result : T both resolve to the same TypeParam.
    let params = signature_params(&check.types, sig).unwrap();
    let x = guff_types::tuple::tuple_at(&check.types, params, 0);
    assert_eq!(x.typ(&check.objects).unwrap(), t_param);
    let results = signature_results(&check.types, sig).unwrap();
    let r = guff_types::tuple::tuple_at(&check.types, results, 0);
    assert_eq!(r.typ(&check.objects).unwrap(), t_param);
}

#[test]
fn generic_func_multiple_type_params() {
    // func Map[T, U any](s []T, f func(T) U) []U
    let (check, sig) = sig_of(
        "package p\nfunc Map[T, U any](s []T, f func(T) U) []U { return nil }\n",
        "Map",
    );
    let tps = guff_types::signature_type_params(&check.types, sig).expect("type params");
    assert_eq!(tps.len(), 2);
    assert_eq!(
        tuple_len(&check.types, signature_params(&check.types, sig)),
        2
    );
    assert_eq!(
        tuple_len(&check.types, signature_results(&check.types, sig)),
        1
    );
}

#[test]
fn generic_func_type_params_do_not_leak_to_package_scope() {
    // After building F[T any], `T` must not be visible at package scope.
    let (check, _sig) = sig_of("package p\nfunc F[T any](x T) T { return x }\n", "F");
    let pkg_scope = check.packages.get(check.pkg).scope();
    assert!(
        guff_types::scope_lookup(&check.scopes, pkg_scope, "T").is_none(),
        "type parameter T leaked into package scope"
    );
}

#[test]
fn generic_method_collects_receiver_type_params() {
    // func (b Box[T]) Get() T { ... } — the receiver declares type param T,
    // and the receiver type is the instantiated Box[T]; the result is T.
    let (check, sig) = sig_of(
        "package p\ntype Box[T any] struct { v T }\nfunc (b Box[T]) Get() T { return b.v }\n",
        "Get",
    );
    // The signature carries one receiver type parameter.
    let rparams = guff_types::signature::signature_recv_type_params(&check.types, sig)
        .expect("receiver type params");
    assert_eq!(rparams.len(), 1);

    // The receiver type is an instantiated Named (Box[T]).
    let recv = signature_recv(&check.types, sig).expect("receiver present");
    let rty = recv.typ(&check.objects).unwrap();
    assert_eq!(rty.kind(&check.types), TypeKind::Named);

    // The result type is the receiver type parameter T.
    let results = signature_results(&check.types, sig).unwrap();
    let r = guff_types::tuple::tuple_at(&check.types, results, 0);
    assert_eq!(r.typ(&check.objects).unwrap(), rparams.list()[0]);
}

#[test]
fn generic_method_pointer_receiver() {
    // func (b *Box[T]) Set(x T) { ... } — pointer receiver to instantiated Box[T].
    let (check, sig) = sig_of(
        "package p\ntype Box[T any] struct { v T }\nfunc (b *Box[T]) Set(x T) { b.v = x }\n",
        "Set",
    );
    let recv = signature_recv(&check.types, sig).expect("receiver present");
    let rty = recv.typ(&check.objects).unwrap();
    assert_eq!(rty.kind(&check.types), TypeKind::Pointer);
}

#[test]
fn generic_method_receiver_arity_mismatch_is_error() {
    // Box has one type parameter; the receiver declares two.
    let (check, _sig) = sig_of(
        "package p\ntype Box[T any] struct { v T }\nfunc (b Box[T, U]) M() {}\n",
        "M",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::BadRecv),
        "expected BadRecv, got: {:?}",
        check.errors
    );
}
