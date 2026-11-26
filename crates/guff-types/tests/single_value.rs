//! A multi-valued call used where one value is wanted.
//!
//! Go rejects `_ = f()` / `x := f()` / `var y int = f()` when `f` returns two
//! values; guff accepted all three silently, which left the package *well*
//! typed on guff's side and ill-typed on golangci-lint's — and "is this
//! package ill-typed" is a per-package switch that decides whether every other
//! finding in the file is reported at all. See `docs/COMPAT-HARDENING.md` §7.
//!
//! Ground truth for every message here is `go build` / `go/types` on the same
//! source (go1.26.5).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

fn messages(src: &str) -> Vec<String> {
    check_src(src)
        .errors
        .iter()
        .map(|e| e.msg.clone())
        .collect()
}

const TWO: &str = "package p\nfunc two() (int, error) { return 0, nil }\nfunc one() int { return 0 }\n";

#[test]
fn blank_assign_from_two_valued_call_is_a_mismatch() {
    assert_eq!(
        messages(&format!("{TWO}func f() {{\n\t_ = two()\n}}\n")),
        ["assignment mismatch: 1 variable but two returns 2 values"]
    );
}

#[test]
fn short_var_decl_from_two_valued_call_is_a_mismatch() {
    assert_eq!(
        messages(&format!("{TWO}func f() {{\n\tx := two()\n\t_ = x\n}}\n")),
        ["assignment mismatch: 1 variable but two returns 2 values"]
    );
}

#[test]
fn plain_assign_from_two_valued_call_is_a_mismatch() {
    assert_eq!(
        messages(&format!(
            "{TWO}func f() {{\n\tvar x int\n\tx = two()\n\t_ = x\n}}\n"
        )),
        ["assignment mismatch: 1 variable but two returns 2 values"]
    );
}

#[test]
fn mismatch_names_the_callee_through_a_selector() {
    // `call.Fun` is rendered with go/types' ExprString, so a method value
    // prints as `v.m`, not as the receiver alone.
    let src = "package p\n\
               type T struct{}\n\
               func (T) m() (int, error) { return 0, nil }\n\
               func f() {\n\tvar v T\n\t_ = v.m()\n}\n";
    assert_eq!(
        messages(src),
        ["assignment mismatch: 1 variable but v.m returns 2 values"]
    );
}

#[test]
fn var_decl_with_declared_type_is_a_single_value_context() {
    // A single declared variable does not go through initVars at all
    // (`varDecl` calls `check.expr` directly), so the error is singleValue's.
    assert_eq!(
        messages(&format!("{TWO}var y int = two()\n")),
        ["multiple-value two() (value of type (int, error)) in single-value context"]
    );
}

#[test]
fn n_to_n_var_decl_reports_per_expression() {
    // `var a, b = two(), 1` is a 2:2 mapping, so each rhs is evaluated in a
    // single-value context; only the call is wrong.
    assert_eq!(
        messages(&format!("{TWO}var a, b = two(), 1\n")),
        ["multiple-value two() (value of type (int, error)) in single-value context"]
    );
}

#[test]
fn tuple_in_an_operand_position_is_a_single_value_context() {
    assert_eq!(
        messages(&format!("{TWO}var z = two() + 1\n")),
        ["multiple-value two() (value of type (int, error)) in single-value context"]
    );
}

#[test]
fn extra_argument_alongside_a_tuple_is_a_single_value_context() {
    // With more than one argument the spread is not available, so the tuple
    // argument is reduced first — Go's `genericExprList` splits on n == 1.
    let src = format!("{TWO}func g(a int, b error, c int) {{}}\nfunc f() {{\n\tg(two(), 1)\n}}\n");
    let msgs = messages(&src);
    assert!(
        msgs.iter().any(|m| m
            == "multiple-value two() (value of type (int, error)) in single-value context"),
        "got {msgs:?}"
    );
}

// --- shapes that must stay silent -----------------------------------------

#[test]
fn n_to_1_spread_is_still_fine() {
    for src in [
        format!("{TWO}func f() {{\n\ta, b := two()\n\t_, _ = a, b\n}}\n"),
        format!("{TWO}func f() {{\n\tx := one()\n\t_ = x\n}}\n"),
        format!("{TWO}func f() {{\n\t_ = one()\n}}\n"),
        format!("{TWO}var a, b = two()\n"),
        // A lone multi-valued argument spreads across the parameters.
        format!("{TWO}func g(a int, b error) {{}}\nfunc f() {{\n\tg(two())\n}}\n"),
        // …including through parentheses, which `unparen` sees through.
        format!("{TWO}func f() {{\n\ta, b := (two())\n\t_, _ = a, b\n}}\n"),
        // Returning a tuple from a matching signature.
        format!("{TWO}func f() (int, error) {{\n\treturn two()\n}}\n"),
        // comma-ok forms still expand.
        "package p\nfunc f(m map[string]int) {\n\tv, ok := m[\"k\"]\n\t_, _ = v, ok\n}\n"
            .to_string(),
    ] {
        assert_eq!(messages(&src), Vec::<String>::new(), "for source:\n{src}");
    }
}

#[test]
fn a_broken_rhs_does_not_also_report_a_count_mismatch() {
    // The rhs is already invalid, so Go reports the undefined name only.
    let msgs = messages("package p\nfunc f() {\n\t_ = nosuchfunc()\n}\n");
    assert_eq!(msgs.len(), 1, "got {msgs:?}");
    assert!(msgs[0].contains("undefined"), "got {msgs:?}");
}

#[test]
fn return_of_a_two_valued_call_into_one_result_is_a_mismatch() {
    // `returnError`'s wording, not `assignError`'s.
    assert_eq!(
        messages(&format!("{TWO}func f() int {{\n\treturn two()\n}}\n")),
        ["too many return values"]
    );
}
