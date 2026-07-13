//! Tests for statement type-checking (`stmt.rs`).
//!
//! Chunk 30a-1: the dispatch skeleton, scope helpers, and the `ExprStmt` case.

use guff::ast::{Decl, Stmt};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::arena::ObjectData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::stmt::StmtContext;
use guff_types::{Checker, Config};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

/// Build a checker whose current environment scope is the package scope, and
/// return it alongside the body statements of the first function declaration.
fn checker_with_body(src: &str) -> (Checker, Vec<Stmt>) {
    let file = parse(src);
    let mut body = Vec::new();
    for d in &file.decls {
        if let Decl::FuncDecl(f) = d {
            if let Some(b) = &f.body {
                body = b.list.clone();
                break;
            }
        }
    }
    let mut check = Checker::new(Config::default());
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    (check, body)
}

#[test]
fn open_close_scope_round_trips() {
    let mut check = Checker::new(Config::default());
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);

    check.open_scope(0, 0, "block");
    let inner = check.env.scope.expect("inner scope");
    assert_ne!(inner, pkg_scope);
    assert_eq!(check.scopes.get(inner).parent(), Some(pkg_scope));

    check.close_scope();
    assert_eq!(check.env.scope, Some(pkg_scope));
}

#[test]
fn empty_and_bad_statements_are_ignored() {
    let (mut check, _body) = checker_with_body("package p\nfunc f() {}\n");
    // An empty statement list checks cleanly.
    check.stmt_list(StmtContext::EMPTY, &[]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn bare_constant_expression_is_not_used() {
    // `1 + 1` evaluated as a statement is a constant, not a call → UnusedExpr.
    let (mut check, body) = checker_with_body("package p\nfunc f() { 1 + 1 }\n");
    assert_eq!(body.len(), 1);
    check.stmt(StmtContext::EMPTY, &body[0]);
    assert_eq!(check.errors.len(), 1, "errors: {:?}", check.errors);
    assert_eq!(
        check.errors[0].code,
        guff_types_errors::Code::UnusedExpr
    );
}

/// Run every statement in the first func body in `src`, with the package scope
/// as the current environment scope.
fn run_body(src: &str) -> Checker {
    let (mut check, body) = checker_with_body(src);
    for s in &body {
        check.stmt(StmtContext::EMPTY, s);
    }
    check
}

#[test]
fn local_var_decl_declares_typed_variable() {
    let check = run_body("package p\nfunc f() { var x int = 5 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    let x = scope_lookup(&check.scopes, pkg_scope, "x").expect("x declared");
    assert!(matches!(check.objects.get(x), ObjectData::Var(_)));
}

#[test]
fn local_const_and_type_decls() {
    let check = run_body("package p\nfunc f() { const c = 5\ntype T int }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    assert!(matches!(
        check
            .objects
            .get(scope_lookup(&check.scopes, pkg_scope, "c").unwrap()),
        ObjectData::Const(_)
    ));
    assert!(matches!(
        check
            .objects
            .get(scope_lookup(&check.scopes, pkg_scope, "T").unwrap()),
        ObjectData::TypeName(_)
    ));
}

#[test]
fn short_var_decl_then_assignment() {
    let check = run_body("package p\nfunc f() { x := 1\nx = 2 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    assert!(matches!(
        check
            .objects
            .get(scope_lookup(&check.scopes, pkg_scope, "x").unwrap()),
        ObjectData::Var(_)
    ));
}

#[test]
fn compound_assignment_is_checked() {
    let check = run_body("package p\nfunc f() { x := 1\nx += 2 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn short_var_decl_with_no_new_vars_errors() {
    // Second `:=` introduces no new variable (x already in scope).
    let check = run_body("package p\nfunc f() { x := 1\nx := 2 }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::NoNewVar),
        "expected NoNewVar, got: {:?}",
        check.errors
    );
}

#[test]
fn assignment_to_non_addressable_errors() {
    // `1 = 2` — the lhs is not addressable.
    let check = run_body("package p\nfunc f() { 1 = 2 }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::UnassignableOperand),
        "expected UnassignableOperand, got: {:?}",
        check.errors
    );
}

#[test]
fn inc_dec_on_numeric_is_ok() {
    let check = run_body("package p\nfunc f() { x := 1\nx++ }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn inc_dec_on_non_numeric_errors() {
    let check = run_body("package p\nfunc f() { s := \"a\"\ns++ }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::NonNumericIncDec),
        "expected NonNumericIncDec, got: {:?}",
        check.errors
    );
}

#[test]
fn send_to_channel_is_ok() {
    let check = run_body("package p\nfunc f() { ch := make(chan int)\nch <- 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn send_to_non_channel_errors() {
    let check = run_body("package p\nfunc f() { x := 1\nx <- 1 }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidSend),
        "expected InvalidSend, got: {:?}",
        check.errors
    );
}

#[test]
fn if_for_block_check_cleanly() {
    let check = run_body(
        "package p\nfunc f() {\n\
         x := 1\n\
         if x > 0 { x = 2 } else { x = 3 }\n\
         for i := 0; i < 10; i++ { x = i }\n\
         { y := x; _ = y }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn non_boolean_if_condition_errors() {
    let check = run_body("package p\nfunc f() { if 1 { } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidCond),
        "expected InvalidCond, got: {:?}",
        check.errors
    );
}

#[test]
fn non_boolean_for_condition_errors() {
    let check = run_body("package p\nfunc f() { for 1 { } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidCond),
        "expected InvalidCond, got: {:?}",
        check.errors
    );
}

#[test]
fn declare_in_post_statement_errors() {
    let check = run_body("package p\nfunc f() { for i := 0; i < 1; j := 2 { _ = i } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidPostDecl),
        "expected InvalidPostDecl, got: {:?}",
        check.errors
    );
}

#[test]
fn expression_switch_checks_cleanly() {
    let check = run_body(
        "package p\nfunc f() {\n\
         x := 2\n\
         switch x {\n\
         case 1: x = 10\n\
         case 2, 3: x = 20\n\
         default: x = 0\n\
         }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn tagless_switch_checks_cleanly() {
    let check = run_body(
        "package p\nfunc f() {\n\
         x := 2\n\
         switch {\n\
         case x > 0: x = 1\n\
         default: x = 0\n\
         }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn switch_multiple_defaults_errors() {
    let check = run_body(
        "package p\nfunc f() {\n\
         switch 1 {\n\
         default: \n\
         default: \n\
         }\n\
         }\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateDefault),
        "expected DuplicateDefault, got: {:?}",
        check.errors
    );
}

/// Build a checker, construct the first function's signature, set it as the
/// active `env.sig`, and run the body statements — mirroring what `funcBody`
/// will eventually do. Returns the checker for error inspection.
fn run_func(src: &str) -> Checker {
    let file = parse(src);
    let mut check = Checker::new(Config::default());
    let pkg_scope = check.packages.get(check.pkg).scope();
    check.env.scope = Some(pkg_scope);
    for d in &file.decls {
        if let Decl::FuncDecl(f) = d {
            let sig = check.func_type(f.recv.as_ref(), &f.ty);
            check.env.sig = Some(sig);
            if let Some(b) = &f.body {
                for s in &b.list {
                    check.stmt(StmtContext::EMPTY, s);
                }
            }
            break;
        }
    }
    check
}

#[test]
fn return_matching_result_is_ok() {
    let check = run_func("package p\nfunc f() int { return 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn return_wrong_type_errors() {
    let check = run_func("package p\nfunc f() int { return \"x\" }\n");
    assert!(!check.errors.is_empty(), "expected a type error on return");
}

#[test]
fn empty_return_with_no_results_is_ok() {
    let check = run_func("package p\nfunc f() { return }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn range_over_slice_declares_key_value() {
    let check = run_body(
        "package p\nfunc f() {\n\
         s := []int{}\n\
         for i, v := range s { _ = i\n_ = v }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn range_over_map_declares_key_value() {
    let check = run_body(
        "package p\nfunc f() {\n\
         m := map[string]int{}\n\
         for k, v := range m { _ = k\n_ = v }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn range_over_non_iterable_errors() {
    let check = run_body(
        "package p\nfunc f() {\n\
         x := 1.5\n\
         for i := range x { _ = i }\n\
         }\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::InvalidRangeExpr),
        "expected InvalidRangeExpr, got: {:?}",
        check.errors
    );
}

#[test]
fn break_outside_loop_errors() {
    let check = run_body("package p\nfunc f() { break }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::MisplacedBreak),
        "expected MisplacedBreak, got: {:?}",
        check.errors
    );
}

#[test]
fn continue_inside_for_is_ok() {
    let check = run_body("package p\nfunc f() { for { continue } }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn select_with_receive_checks_cleanly() {
    let check = run_body(
        "package p\nfunc f() {\n\
         ch := make(chan int)\n\
         select {\n\
         case v := <-ch: _ = v\n\
         default:\n\
         }\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn type_expression_statement_is_not_an_expr() {
    // `int` used as a statement is a type expression → NotAnExpr.
    let (mut check, body) = checker_with_body("package p\nfunc f() { int }\n");
    assert_eq!(body.len(), 1);
    check.stmt(StmtContext::EMPTY, &body[0]);
    assert_eq!(check.errors.len(), 1, "errors: {:?}", check.errors);
    assert_eq!(
        check.errors[0].code,
        guff_types_errors::Code::NotAnExpr
    );
}
