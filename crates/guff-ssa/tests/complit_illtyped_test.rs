//! Composite literals that the type checker rejects.
//!
//! golangci-lint never reaches the SSA builder with one of these: an ill-typed
//! package makes every action fail with `IllTypedError` before an analyzer
//! runs. guff builds anyway — its linters run on whatever the checker managed
//! to produce — so the builder is the one that has to survive a struct literal
//! whose key names no field, is not a field name at all, or supplies more
//! values than the struct has fields.
//!
//! Before 2026-09-10 each of those three shapes panicked the worker thread
//! (`struct field "Name" not found`), which killed the whole package's findings
//! and still exited 0. The rule now is the one [`comp_lit`] already used for an
//! `Invalid` literal type: skip the element, keep the package.
//!
//! Every source below is a shape measured against `go vet` on 2026-09-10; the
//! `go vet` diagnostic is quoted next to each function.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

/// Builds `SRC` and returns the disassembly of `fname`. Unlike `complit_test`'s
/// helper this asserts the checker *did* report errors: a source that silently
/// starts type-checking would stop exercising the ill-typed path and the test
/// would keep passing while measuring nothing.
fn build(fname: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert!(!check.errors.is_empty(), "SRC is supposed to be ill-typed");
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);

    let fid: FuncId = match prog.packages.get(ssa_pkg_id).members.get(fname) {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected {fname} to be a Function member, got {other:?}"),
    };
    disassemble_function(prog.functions.get(fid), &prog)
}

const SRC: &str = "\
package p

type Inner struct{ A int }

type T struct{ Age int }

// unknown field Name in struct literal of type T
func unknown_key() T { return T{Name: \"x\", Age: 1} }

// too many values in struct literal of type T
func too_many_values() T { return T{1, 2, 3} }

// invalid field name \"Age\" in struct literal
func string_key() T { return T{\"Age\": 1} }

// invalid field name p.Inner in struct literal
func selector_key() T { return T{Inner.A: 1} }

// unknown field B in struct literal of type Inner
func nested() Inner { return Inner{A: 1, B: 2} }

// unknown field Bad in struct literal of type T
func pointer() *T { return &T{Bad: 1} }

// unknown field Bad in struct literal of type T
func in_slice() []T { return []T{{Bad: 1}} }

// unknown field Bad in struct literal of type T
func in_map() map[string]T { return map[string]T{\"k\": {Bad: 1}} }

// index 2 is out of bounds (>= 2)
func array_overflow() [2]int { return [2]int{1, 2, 3} }

// unknown field Name in struct literal of type T — a package-level initializer,
// which the builder reaches through `init` rather than a function body.
var V = T{Name: 1}
";

/// The six shapes whose key names no field of the struct. Each one used to
/// panic at `struct field {name:?} not found`; the assertion is that the
/// function builds at all.
#[test]
fn unknown_field_names_are_skipped_not_panicked() {
    for f in ["unknown_key", "nested", "pointer", "in_slice", "in_map"] {
        let asm = build(f);
        assert!(asm.contains(&format!("func {f}")), "{f} did not build:\n{asm}");
    }
}

/// The bad element is dropped and the good one is still written: `T{Name: "x",
/// Age: 1}` stores 1 into `Age` (#0) and nothing else. Dropping the whole
/// literal instead would silently change what every SSA-based linter sees.
#[test]
fn the_well_typed_elements_of_a_bad_literal_still_store() {
    let asm = build("unknown_key");
    println!("{asm}");
    assert!(asm.contains("&t0.Age [#0]"), "expected FieldAddr Age:\n{asm}");
    assert!(asm.contains("*t1 = 1"), "expected the store of the good element:\n{asm}");
    assert_eq!(asm.matches("[#").count(), 1, "exactly one field written:\n{asm}");
}

/// `T{1, 2, 3}` for a one-field `T`: the first value lands in `Age`, the two
/// past the end are dropped. Previously an index-out-of-bounds panic inside
/// `guff_types::struct_field`.
#[test]
fn positional_values_past_the_last_field_are_skipped() {
    let asm = build("too_many_values");
    println!("{asm}");
    assert!(asm.contains("&t0.Age [#0]"), "expected FieldAddr Age:\n{asm}");
    assert!(asm.contains("*t1 = 1"), "expected the in-range store:\n{asm}");
    assert_eq!(asm.matches("[#").count(), 1, "only field #0 exists:\n{asm}");
}

/// A key that is not an identifier at all — a string literal or a selector.
/// Both used to panic at `struct literal key is not an identifier`.
#[test]
fn keys_that_are_not_identifiers_are_skipped() {
    for f in ["string_key", "selector_key"] {
        let asm = build(f);
        println!("{asm}");
        assert!(asm.contains(&format!("func {f}")), "{f} did not build:\n{asm}");
        assert_eq!(asm.matches("[#").count(), 0, "{f} must write no field:\n{asm}");
    }
}

/// A package-level `var` initializer is built into `init`, not into a function
/// body, so it reaches `comp_lit_struct` by a different route and needs its own
/// case. It panicked too.
#[test]
fn a_bad_package_level_initializer_is_skipped() {
    let asm = build("init");
    println!("{asm}");
    assert!(asm.contains("func init"), "init did not build:\n{asm}");
    assert_eq!(asm.matches("[#").count(), 0, "the only element is bad:\n{asm}");
}

/// The array case already survived (`comp_lit_array_slice` bounds its writes),
/// and it stays that way: an over-long array literal builds.
#[test]
fn an_over_long_array_literal_still_builds() {
    let asm = build("array_overflow");
    println!("{asm}");
    assert!(asm.contains("func array_overflow"), "did not build:\n{asm}");
}
