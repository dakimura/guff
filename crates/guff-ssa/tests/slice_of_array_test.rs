//! Slicing an array slices its *storage*, so the operand is its address.
//!
//! ```go
//! switch typeparams.CoreType(xtyp).(type) {
//! case *types.Array:
//!     // Potentially escaping.
//!     x = b.addr(fn, e.X, true).address(fn)
//! case *types.Basic, *types.Slice, *types.Pointer: // *array
//!     x = b.expr(fn, e.X)
//! }
//! ```
//!
//! Reading the array as a value instead keeps the local in a register, and
//! every write through the slice is lost. `var id [16]byte;
//! hex.Decode(id[:], …); return id` then has two returns of the same zero
//! constant, which is how unparam came to say "result 0 ([16]byte) is always
//! nil" on pyroscope `pkg/pprof/pprof.go:1166` — a message that cannot be true
//! of an array. Every SSA-based check reading that function saw the same lost
//! store.

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

fn build(fname: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert!(
        check.errors.is_empty(),
        "SRC must type-check: {:?}",
        check.errors
    );
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

func fill(b []byte) {}

// The array is a local: slicing it must spill it to an Alloc first.
func array_local() [16]byte {
	var id [16]byte
	fill(id[:])

	return id
}

// A parameter is the same case — `a` is addressable.
func array_param(a [4]int) []int { return a[:] }

// Already a slice: no address is taken.
func slice_param(s []int) []int { return s[1:] }

// Already a pointer to an array: no address is taken either.
func pointer_param(p *[4]int) []int { return p[:] }

// A string slices as a value.
func string_param(s string) string { return s[1:] }
";

/// The local array is spilled and the slice is taken of the `Alloc`, so the
/// value the function returns is a *load*, not the zero constant it started
/// as.
#[test]
fn slicing_a_local_array_spills_it() {
    let asm = build("array_local");
    assert!(
        asm.contains("new [16]byte (id)"),
        "the array must be an Alloc:\n{asm}"
    );
    assert!(
        asm.contains("slice t0["),
        "the slice operand must be the alloc t0:\n{asm}"
    );
    assert!(
        asm.contains("*t0"),
        "the return must load the alloc rather than reuse a constant:\n{asm}"
    );
}

/// An addressable parameter is spilled for the same reason.
#[test]
fn slicing_an_array_parameter_spills_it() {
    let asm = build("array_param");
    assert!(
        asm.contains("new [4]int (a)"),
        "the parameter must be spilled:\n{asm}"
    );
    assert!(asm.contains("slice t0["), "{asm}");
}

/// The three operand types go/ssa slices *as values*: nothing is spilled, so
/// the fix cannot be "always take the address".
#[test]
fn slices_pointers_and_strings_are_sliced_as_values() {
    for f in ["slice_param", "pointer_param", "string_param"] {
        let asm = build(f);
        assert!(
            !asm.contains(" = new "),
            "{f} must not spill anything:\n{asm}"
        );
        assert!(asm.contains("slice "), "{f} has no Slice instruction:\n{asm}");
    }
}
