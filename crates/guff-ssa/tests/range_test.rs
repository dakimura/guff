//! Range and labelled-break SSA tests.

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

fn build(src: &str, fname: &str) -> String {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
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

#[test]
fn test_range_over_slice() {
    const SRC: &str = "\
package p

func sum(s []int) int {
	n := 0
	for _, v := range s {
		n += v
	}
	return n
}
";
    let asm = build(SRC, "sum");
    assert!(
        asm.contains("len(s)"),
        "expected len call in slice range loop:\n{asm}"
    );
    assert!(
        asm.contains("rangeindex"),
        "expected indexed range loop:\n{asm}"
    );
}

#[test]
fn test_range_over_array() {
    const SRC: &str = "\
package p

func count(a [2]int) {
	for range a {
	}
}
";
    let asm = build(SRC, "count");
    assert!(
        !asm.contains("len(a)"),
        "static array range should not call len:\n{asm}"
    );
    assert!(
        asm.contains("rangeindex"),
        "expected indexed range loop:\n{asm}"
    );
}

#[test]
fn test_range_over_channel() {
    const SRC: &str = "\
package p

func endless(ch <-chan int) {
	for range ch {
	}
}
";
    let endless_asm = build(SRC, "endless");
    assert!(
        endless_asm.contains("<-ch,ok"),
        "expected comma-ok receive in range loop:\n{endless_asm}"
    );
}

/// The base register of every `= &<base>.n [#0]` line, in order.
fn field_bases(asm: &str) -> Vec<&str> {
    asm.lines()
        .filter_map(|l| {
            let rest = l.split_once("= &")?.1;
            rest.split_once(".n [#0]").map(|(base, _)| base)
        })
        .collect()
}

#[test]
fn test_range_over_channel_defines_the_key_variable() {
    // go/ssa's `rangeStmt` declares the `:=` iteration variables for *every*
    // range kind; `rangeChan` only returns the received key. guff inlines the
    // declaration into each range arm, and this one used to skip it — so
    // `address(key)` found no local and handed back a nil address. The body
    // then read `t = *nil` for every use of `t`, and with a value element the
    // load's type came out `*invalid type`, which silently switched off every
    // analyzer that keys on the operand's type (gosec G115 stopped seeing the
    // conversion at all).
    const SRC: &str = "\
package p

type rec struct{ n uint64 }

func f(ch chan rec) uint64 {
	var total uint64
	for t := range ch {
		total += t.n
	}
	return total
}
";
    let asm = build(SRC, "f");
    assert!(
        !asm.contains("*nil"),
        "channel range key must have a real address:\n{asm}"
    );
    assert!(
        !asm.contains("invalid type"),
        "channel range key must keep its element type:\n{asm}"
    );
    // A struct is an aggregate, so the local is not lifted and stays visible.
    assert!(
        asm.contains("local rec (t)"),
        "expected a local for the received key:\n{asm}"
    );
    assert_eq!(
        field_bases(&asm).len(),
        1,
        "expected one field address off the key:\n{asm}"
    );
}

#[test]
fn test_range_over_channel_of_pointers_defines_the_key_variable() {
    // The pointer element is the shape beats' packetbeat/protos/thrift writes
    // (`chan *thriftTransaction`). Here the broken load still carried the right
    // type, so nothing looked wrong — but the two reads of `t.n` came off two
    // *different* `*nil` loads, and gosec's `isSameOrRelated` could no longer
    // tell that the bounds check guarded the conversion below it.
    const SRC: &str = "\
package p

type rec struct{ n uint64 }

func f(ch chan *rec) uint64 {
	var total uint64
	for t := range ch {
		if t.n < 10 {
			total += t.n
		}
	}
	return total
}
";
    let asm = build(SRC, "f");
    assert!(
        !asm.contains("*nil"),
        "channel range key must have a real address:\n{asm}"
    );
    let bases = field_bases(&asm);
    assert_eq!(bases.len(), 2, "expected two field addresses:\n{asm}");
    assert_eq!(
        bases[0], bases[1],
        "both reads of t.n must come off the same received key:\n{asm}"
    );
    let key = asm
        .lines()
        .find(|l| l.contains("= extract") && l.trim_end().ends_with("*rec"))
        .and_then(|l| l.trim().split_once(" = "))
        .map(|(reg, _)| reg.to_string())
        .unwrap_or_default();
    assert_eq!(
        bases[0], key,
        "the field addresses must come off the received key itself:\n{asm}"
    );
}

#[test]
fn test_range_over_channel_key_captured_by_a_closure() {
    // An escaping key keeps its `Alloc` instead of being lifted, so this pins
    // that the *declaration* — not just the lifted register — is what used to
    // be missing.
    const SRC: &str = "\
package p

func f(ch chan int) []func() int {
	var out []func() int
	for t := range ch {
		out = append(out, func() int { return t })
	}
	return out
}
";
    let asm = build(SRC, "f");
    assert!(
        !asm.contains("*nil"),
        "captured channel range key must have a real address:\n{asm}"
    );
    assert!(
        asm.contains("new int (t)"),
        "expected a heap cell for the captured key:\n{asm}"
    );
}

#[test]
fn test_range_over_channel_assigns_an_existing_variable() {
    // `for t = range ch` (no `:=`) must *not* declare anything: the store goes
    // to the variable already in scope, which is why the missing declaration
    // never showed up in this arm.
    const SRC: &str = "\
package p

func f(ch chan int) int {
	var t int
	sum := 0
	for t = range ch {
		sum += t
	}
	return sum + t
}
";
    let asm = build(SRC, "f");
    assert!(
        !asm.contains("*nil"),
        "assigned channel range key must have a real address:\n{asm}"
    );
}

#[test]
fn test_range_over_map() {
    const SRC: &str = "\
package p

func walk(m map[string]int) {
	for range m {
	}
}
";
    let asm = build(SRC, "walk");
    assert!(asm.contains("= range "), "expected range instruction:\n{asm}");
    assert!(asm.contains("= next "), "expected next instruction:\n{asm}");
    assert!(asm.contains("rangeiter"), "expected rangeiter blocks:\n{asm}");
}

#[test]
fn test_range_over_string() {
    const SRC: &str = "\
package p

func walk(s string) {
	for _, r := range s {
		_ = r
	}
}
";
    let asm = build(SRC, "walk");
    assert!(asm.contains("= range "), "expected range instruction:\n{asm}");
    assert!(asm.contains("= next "), "expected next instruction:\n{asm}");
}

#[test]
fn test_range_over_int() {
    const SRC: &str = "\
package p

func sum(n int) int {
	total := 0
	for i := range n {
		total += i
	}
	return total
}
";
    let asm = build(SRC, "sum");
    assert!(
        asm.contains("rangeint"),
        "expected integer range loop:\n{asm}"
    );
}

/// `range p` over a pointer to a **named** array type.
///
/// The pointee is the type as *written*, so `*Key` where `type Key [32]byte`
/// hands the builder a `Named`, and `array_elem` — which does not unwrap —
/// panicked on it: `expected Array, got Discriminant(11)`. It killed an
/// analysis worker on minio, and a killed worker is not a finding-set
/// difference, so nothing downstream showed it.
///
/// `typeset.rs`'s `index_elem` already unwrapped before asking; this path did
/// not. The length is the constant N for a pointer-to-array too — Go reads it
/// off the type and never loads the pointer.
#[test]
fn test_range_over_pointer_to_named_array() {
    const SRC: &str = "\
package p

type Key [4]byte

func first(p *Key) byte {
	var out byte
	for i, b := range p {
		if i == 0 {
			out = b
		}
	}
	return out
}
";
    let asm = build(SRC, "first");
    assert!(
        asm.contains("rangeindex"),
        "expected an indexed range loop:\n{asm}"
    );
    assert!(
        !asm.contains("len("),
        "the length of a *[N]T range is the constant N, not a len call:\n{asm}"
    );
}

/// The same over a pointer to an *unnamed* array — the shape that already
/// worked, kept beside it so a future unwrap that goes too far is visible.
#[test]
fn test_range_over_pointer_to_unnamed_array() {
    const SRC: &str = "\
package p

func firstUnnamed(p *[4]byte) byte {
	var out byte
	for i, b := range p {
		if i == 0 {
			out = b
		}
	}
	return out
}
";
    let asm = build(SRC, "firstUnnamed");
    assert!(
        asm.contains("rangeindex"),
        "expected an indexed range loop:\n{asm}"
    );
    assert!(
        !asm.contains("len("),
        "the length of a *[N]T range is the constant N, not a len call:\n{asm}"
    );
}
