//! Tests for the `check_files` driver (chunk 32) — type-checking a whole
//! (small) package end-to-end. This is the first proof that the checker runs
//! collect → package_objects → process_delayed over real source.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::arena::ObjectData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
    check
}

#[test]
fn small_package_checks_without_errors() {
    let check = check_src(
        "package p\n\
         type T int\n\
         const c = 5\n\
         var x int = c\n\
         var y = x + 1\n\
         func f(a int) int { return a }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let pkg_scope = check.packages.get(check.pkg).scope();
    let lk = |n: &str| scope_lookup(&check.scopes, pkg_scope, n).expect(n);

    // T is a named type over int.
    let t = lk("T");
    assert!(matches!(check.objects.get(t), ObjectData::TypeName(_)));

    // c is a constant; x and y are typed variables; f is a func.
    assert!(matches!(check.objects.get(lk("c")), ObjectData::Const(_)));
    let x = lk("x");
    assert_eq!(
        x.typ(&check.objects).unwrap().kind(&check.types),
        TypeKind::Basic
    );
    let f = lk("f");
    assert_eq!(
        f.typ(&check.objects).unwrap().kind(&check.types),
        TypeKind::Signature
    );

    // The package is marked complete with its name set from the clause.
    assert_eq!(check.packages.get(check.pkg).name(), "p");
    assert!(check.packages.get(check.pkg).complete());
}

#[test]
fn forward_references_resolve() {
    // y refers to x (declared later); types collected before checking.
    let check = check_src("package p\nvar y = x\nvar x = 3\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn method_attached_during_check() {
    let check = check_src("package p\ntype T int\nfunc (t T) M() int { return 0 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    let t = scope_lookup(&check.scopes, pkg_scope, "T").unwrap();
    let tt = t.typ(&check.objects).unwrap();
    assert_eq!(
        guff_types::named::named_num_methods(&check.types, tt),
        1
    );
}

#[test]
fn type_error_is_reported() {
    // int8 cannot hold 1000.
    let check = check_src("package p\nconst c int8 = 1000\n");
    assert!(!check.errors.is_empty(), "expected an overflow error");
}

#[test]
fn function_body_is_checked() {
    // The body is checked via func_decl → later → func_body (chunk 30e). A
    // clean body (params/locals/return) yields no errors.
    let check = check_src(
        "package p\n\
         func f(a int, b int) int {\n\
         c := a + b\n\
         if c > 0 { c = c - 1 }\n\
         return c\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn function_body_type_error_is_reported() {
    // Returning a string from an int-returning function is a type error,
    // surfaced only because the body is now checked.
    let check = check_src("package p\nfunc f() int { return \"x\" }\n");
    assert!(!check.errors.is_empty(), "expected a return type error");
}

#[test]
fn function_literal_is_checked() {
    // A func literal assigned to a variable: its signature is built and its
    // body checked (chunk 31a). The body refers to the literal's parameter.
    let check = check_src("package p\nvar g = func(a int) int { return a + 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn function_literal_body_type_error_is_reported() {
    let check = check_src("package p\nvar g = func() int { return \"x\" }\n");
    assert!(
        !check.errors.is_empty(),
        "expected a return type error in func literal"
    );
}

#[test]
fn function_body_uses_package_level_names() {
    // The body refers to a package-level var declared after the function.
    let check = check_src("package p\nfunc f() int { return g }\nvar g int = 3\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn generic_function_body_references_type_param() {
    // func Id[T any](x T) T { var y T = x; return y } — the body refers to the
    // type parameter T, which must be in scope during body checking (chunk 35c).
    let check = check_src("package p\nfunc Id[T any](x T) T { var y T = x; return y }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn generic_call_inference_end_to_end() {
    // Calling a generic function infers its type argument; the result type
    // flows into a typed var declaration (chunk 35d).
    let check = check_src(
        "package p\nfunc Id[T any](x T) T { return x }\nvar a int = 5\nvar r int = Id(a)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn generic_call_inference_result_type_mismatch_is_reported() {
    // Id(a) infers T=int, so its result int cannot initialize a string var.
    let check = check_src(
        "package p\nfunc Id[T any](x T) T { return x }\nvar a int = 5\nvar r string = Id(a)\n",
    );
    assert!(
        !check.errors.is_empty(),
        "assigning inferred int result to string must error"
    );
}

#[test]
fn generic_method_declaration_end_to_end() {
    // A generic type with a method whose receiver declares the type parameter,
    // and whose body references it (chunk 35e).
    let check = check_src(
        "package p\ntype Box[T any] struct { v T }\nfunc (b Box[T]) Get() T { return b.v }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// chunk 36 — declared and not used (usage)

fn has_unused(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == guff_types_errors::Code::UnusedVar)
}

#[test]
fn unused_local_var_is_reported() {
    let check = check_src("package p\nfunc f() { var x int }\n");
    assert!(
        has_unused(&check),
        "expected 'declared and not used', got: {:?}",
        check.errors
    );
}

#[test]
fn used_local_var_is_ok() {
    let check = check_src("package p\nfunc f() int { var x int = 1; return x }\n");
    assert!(
        !has_unused(&check),
        "used var must not be flagged: {:?}",
        check.errors
    );
}

#[test]
fn unused_short_var_decl_is_reported() {
    let check = check_src("package p\nfunc f() { x := 1 }\n");
    assert!(
        has_unused(&check),
        "expected unused for `x := 1`, got: {:?}",
        check.errors
    );
}

#[test]
fn unused_params_and_results_are_not_reported() {
    // Parameters (and named results) are exempt from the usage check.
    let check = check_src("package p\nfunc f(a int, b int) (r int) { return }\n");
    assert!(
        !has_unused(&check),
        "params/results must not be flagged: {:?}",
        check.errors
    );
}

#[test]
fn unused_var_in_nested_block_is_reported() {
    let check = check_src("package p\nfunc f() { { var y int; _ = 0 } }\n");
    assert!(
        has_unused(&check),
        "expected unused in nested block, got: {:?}",
        check.errors
    );
}

#[test]
fn blank_assignment_uses_the_var() {
    // `_ = x` counts as a use of x.
    let check = check_src("package p\nfunc f() { var x int; _ = x }\n");
    assert!(
        !has_unused(&check),
        "`_ = x` must count as a use: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// chunk 37 — missing return (isTerminating)

fn has_missing_return(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == guff_types_errors::Code::MissingReturn)
}

#[test]
fn missing_return_in_function_with_results() {
    let check = check_src("package p\nfunc f() int { }\n");
    assert!(
        has_missing_return(&check),
        "expected MissingReturn, got: {:?}",
        check.errors
    );
}

#[test]
fn trailing_return_is_terminating() {
    let check = check_src("package p\nfunc f() int { return 1 }\n");
    assert!(
        !has_missing_return(&check),
        "return must terminate: {:?}",
        check.errors
    );
}

#[test]
fn no_results_needs_no_return() {
    let check = check_src("package p\nfunc f() { var _ = 1 }\n");
    assert!(
        !has_missing_return(&check),
        "no-result func needs no return: {:?}",
        check.errors
    );
}

#[test]
fn if_else_both_terminating_is_ok() {
    let check =
        check_src("package p\nfunc f(b bool) int { if b { return 1 } else { return 2 } }\n");
    assert!(
        !has_missing_return(&check),
        "if/else both return: {:?}",
        check.errors
    );
}

#[test]
fn if_without_else_is_not_terminating() {
    let check = check_src("package p\nfunc f(b bool) int { if b { return 1 } }\n");
    assert!(
        has_missing_return(&check),
        "if without else is not terminating: {:?}",
        check.errors
    );
}

#[test]
fn panic_call_is_terminating() {
    let check = check_src("package p\nfunc f() int { panic(\"unreachable\") }\n");
    assert!(
        !has_missing_return(&check),
        "panic terminates: {:?}",
        check.errors
    );
}

#[test]
fn infinite_for_is_terminating() {
    let check = check_src("package p\nfunc f() int { for {} }\n");
    assert!(
        !has_missing_return(&check),
        "`for {{}}` terminates: {:?}",
        check.errors
    );
}

#[test]
fn for_with_break_is_not_terminating() {
    let check = check_src("package p\nfunc f() int { for { break } }\n");
    assert!(
        has_missing_return(&check),
        "for-with-break is not terminating: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// chunk 38 — invalid recursive types (validType)

fn has_decl_cycle(check: &Checker) -> bool {
    check
        .errors
        .iter()
        .any(|e| e.code == guff_types_errors::Code::InvalidDeclCycle)
}

#[test]
fn direct_recursive_struct_is_invalid() {
    // type T struct { x T } has no finite layout.
    let check = check_src("package p\ntype T struct { x T }\n");
    assert!(
        has_decl_cycle(&check),
        "expected InvalidDeclCycle, got: {:?}",
        check.errors
    );
}

#[test]
fn recursion_through_pointer_is_ok() {
    // A pointer breaks the cycle: type T struct { next *T } is valid.
    let check = check_src("package p\ntype T struct { next *T }\n");
    assert!(
        !has_decl_cycle(&check),
        "pointer recursion is valid: {:?}",
        check.errors
    );
}

#[test]
fn recursion_through_slice_is_ok() {
    // A slice also breaks the cycle: type T struct { kids []T }.
    let check = check_src("package p\ntype T struct { kids []T }\n");
    assert!(
        !has_decl_cycle(&check),
        "slice recursion is valid: {:?}",
        check.errors
    );
}

#[test]
fn mutual_recursive_types_are_invalid() {
    // type A struct { b B }; type B struct { a A } — no finite layout.
    let check = check_src("package p\ntype A struct { b B }\ntype B struct { a A }\n");
    assert!(
        has_decl_cycle(&check),
        "expected InvalidDeclCycle, got: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// chunk 39 — label checking (labels.go)

fn has_code(check: &Checker, code: guff_types_errors::Code) -> bool {
    check.errors.iter().any(|e| e.code == code)
}

#[test]
fn used_break_label_is_ok() {
    let check = check_src("package p\nfunc f() {\nL:\nfor { break L }\n}\n");
    assert!(
        check.errors.is_empty(),
        "labeled break should be valid: {:?}",
        check.errors
    );
}

#[test]
fn unused_label_is_reported() {
    let check = check_src("package p\nfunc f() {\nL:\nfor { break }\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::UnusedLabel),
        "expected UnusedLabel, got: {:?}",
        check.errors
    );
}

#[test]
fn undeclared_goto_label_is_reported() {
    let check = check_src("package p\nfunc f() {\ngoto L\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::UndeclaredLabel),
        "expected UndeclaredLabel, got: {:?}",
        check.errors
    );
}

#[test]
fn duplicate_label_is_reported() {
    let check = check_src("package p\nfunc f() {\nL:\nfor { break L }\nL:\nfor { break L }\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::DuplicateLabel),
        "expected DuplicateLabel, got: {:?}",
        check.errors
    );
}

#[test]
fn misplaced_break_label_is_reported() {
    // L is on a (non-breakable) labeled block; `break L` from inside the loop
    // has no matching enclosing for/switch/select.
    let check = check_src("package p\nfunc f() {\nL:\n{ _ = 0 }\nfor { break L }\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::MisplacedLabel),
        "expected MisplacedLabel, got: {:?}",
        check.errors
    );
}

// ---------------------------------------------------------------------------
// chunk 40 — n:1 multi-value assignment (a, b := f())

#[test]
fn short_var_decl_from_multi_value_call() {
    let check = check_src(
        "package p\n\
         func two() (int, string) { return 0, \"\" }\n\
         func f() { a, b := two(); _ = a; _ = b }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn assignment_from_multi_value_call() {
    let check = check_src(
        "package p\n\
         func two() (int, string) { return 0, \"\" }\n\
         func f() { var a int; var b string; a, b = two(); _ = a; _ = b }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn multi_value_count_mismatch_is_reported() {
    let check = check_src(
        "package p\n\
         func two() (int, string) { return 0, \"\" }\n\
         func f() { a, b, c := two(); _ = a; _ = b; _ = c }\n",
    );
    assert!(
        has_code(&check, guff_types_errors::Code::WrongAssignCount),
        "expected WrongAssignCount, got: {:?}",
        check.errors
    );
}

#[test]
fn multi_value_type_mismatch_is_reported() {
    // a, b := two() infers a:int, b:string; assigning b (string) where int is
    // expected must error.
    let check = check_src(
        "package p\n\
         func two() (int, string) { return 0, \"\" }\n\
         func f() { a, b := two(); var c int = b; _ = a; _ = c }\n",
    );
    assert!(
        !check.errors.is_empty(),
        "expected a type error for `var c int = b`"
    );
}

#[test]
fn channel_receive_value() {
    // `v := <-ch` types v as the channel element type.
    let check = check_src("package p\nfunc f() { ch := make(chan int); v := <-ch; _ = v }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn comma_ok_map_index() {
    // `v, ok := m[k]` — v is the value type, ok is bool.
    let check = check_src(
        "package p\nfunc f() { m := make(map[string]int); v, ok := m[\"x\"]; _ = v; _ = ok }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn comma_ok_type_assertion() {
    // `v, ok := i.(int)` — v is int, ok is bool.
    let check = check_src("package p\nfunc f(i interface{}) { v, ok := i.(int); _ = v; _ = ok }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn comma_ok_ok_is_bool() {
    // The second value of a comma-ok must be assignable to bool, not int.
    let check = check_src(
        "package p\nfunc f(i interface{}) { v, ok := i.(int); var b int = ok; _ = v; _ = b }\n",
    );
    assert!(
        !check.errors.is_empty(),
        "the comma-ok `ok` is bool, not int"
    );
}

#[test]
fn comma_ok_channel_receive() {
    // `v, ok := <-ch` — v is the element type, ok is bool.
    let check = check_src(
        "package p\n\
         func f() { ch := make(chan int); v, ok := <-ch; var i int = v; var b bool = ok; _ = i; _ = b }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn comma_ok_channel_receive_ok_is_bool() {
    // The second value of `v, ok := <-ch` is bool, not int.
    let check = check_src(
        "package p\nfunc f() { ch := make(chan int); v, ok := <-ch; var b int = ok; _ = v; _ = b }\n",
    );
    assert!(
        !check.errors.is_empty(),
        "the channel-receive `ok` is bool, not int"
    );
}

#[test]
fn switch_duplicate_int_case() {
    // Two case clauses with the same constant int value are a DuplicateCase.
    let check = check_src("package p\nfunc f(x int) { switch x { case 1: case 1: } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| format!("{:?}", e.code).contains("DuplicateCase")),
        "expected DuplicateCase, got: {:?}",
        check.errors
    );
}

#[test]
fn switch_duplicate_case_within_one_clause() {
    // `case 2, 2:` — the same value twice inside one clause list.
    let check = check_src("package p\nfunc f(x int) { switch x { case 2, 2: } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| format!("{:?}", e.code).contains("DuplicateCase")),
        "expected DuplicateCase, got: {:?}",
        check.errors
    );
}

#[test]
fn switch_duplicate_string_case() {
    let check = check_src("package p\nfunc f(s string) { switch s { case \"a\": case \"a\": } }\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| format!("{:?}", e.code).contains("DuplicateCase")),
        "expected DuplicateCase, got: {:?}",
        check.errors
    );
}

#[test]
fn switch_distinct_cases_ok() {
    // Distinct constant cases must not be flagged.
    let check = check_src("package p\nfunc f(x int) { switch x { case 1: case 2: case 3: } }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn switch_non_constant_cases_not_deduped() {
    // Non-constant case values (variables) are never duplicate-checked, even
    // when they are the same expression.
    let check = check_src("package p\nfunc f(x, y int) { switch x { case y: case y: } }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn go_and_defer_calls_check_cleanly() {
    // Now that the parser accepts `go f()`/`defer f()` (chunk 87), the checker
    // runs them through `suspended_call` → `call_expr` without error.
    let check = check_src("package p\nfunc f() {}\nfunc g() {\n\tgo f()\n\tdefer f()\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn defer_conversion_is_rejected() {
    // `defer T(x)` is a conversion, not a call (chunk 88, exprKind).
    let check = check_src("package p\nfunc g(x int) {\n\tdefer int(x)\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::InvalidDefer),
        "expected InvalidDefer, got: {:?}",
        check.errors
    );
}

#[test]
fn go_conversion_is_rejected() {
    let check = check_src("package p\nfunc g(x int) {\n\tgo int(x)\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::InvalidGo),
        "expected InvalidGo, got: {:?}",
        check.errors
    );
}

#[test]
fn defer_expression_builtin_discards_result() {
    // `defer len(s)` — an expression-valued builtin's result is discarded.
    let check = check_src("package p\nfunc g(s []int) {\n\tdefer len(s)\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::UnusedResults),
        "expected UnusedResults, got: {:?}",
        check.errors
    );
}

#[test]
fn statement_builtin_defer_checks_cleanly() {
    // `panic`/`close` are statement builtins — valid as a `defer` target.
    let check = check_src("package p\nfunc g() {\n\tdefer panic(\"x\")\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn conversion_in_statement_position_is_unused() {
    // `int(x)` as a statement is a conversion, not a call — its value is unused.
    // (The old syntactic call-check missed this.)
    let check = check_src("package p\nfunc g(x int) {\n\tint(x)\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::UnusedExpr),
        "expected UnusedExpr, got: {:?}",
        check.errors
    );
}

#[test]
fn expression_builtin_in_statement_position_is_unused() {
    // `len(s)` as a statement discards its value.
    let check = check_src("package p\nfunc g(s []int) {\n\tlen(s)\n}\n");
    assert!(
        has_code(&check, guff_types_errors::Code::UnusedExpr),
        "expected UnusedExpr, got: {:?}",
        check.errors
    );
}

#[test]
fn call_and_statement_builtin_in_statement_position_ok() {
    // An ordinary call and a statement-builtin call are valid statements.
    let check = check_src(
        "package p\nfunc f() {}\nfunc g(m map[int]int) {\n\tf()\n\tclear(m)\n\tprintln(1)\n}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}
