//! Operators on type parameters — Go's `allX` predicate family.
//!
//! `isX` stops at `Underlying()`, so a type parameter satisfies none of them
//! and `total += x` inside `func Sum[T ~int|~float64]` used to be rejected
//! with "operator ADD not defined on operand". That doesn't just mis-report a
//! type error: the package goes ill-typed and every type-dependent analyzer
//! silently reports nothing (COMPAT-HARDENING.md Phase 1's "failure that
//! doesn't show up as a diff").
//!
//! Ground truth for every case below is `go build` on go1.26 — the positive
//! cases compile, the negative ones are the errors it prints.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::{Checker, Config};

const CONSTRAINTS: &str = "\
type Num interface{ ~int | ~float64 }\n\
type Int interface{ ~int | ~int8 | ~uint }\n\
type Str interface{ ~string }\n\
type Bool interface{ ~bool }\n\
type Ord interface{ ~int | ~string }\n\
type IntOrStr interface{ ~int | ~string }\n\
type IntOrBool interface{ ~int | ~bool }\n";

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

/// Type-check `decl` alongside the shared constraint declarations.
fn check_decl(decl: &str) -> Checker {
    check_src(&format!("package p\n{}{}\n", CONSTRAINTS, decl))
}

#[track_caller]
fn accepts(decl: &str) {
    let c = check_decl(decl);
    assert!(
        c.errors.is_empty(),
        "{decl}\n  unexpected errors: {:?}",
        c.errors
    );
}

#[track_caller]
fn rejects(decl: &str) {
    let c = check_decl(decl);
    assert!(
        !c.errors.is_empty(),
        "{decl}\n  expected an error, got none"
    );
}

// ----------------------------------------------------------------------------
// Accepted — the predicate holds for every term of the type set.

#[test]
fn arithmetic_on_numeric_type_param() {
    // The form that motivated this: `+=` in a loop over a generic slice.
    accepts(
        "func Sum[T Num](xs []T) T {\n\
           var total T\n\
           for _, x := range xs { total += x }\n\
           return total\n\
         }",
    );
    accepts("func Arith[T Num](a, b T) T { return a + b - a*b/b }");
    accepts("func Neg[T Num](a T) T { return -a }");
    accepts("func Pos[T Num](a T) T { return +a }");
}

#[test]
fn integer_ops_on_integer_type_param() {
    accepts("func Bits[T Int](a, b T) T { return a%b | a&b ^ a&^b }");
    accepts("func Compl[T Int](a T) T { return ^a }");
}

#[test]
fn shift_with_type_param_operand_and_count() {
    accepts("func ShiftL[T Int](a T) T { return a << 3 }");
    accepts("func ShiftBy[T Int](n T) int { return 1 << n }");
}

#[test]
fn string_concat_on_string_type_param() {
    accepts("func Cat[T Str](a, b T) T { return a + b }");
    // `~int | ~string` is allNumericOrString, so `+` is defined but `-` is not.
    accepts("func Plus[T IntOrStr](a, b T) T { return a + b }");
}

#[test]
fn boolean_ops_and_conditions_on_bool_type_param() {
    accepts("func And[T Bool](a, b T) T { return a && b }");
    accepts("func Not[T Bool](a T) T { return !a }");
    accepts("func If[T Bool](a T) int { if a { return 1 }; for a { break }; return 0 }");
}

#[test]
fn ordering_and_minmax_on_ordered_type_param() {
    accepts("func Less[T Ord](a, b T) bool { return a < b && a <= b && a > b && a >= b }");
    accepts("func MinMax[T Ord](a, b T) T { return min(a, b) + max(a, b) }");
}

#[test]
fn incdec_on_numeric_type_param() {
    accepts("func Inc[T Num](a T) T { a++; a--; return a }");
}

#[test]
fn untyped_constant_converts_to_type_param_when_every_term_accepts_it() {
    accepts("func Init[T Num]() T { var x T = 1; x += 2; return x }");
    // Not representable in one of the terms.
    rejects("func Big[T interface{ ~int8 }]() T { var x T = 300; return x }");
    // `1` is fine for ~int, not for ~string — `underIs` needs every term.
    rejects("func Mixed[T IntOrStr]() T { var x T = 1; return x }");
    rejects("func Any[T any]() T { var x T = 0; return x }");
}

#[test]
fn index_by_integer_type_param() {
    accepts("func At[I Int](s []byte, i I) byte { return s[i] }");
}

#[test]
fn append_string_type_param_to_byte_slice() {
    accepts("func AppendStr[T Str](b []byte, s T) []byte { return append(b, s...) }");
}

// ----------------------------------------------------------------------------
// Generic type aliases (go1.24). The Alias has to exist before its RHS is
// checked, or the type parameters aren't in scope for it: `type A[T any] =
// Box[T]` used to fail with "undefined: T", taking the whole package
// ill-typed with it.

#[test]
fn generic_type_alias_declares_its_parameters_for_the_rhs() {
    let c = check_src(
        "package p\n\
         type Box[T any] struct{ v T }\n\
         func (b *Box[T]) Get() T { return b.v }\n\
         type Alias[T any] = Box[T]\n\
         type Pair[K comparable, V any] = map[K]V\n\
         type Selfish[T any] = *Alias[T]\n\
         type Plain = Box[int]\n\
         var _ Alias[int]\n\
         var _ Pair[string, int]\n\
         var _ Selfish[bool]\n\
         var _ Plain\n\
         func Use(a Alias[int]) int { return a.Get() }\n",
    );
    assert!(c.errors.is_empty(), "unexpected errors: {:?}", c.errors);
}

#[test]
fn alias_rhs_cannot_be_a_parameter_it_declares() {
    let c = check_src("package p\ntype Bad[P any] = P\n");
    assert_eq!(c.errors.len(), 1, "errors: {:?}", c.errors);
    assert_eq!(
        c.errors[0].msg,
        "cannot use type parameter declared in alias declaration as RHS"
    );
}

// ----------------------------------------------------------------------------
// Rejected — `allX` needs *every* term to qualify, and a term-less type set
// (`any`) qualifies for nothing. These are the errors go1.26 prints.

#[test]
fn unconstrained_type_param_supports_no_operator() {
    rejects("func BadAdd[T any](a, b T) T { return a + b }");
}

#[test]
fn one_bad_term_disqualifies_the_whole_set() {
    rejects("func BadSub[T IntOrStr](a, b T) T { return a - b }");
    rejects("func BadOrd[T IntOrBool](a, b T) bool { return a < b }");
    rejects("func BadMinMax[T IntOrBool](a, b T) T { return min(a, b) }");
}

#[test]
fn non_integer_type_param_cannot_shift() {
    rejects("func BadShift[T Num](a T) T { return a << 1 }");
    rejects("func BadShiftCount[T Num](a T) int { return 1 << a }");
}

#[test]
fn non_integer_type_param_cannot_index() {
    rejects("func BadIndex[T Num](s []byte, i T) byte { return s[i] }");
}

#[test]
fn non_boolean_type_param_is_not_a_condition() {
    rejects("func BadIf[T Num](a T) { if a { } }");
    rejects("func BadNot[T Num](a T) T { return !a }");
}

#[test]
fn non_numeric_type_param_cannot_incdec() {
    rejects("func BadInc[T Str](a T) { a++ }");
}

#[test]
fn non_string_type_param_cannot_be_appended_to_bytes() {
    rejects("func BadAppend[T Num](b []byte, s T) []byte { return append(b, s...) }");
}

// ----------------------------------------------------------------------------
// Not fixed here: `range` and channel operations reach the type set through
// `commonUnder`, a separate mechanism from `allX`. `stmt.rs` says so in both
// places ("`commonUnder` is approximated by `Underlying` (no type-set
// iteration)"). Every declaration below compiles under go1.26; guff rejects
// it and takes the package ill-typed with it.
//
// Recorded, not asserted-green: see docs/COMPAT-HARDENING.md, 12th session.

#[test]
#[ignore = "commonUnder over a type set is not ported — see COMPAT-HARDENING.md (12th session, next step 4)"]
fn range_and_channel_ops_on_type_params() {
    accepts(
        "func RangeSlice[T interface{ ~[]int }](xs T) int {\n\
           n := 0\n\
           for i, v := range xs { n += i + v }\n\
           return n\n\
         }",
    );
    accepts(
        "func RangeMap[T interface{ ~map[string]int }](m T) int {\n\
           n := 0\n\
           for _, v := range m { n += v }\n\
           return n\n\
         }",
    );
    accepts("func Recv[T interface{ ~chan int }](c T) int { return <-c }");
    accepts("func Send[T interface{ ~chan int }](c T) { c <- 1 }");
}
