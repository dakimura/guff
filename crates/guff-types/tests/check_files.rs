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
fn switch_byte_escape_and_code_point_are_distinct_cases() {
    // "\xff" is one byte and "\u00ff" is the two bytes of U+00FF, so this
    // switch has no duplicate. Decoding the byte escape into a code point made
    // both cases the same string and reported one — see the map-literal twin
    // in tests/literals.rs.
    let check = check_src(
        "package p\nfunc f(s string) { switch s { case \"\\xff\": case \"\\u00ff\": } }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
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


#[test]
fn flag_map_value_implements_interface_var_before_methods() {
    // Regression: consul `command/flags` declares
    // `var _ flag.Value = (*FlagMapValue)(nil)` *before* the Set/String methods.
    // Implements must force obj_decl on those methods (Go missingMethod).
    let check = check_src(
        "package p\n\
         type error interface { Error() string }\n\
         type Value interface {\n\
         \tString() string\n\
         \tSet(string) error\n\
         }\n\
         var _ Value = (*FlagMapValue)(nil)\n\
         type FlagMapValue map[string]string\n\
         func (h *FlagMapValue) String() string { return \"\" }\n\
         func (h *FlagMapValue) Set(value string) error { return nil }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn bidirectional_chan_comparable_to_recv_only() {
    // Spec: first operand assignable to second's type, or vice versa.
    // `chan T` is assignable to `<-chan T`, so `w != d` is valid (vault fairshare).
    let check = check_src(
        "package p\n\
         type T struct{}\n\
         func f(w <-chan T, d chan T) bool { return w != d }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn embedded_struct_and_iface_implements_composed_interface() {
    // restic internal/fs: *readerFile embeds io.ReadCloser + fakeFile and must
    // satisfy File (which embeds io.Reader + io.Closer).
    let check = check_src(
        r#"
package p

type Reader interface { Read(p []byte) (int, error) }
type Closer interface { Close() error }
type ReadCloser interface {
	Reader
	Closer
}

type File interface {
	MakeReadable() error
	Reader
	Closer
	Stat() (string, error)
}

type fakeFile struct{ name string }

func (f fakeFile) MakeReadable() error { return nil }
func (f fakeFile) Read(_ []byte) (int, error) { return 0, nil }
func (f fakeFile) Close() error { return nil }
func (f fakeFile) Stat() (string, error) { return f.name, nil }

var _ File = fakeFile{}

type readerFile struct {
	ReadCloser
	fakeFile
}

func (r *readerFile) Read(p []byte) (int, error) { return r.ReadCloser.Read(p) }
func (r *readerFile) Close() error { return r.ReadCloser.Close() }

var _ File = &readerFile{}
"#,
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

// ----------------------------------------------------------------------------
// R26 — false "ill-typed" packages found on a large real-world corpus.

/// A method body may name the *receiver's* type parameters. `func_body` builds
/// a fresh scope, so they have to be re-declared into it (Go reuses the
/// signature's own scope, which `collectRecv` already declared them into).
#[test]
fn method_body_sees_receiver_type_params() {
    let check = check_src(
        "package p\n\
         type Loader[T any] struct{ def *T }\n\
         func (l *Loader[T]) New() *T { v := new(T); return v }\n\
         func (l *Loader[T]) Zero() T { var z T; return z }\n\
         type Box[K comparable, R any] struct{ m map[K]R }\n\
         func (b *Box[K, R]) Make() []R { return make([]R, 0) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

/// An interface's type set must not be computed while an embedded named type
/// is still mid-declaration: the methods promoted through it would be lost and
/// the (cached) result would never be recomputed.
#[test]
fn embedded_interface_methods_are_promoted_across_declaration_cycles() {
    let check = check_src(
        "package p\n\
         type Descriptor interface { FullName() string; Parent() Descriptor }\n\
         type isMethod interface { ProtoType(MethodDescriptor) }\n\
         type MethodDescriptor interface { Descriptor; Streaming() bool; isMethod }\n\
         func use(md MethodDescriptor) string { return md.FullName() }\n\
         func widen(md MethodDescriptor) Descriptor { return md }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

/// A conversion whose target is written as a bare type literal is still a
/// conversion — it used to leave the operand invalid, which was swallowed
/// silently (no type recorded for the node, no diagnostics).
#[test]
fn conversions_to_type_literals_are_checked() {
    let check = check_src(
        "package p\n\
         type R interface{ Resolve() string }\n\
         func a(s string) []byte { return []byte(s) }\n\
         func b(m map[string]int) map[string]int { return map[string]int(m) }\n\
         func c(f func()) func() { return (func())(f) }\n\
         func d(x int) (R, bool) { r, ok := interface{}(x).(R); return r, ok }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // ... and the conversion's type is now known, so a bad use is reported.
    //
    // `[]byte`, not `[]uint8`: `byte` is its own predeclared Basic with its own
    // name (go/types' `aliases` array), so a type written `[]byte` renders that
    // way. Go says "cannot use []byte(s) (value of type []byte) as int value in
    // variable declaration" here; this asserted `[]uint8` while the two
    // spellings shared one Basic.
    let bad = check_src("package p\nfunc a(s string) { var z int = []byte(s); _ = z }\n");
    assert!(
        bad.errors.iter().any(|e| e.msg.contains("[]byte")),
        "expected a type error mentioning []byte, got: {:?}",
        bad.errors
    );
}

/// Calling, indexing and slicing a value whose type is a type parameter uses
/// the common underlying type of its type set.
#[test]
fn type_param_operands_use_the_common_underlying_type() {
    let check = check_src(
        "package p\n\
         type Conn interface{ Do() }\n\
         func New[T any, F func(Conn) T](fn F, c Conn) T { return fn(c) }\n\
         func Head[S ~[]E, E any](s S) E { return s[0] }\n\
         func Tail[S ~[]E, E any](s S) S { return s[1:] }\n\
         func Get[M ~map[K]V, K comparable, V any](m M, k K) (V, bool) { v, ok := m[k]; return v, ok }\n\
         func Idx[S ~string](s S) byte { return s[0] }\n\
         func Cut[S ~string](s S) S { return s[1:] }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // A type set whose members are not indexable in the same way is rejected.
    let bad = check_src(
        "package p\nfunc f[T ~[]int | ~map[string]int](t T) int { return t[0] }\n",
    );
    assert!(
        bad.errors.iter().any(|e| e.msg.contains("cannot index")),
        "expected 'cannot index', got: {:?}",
        bad.errors
    );
}

/// go1.26 `new(expr)`: `new` accepts a value as well as a type.
#[test]
fn new_accepts_a_value_argument() {
    let check = check_src(
        "package p\n\
         type S struct{ Name string }\n\
         func a(s S) *string { return new(s.Name) }\n\
         func b() *bool { return new(true) }\n\
         func c() *int { return new(int) }\n\
         func d() *int { x := 3; return new(x) }\n\
         func e() *[]int { return new([]int) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // `new` of a non-value is still an error, and the probe that classifies the
    // argument must not leave its "is not a type" diagnostic behind.
    let bad = check_src("package p\nfunc f() { _ = new(nil) }\n");
    assert!(
        !bad.errors.is_empty(),
        "new(nil) should be rejected"
    );
    assert!(
        !bad.errors.iter().any(|e| e.msg.contains("is not a type")),
        "the type probe leaked a diagnostic: {:?}",
        bad.errors
    );
}

/// An array length may be any constant expression, not just an integer
/// literal — `const n = 20; type t struct{ a [n]byte }`.
#[test]
fn array_length_accepts_constant_expressions() {
    let check = check_src(
        "package p\n\
         const m = 20\n\
         type k struct{ pcs [m]uintptr }\n\
         func f(x k) []uintptr { return x.pcs[:] }\n\
         func g() []uintptr { var a [2 * 10]uintptr; return a[:] }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    for (src, want) in [
        ("package p\nconst m = -1\nvar a [m]int\n", "invalid array length"),
        ("package p\nvar v = 3\nvar a [v]int\n", "invalid array length"),
        (
            "package p\nconst m = \"x\"\nvar a [m]int\n",
            "must be integer",
        ),
    ] {
        let bad = check_src(src);
        assert!(
            bad.errors.iter().any(|e| e.msg.contains(want)),
            "expected {want:?} for {src:?}, got: {:?}",
            bad.errors
        );
    }
}

/// A lone multi-valued argument is spread across the parameters:
/// `f(g())` where `g` returns exactly what `f` takes.
#[test]
fn single_multi_valued_argument_is_spread() {
    let check = check_src(
        "package p\n\
         func pair() (int, string) { return 0, \"\" }\n\
         func take(a int, b string) {}\n\
         func f() { take(pair()) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // The spread values are still assignment-checked against the parameters.
    let bad = check_src(
        "package p\n\
         func pair() (int, string) { return 0, \"\" }\n\
         func take(a int, b int) {}\n\
         func f() { take(pair()) }\n",
    );
    assert!(
        bad.errors
            .iter()
            .any(|e| e.msg.contains("cannot use string value as int value")),
        "expected a per-value assignment error, got: {:?}",
        bad.errors
    );
}

/// A generic type instantiated **inside another type's declaration**. Verifying
/// the constraint needs the type argument's methods, and a method's signature is
/// resolved by its own object declaration — which has not run yet at that point.
/// Go defers the check (`typexpr.go`'s `check.later(func() { … verify … })`);
/// guff ran it inline and read `*handler`'s `Serve` before it had a type, so the
/// error read "wrong type for method Serve; have <nothing>".
///
/// The same shape in a value position was always fine, because function bodies
/// are checked last. syncthing's `serviceMap[string, *indexHandler]` is the
/// struct-field spelling, and it made guff call the whole of `lib/model`
/// ill-typed.
#[test]
fn constraint_is_verified_after_methods_resolve() {
    let check = check_src(
        "package p\n\
         type service interface{ serve() error }\n\
         type box[S service] struct{ v S }\n\
         type handler struct{ n int }\n\
         func (h *handler) serve() error { return nil }\n\
         type registry struct{ h *box[*handler] }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// The constraint really is checked — deferring it must not turn it off.
#[test]
fn constraint_violation_is_still_reported_when_deferred() {
    let check = check_src(
        "package p\n\
         type service interface{ serve() error }\n\
         type box[S service] struct{ v S }\n\
         type handler struct{ n int }\n\
         type registry struct{ h *box[*handler] }\n",
    );
    assert!(
        check.errors.iter().any(|e| e.msg.contains("serve")),
        "expected a missing-method error, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// `append` asks go/types for `coreType(S)`, not `under(S)`: a type parameter's
/// underlying type is its constraint interface, and only its *type set* says
/// whether every member is a slice. syncthing's
/// `func without[E comparable, S ~[]E](s S, e E) S` is the shape that makes the
/// difference — `clear` and `delete` had the same gap.
#[test]
fn builtins_accept_a_type_parameter_constrained_to_a_slice() {
    let check = check_src(
        "package p\n\
         func without[E comparable, S ~[]E](s S, e E) S {\n\
         \tfor i, x := range s {\n\
         \t\tif x == e {\n\
         \t\t\treturn append(s[:i], s[i+1:]...)\n\
         \t\t}\n\
         \t}\n\
         \treturn s\n\
         }\n\
         func wipe[S ~[]int](s S) { clear(s) }\n\
         func drop[K comparable, M ~map[K]int](m M, k K) { delete(m, k) }\n\
         func shut[C ~chan int](c C) { close(c) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// Converting an untyped constant to an interface does not ask whether the
/// constant is *representable* as one — nothing is. Go converts through the
/// operand's **default type** and accepts when the interface is the empty one
/// (`implicitTypeAndValue`'s `*Interface` arm). `any("key")` is that
/// conversion, and rejecting it took gitea's `cmd/cmdtest` and cli's
/// `pkg/cmd/pr/list` with it.
///
/// The predicate matters as much as the arm: `Interface.Empty()` is
/// `typeSet().IsAll()` — the set of *all* types — while `is_empty()` is the set
/// nothing satisfies. With the wrong one only the `interface{}` literal passed,
/// because its type set had not been computed yet.
#[test]
fn an_untyped_constant_converts_to_an_empty_interface() {
    let check = check_src(
        "package p\n\
         type myany = interface{}\n\
         type MyAny interface{}\n\
         func f() {\n\
         \t_ = interface{}(\"a\")\n\
         \t_ = myany(\"b\")\n\
         \t_ = MyAny(\"c\")\n\
         \t_ = any(\"d\")\n\
         \t_ = any(1)\n\
         \t_ = any(nil)\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// A *non-empty* interface still refuses: an untyped constant has no methods,
/// so there is nothing for it to implement, and `go build` says so too.
#[test]
fn an_untyped_constant_does_not_convert_to_a_non_empty_interface() {
    let check = check_src(
        "package p\n\
         type Stringer interface{ String() string }\n\
         func f() { _ = Stringer(\"e\") }\n",
    );
    assert!(
        check.errors.iter().any(|e| e.msg.contains("cannot convert")),
        "expected a conversion error, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// Inference step 3 promotes an untyped argument to its default type, and
/// "untyped" is not "untyped *constant*": a comparison yields an untyped
/// **bool value**, which has a default type like any other untyped operand.
/// Requiring a constant dropped it from step 3 as well as step 1, so
/// `optional.Some(n > 0)` against `func Some[T any](v T) Option[T]` could not
/// infer `T` at all — and the package went ill-typed for it. Three of gitea's
/// were this line.
#[test]
fn inference_learns_from_an_untyped_value_that_is_not_a_constant() {
    let check = check_src(
        "package p\n\
         type Option[T any] struct{ v T }\n\
         func Some[T any](v T) Option[T] { return Option[T]{v} }\n\
         func f(n int64, s string) {\n\
         \t_ = Some(n > 0)\n\
         \t_ = Some(s == \"x\")\n\
         \t_ = Some(1)\n\
         \t_ = Some(\"x\")\n\
         \t_ = Some(n)\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// Untyped **nil** is the one untyped operand with no default type, and it must
/// still contribute nothing — to step 1, so another argument can supply the
/// type parameter, and to step 3, so it cannot invent one.
#[test]
fn untyped_nil_still_contributes_nothing_to_inference() {
    let ok = check_src(
        "package p\n\
         type T struct{}\n\
         func kind[O any](o O, extra any) O { return o }\n\
         func f() { _ = kind(&T{}, nil) }\n",
    );
    assert!(
        ok.errors.is_empty(),
        "expected no errors, got {:?}",
        ok.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );

    // With nothing else to learn from, inference still fails.
    let bad = check_src(
        "package p\n\
         func only[T any](v T) T { return v }\n\
         func g() { _ = only(nil) }\n",
    );
    assert!(!bad.errors.is_empty(), "expected inference to fail on a lone nil");
}

/// `T[A, B](v)` is a conversion to an instantiated generic type, and it took
/// the generic-*function* path because nothing else handled a multi-index
/// callee. The single-argument form `T[A](v)` was fine — it goes through
/// `index_expr`, which instantiates — so the bug only ever showed on two or
/// more type arguments. `iter.Seq2[[]ptrace.Traces, error](fn)` is one line in
/// nine of jaeger's packages, and each one went ill-typed for it.
#[test]
fn a_conversion_to_a_generic_type_may_take_several_type_arguments() {
    let check = check_src(
        "package p\n\
         type F1[T any] func(T)\n\
         type F2[K, V any] func(K, V)\n\
         type F3[A, B, C any] func(A, B, C)\n\
         func f() {\n\
         \t_ = F1[int](func(int) {})\n\
         \t_ = F2[int, string](func(int, string) {})\n\
         \t_ = F3[int, string, bool](func(int, string, bool) {})\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// And what the multi-index branch was written for still works: explicit
/// instantiation of a generic *function* in call position, including the
/// partial form the inference has to finish.
#[test]
fn explicit_instantiation_of_a_generic_function_still_works() {
    let check = check_src(
        "package p\n\
         func pair[K, V any](k K, v V) (K, V) { return k, v }\n\
         func f() {\n\
         \t_, _ = pair[int, string](1, \"a\")\n\
         \t_, _ = pair[int](1, \"a\")\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );

    // A multi-index on an operand that is neither a generic function nor a
    // generic type is still an error — the branch's original job.
    let bad = check_src(
        "package p\n\
         func plain(i int) int { return i }\n\
         func g() { _ = plain[int, string](1) }\n",
    );
    assert!(
        bad.errors.iter().any(|e| e.msg.contains("cannot index")),
        "expected a 'cannot index', got {:?}",
        bad.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// A pointer indirection is addressable whatever the pointer expression was —
/// the spec lists it beside "a variable", and go/types sets `x.mode = variable`
/// unconditionally. guff required the *operand* to be addressable, so a deref
/// of a conversion or of a call result could not be assigned to. thanos,
/// argo-cd and cli have five ill-typed packages between them from this.
#[test]
fn a_pointer_indirection_is_always_addressable() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         type S struct{ V float64 }\n\
         func at(s *S) *float64 { return &s.V }\n\
         func f(ptr unsafe.Pointer, s *S, ss []*S) {\n\
         \t*(*S)(ptr) = S{V: 1}\n\
         \t*at(s) = 2\n\
         \t*ss[0] = S{V: 3}\n\
         \t(*s).V = 4\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// Addressability is not blanket, though: what is *not* a pointer indirection
/// still cannot be assigned to.
#[test]
fn assigning_to_something_that_is_not_addressable_is_still_an_error() {
    let check = check_src(
        "package p\n\
         type S struct{ V int }\n\
         func mk() S { return S{} }\n\
         func f(m map[string]S) {\n\
         \tmk().V = 1\n\
         \tm[\"k\"].V = 2\n\
         }\n",
    );
    assert_eq!(
        check
            .errors
            .iter()
            .filter(|e| e.msg.contains("cannot assign to"))
            .count(),
        2,
        "got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// `(*p)()` — a call through a variable of pointer-to-func type — is
/// syntactically indistinguishable from the pointer conversion `(*T)(x)`, so
/// `exprOrType` probes it as a type first. The probe *reports* on the way, and
/// its "p is not a type" survived even though the value path then handled the
/// call correctly: `go build` accepted the file and guff called the package
/// ill-typed, which is not a finding-set difference — only every analyzer
/// going quiet (`compat/health.py`). rclone's `lib/atexit` and `fs/rc/jobs`
/// are two packages of this shape.
#[test]
fn a_call_through_a_pointer_to_func_is_not_a_conversion() {
    let check = check_src(
        "package p\n\
         var fn *func()\n\
         var fns = map[*func()]bool{}\n\
         func run() {\n\
         \t(*fn)()\n\
         \tfor h := range fns {\n\
         \t\t(*h)()\n\
         \t}\n\
         }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// And the conversion it is confused with still works, still reports when its
/// target does not exist, and reports it **once**.
#[test]
fn a_pointer_conversion_is_still_a_conversion() {
    let ok = check_src(
        "package p\n\
         type T int\n\
         func f(x *int) *T { return (*T)(x) }\n",
    );
    assert!(
        ok.errors.is_empty(),
        "expected no errors, got {:?}",
        ok.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );

    let bad = check_src("package p\nfunc f(x *int) { _ = (*Nope)(x) }\n");
    let msgs: Vec<&String> = bad.errors.iter().map(|e| &e.msg).collect();
    assert_eq!(
        msgs.iter().filter(|m| m.contains("Nope")).count(),
        1,
        "expected exactly one report, got {msgs:?}"
    );
}

/// `len` / `cap` ask the same question, and the answer is `underIs`, not
/// `coreType`: every term of the type set has to be lengthable, but they need
/// not agree on *what* they are. syncthing's `lib/sliceutil` is three `len(s)`
/// on an `S ~[]E`, and while they were rejected the package was ill-typed —
/// which is not a finding-set difference, only five linters going quiet
/// (`compat/health.py`).
#[test]
fn len_and_cap_accept_a_type_parameter_whose_terms_are_all_lengthable() {
    let check = check_src(
        "package p\n\
         func removeAndZero[E any, S ~[]E](s S, i int) S {\n\
         \tcopy(s[i:], s[i+1:])\n\
         \ts[len(s)-1] = *new(E)\n\
         \treturn s[:len(s)-1]\n\
         }\n\
         func room[S ~[]int](s S) int { return cap(s) }\n\
         func size[M ~map[string]int](m M) int { return len(m) }\n\
         func chars[T ~string](t T) int { return len(t) }\n\
         func queued[C ~chan int](c C) int { return len(c) + cap(c) }\n\
         func mixed[S ~[]int | ~[]string](s S) int { return len(s) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// `underIs` is weaker than `commonUnder` but it is not "anything goes": a set
/// with one term that has no length still fails, and `cap` refuses a map even
/// though `len` accepts one.
#[test]
fn len_and_cap_still_reject_terms_that_have_no_length() {
    let check = check_src(
        "package p\n\
         func bad[S ~[]int | ~int](s S) int { return len(s) }\n\
         func capMap[M ~map[string]int](m M) int { return cap(m) }\n\
         func anySet[T any](t T) int { return len(t) }\n",
    );
    let msgs: Vec<&String> = check.errors.iter().map(|e| &e.msg).collect();
    assert_eq!(
        msgs.iter()
            .filter(|m| m.contains("for built-in len") || m.contains("for built-in cap"))
            .count(),
        3,
        "expected three rejections, got {msgs:?}"
    );
}

/// And a type parameter whose type set is *not* all slices still fails.
#[test]
fn append_rejects_a_type_parameter_that_is_not_all_slices() {
    let check = check_src(
        "package p\n\
         func bad[S ~[]int | ~map[int]int](s S) S { return append(s, 1) }\n",
    );
    assert!(
        check.errors.iter().any(|e| e.msg.contains("not a slice")),
        "expected an append error, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// A generic instance reached through an embedded field has to have its
/// methods expanded too. `expand_instance_methods` stopped at the outer type,
/// so `noCtor` promoted `Get` out of `Client`'s *origin* — signature
/// `func(string) (T, error)`, `T` unsubstituted — and the interface comparison
/// called it a wrong signature. The shape is the k8s generated client
/// (`gentype.ClientWithListAndApply[...]` embedded in a wrapper struct):
/// traefik, argo-cd and prometheus, 5 ill-typed packages.
///
/// The two halves have to be checked as **separate packages**. The instance is
/// shared per (origin, type arguments), so the control half cures the subject
/// if they sit in one package — which is how the bug hid for so long, and made
/// the first single-package version of this fixture pass unfixed.
#[test]
fn embedded_generic_instance_promotes_substituted_methods() {
    const PRELUDE: &str = "package p\n\
         type Route struct{ N int }\n\
         type Client[T any] struct{ name string }\n\
         func (c *Client[T]) Get(name string) (T, error) { var z T; return z, nil }\n\
         func NewClient[T any](name string) *Client[T] { return &Client[T]{name: name} }\n\
         type OnlyGet interface{ Get(name string) (*Route, error) }\n\
         type wrap struct{ *Client[*Route] }\n\
         func a() OnlyGet { return &wrap{} }\n\
         func b(n *wrap) *Route { r, _ := n.Get(\"x\"); return r }\n";

    // Nothing else in the package ever names `Client[*Route]` in an expression.
    let alone = check_src(PRELUDE);
    assert!(
        alone.errors.is_empty(),
        "expected no errors, got {:?}",
        alone.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );

    // The control: an interface check on `*Client[*Route]` itself expands the
    // shared instance, so this half passed even before the fix.
    let with_ctor = check_src(&format!("{PRELUDE}var _ = NewClient[*Route](\"r\")\n"));
    assert!(
        with_ctor.errors.is_empty(),
        "expected no errors, got {:?}",
        with_ctor.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// The promotion is not one level deep, and the type arguments are not one.
/// `ClientWithList` is the three-parameter k8s shape, embedded two structs
/// down, with one method contributed at each level.
#[test]
fn embedded_generic_instance_promotes_through_nested_embedding() {
    let check = check_src(
        "package p\n\
         type Route struct{ N int }\n\
         type RouteList struct{ Items []Route }\n\
         type Opts struct{ Limit int }\n\
         type Client[T any, L any, O any] struct{ name string }\n\
         func (c *Client[T, L, O]) Get(name string) (T, error) { var z T; return z, nil }\n\
         func (c *Client[T, L, O]) List(o O) (L, error) { var z L; return z, nil }\n\
         type inner struct{ *Client[*Route, *RouteList, Opts] }\n\
         type outer struct{ inner }\n\
         type GetList interface {\n\
         \tGet(name string) (*Route, error)\n\
         \tList(o Opts) (*RouteList, error)\n\
         }\n\
         func f() GetList { return &outer{} }\n",
    );
    assert!(
        check.errors.is_empty(),
        "expected no errors, got {:?}",
        check.errors.iter().map(|e| &e.msg).collect::<Vec<_>>()
    );
}

/// Expanding the embedded instance must not turn the check into "any promoted
/// method will do". Four shapes that upstream rejects, each for a different
/// reason: the wrong type argument (`*Other`, so the substituted result type
/// does not match), a pointer-receiver method promoted through a *value*
/// embedded field (not in `wrapVal`'s method set), a method the instance
/// simply does not have, and the promoted method's result type read directly.
/// This test passes both before and after the fix — that is its job.
#[test]
fn embedded_generic_instance_still_reports_a_method_set_that_does_not_match() {
    let check = check_src(
        "package p\n\
         type Route struct{ N int }\n\
         type Other struct{ M int }\n\
         type Client[T any] struct{ name string }\n\
         func (c *Client[T]) Get(name string) (T, error) { var z T; return z, nil }\n\
         type OnlyGet interface{ Get(name string) (*Route, error) }\n\
         type Watcher interface{ Watch() error }\n\
         type wrongArg struct{ *Client[*Other] }\n\
         func a() OnlyGet { return &wrongArg{} }\n\
         type wrapVal struct{ Client[*Route] }\n\
         func b() OnlyGet { return wrapVal{} }\n\
         type noWatch struct{ *Client[*Route] }\n\
         func c() Watcher { return &noWatch{} }\n\
         type ok struct{ *Client[*Route] }\n\
         func d(n *ok) *Other { r, _ := n.Get(\"x\"); return r }\n",
    );
    let msgs: Vec<&String> = check.errors.iter().map(|e| &e.msg).collect();
    for subject in [
        // Wrong type argument: the substituted result is `*Other`, not `*Route`.
        "*wrongArg",
        // Pointer receiver, value embedded field: `Get` is not in the method set.
        "wrapVal",
        // The instance has no `Watch` at all.
        "*noWatch",
        // The promotion yields `*Route` — not `T`, and not `*Other`. Without
        // this line the three above would also pass on an expansion that
        // substituted the *wrong* arguments, or none at all.
        "*Route value as *Other",
    ] {
        assert_eq!(
            msgs.iter()
                .filter(|m| m.contains(subject) && m.contains("cannot use"))
                .count(),
            1,
            "expected exactly one rejection mentioning {subject}, got {msgs:?}"
        );
    }
    assert_eq!(msgs.len(), 4, "expected exactly four errors, got {msgs:?}");
}
