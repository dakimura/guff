//! Switch / type-switch / select / send / IncDec SSA tests (R17 statement coverage).

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
fn test_value_switch() {
    const SRC: &str = "\
package p

func classify(n int) int {
	switch n {
	case 1:
		return 10
	case 2, 3:
		return 20
	default:
		return 0
	}
}
";
    let asm = build(SRC, "classify");
    assert!(
        asm.contains("switch.body") || asm.contains("if "),
        "expected switch if-else chain:\n{asm}"
    );
    assert!(
        asm.contains("=="),
        "expected equality compares for cases:\n{asm}"
    );
}

#[test]
fn test_bool_switch_and_fallthrough() {
    const SRC: &str = "\
package p

func flags(a, b bool) int {
	n := 0
	switch {
	case a:
		n = 1
		fallthrough
	case b:
		n = n + 2
	}
	return n
}
";
    let asm = build(SRC, "flags");
    assert!(
        asm.contains("jump ") || asm.contains("if "),
        "expected bool-switch CFG:\n{asm}"
    );
}

#[test]
fn test_type_switch() {
    const SRC: &str = "\
package p

func describe(x interface{}) int {
	switch v := x.(type) {
	case int:
		return v
	case string:
		return len(v)
	default:
		return -1
	}
}
";
    let asm = build(SRC, "describe");
    assert!(
        asm.contains("typeassert"),
        "expected typeassert in type switch:\n{asm}"
    );
}

#[test]
fn test_send_and_incdec() {
    const SRC: &str = "\
package p

func pump(ch chan int) {
	i := 0
	i++
	ch <- i
	i--
}
";
    let asm = build(SRC, "pump");
    assert!(asm.contains("send "), "expected send instruction:\n{asm}");
    assert!(
        asm.contains("+") || asm.contains("-"),
        "expected IncDec arithmetic:\n{asm}"
    );
}

#[test]
fn test_select_blocking() {
    const SRC: &str = "\
package p

func race(a, b <-chan int) int {
	select {
	case x := <-a:
		return x
	case y := <-b:
		return y
	}
}
";
    let asm = build(SRC, "race");
    assert!(
        asm.contains("select blocking"),
        "expected blocking select:\n{asm}"
    );
}

#[test]
fn test_select_nonblocking_default() {
    const SRC: &str = "\
package p

func try(ch chan int) bool {
	select {
	case ch <- 1:
		return true
	default:
		return false
	}
}
";
    let asm = build(SRC, "try");
    assert!(
        asm.contains("select nonblocking"),
        "expected nonblocking select:\n{asm}"
    );
}

#[test]
fn test_compound_assign() {
    const SRC: &str = "\
package p

func addto(n int) int {
	n += 3
	return n
}
";
    let asm = build(SRC, "addto");
    assert!(
        asm.contains("+"),
        "expected += to emit addition:\n{asm}"
    );
}

// ---- single-case select: receive before LHS (x/tools v0.52 assignSelectRecvStmt) ----

const SINGLE_SELECT_SRC: &str = "\
package p

func f() int { return 0 }

func single(ch chan int) int {
	select {
	case v := <-ch:
		return v
	}
}

func commaok(ch chan int) (int, bool) {
	select {
	case v, ok := <-ch:
		return v, ok
	}
}

func order(a []int, ch chan int) {
	select {
	case a[f()] = <-ch:
	}
}

func existing(ch chan int) int {
	x := 1
	select {
	case x = <-ch:
	}
	return x
}

func blank(ch chan int) {
	select {
	case _ = <-ch:
	}
	select {
	case <-ch:
	}
}

func send(ch chan int, v int) {
	select {
	case ch <- v:
	}
}

func escape(ch chan int) func() int {
	select {
	case v := <-ch:
		return func() int { return v }
	}
}

func escape2(ch chan int) (func() int, func() bool) {
	select {
	case v, ok := <-ch:
		return func() int { return v }, func() bool { return ok }
	}
}

type S struct{ x int }

func field(g func() *S, ch chan int) {
	select {
	case g().x = <-ch:
	}
}

func multi2(a []int, ch chan int) {
	var ok bool
	select {
	case a[f()], ok = <-ch:
	}
	_ = ok
}

func two(a []int, b, c chan int) {
	select {
	case a[f()] = <-b:
	case a[0] = <-c:
	}
}
";

/// instrs returns the instruction lines of a single-block function, with the
/// right-aligned type column stripped.
fn instrs(asm: &str) -> Vec<String> {
    asm.lines()
        .filter(|l| l.starts_with('\t'))
        .map(|l| {
            let l = l.trim();
            // Drop the type column (separated by a run of spaces).
            match l.find("  ") {
                Some(i) => l[..i].trim_end().to_string(),
                None => l.to_string(),
            }
        })
        .collect()
}

/// Each expectation below is the `ssadump -build=F` output of
/// golang.org/x/tools v0.52.0 for the same source (type column dropped).
/// v0.50 (and guff before this change) evaluated the LHS first, as for an
/// ordinary assignment: `f()` / `g()` / `new int (v)` came before `<-ch`.
#[test]
fn test_single_case_select_receives_before_lhs() {
    let cases: &[(&str, &[&str])] = &[
        ("single", &["t0 = <-ch", "return t0"]),
        (
            "commaok",
            &["t0 = <-ch,ok", "t1 = extract t0 #0", "t2 = extract t0 #1", "return t1, t2"],
        ),
        ("order", &["t0 = <-ch", "t1 = f()", "t2 = &a[t1]", "*t2 = t0", "return"]),
        ("existing", &["t0 = <-ch", "return t0"]),
        ("blank", &["t0 = <-ch", "t1 = <-ch", "return"]),
        ("send", &["send ch <- v", "return"]),
        (
            "escape",
            &["t0 = <-ch", "t1 = new int (v)", "*t1 = t0", "t2 = make closure escape$1 [t1]", "return t2"],
        ),
        (
            "escape2",
            &[
                "t0 = <-ch,ok",
                "t1 = extract t0 #0",
                "t2 = extract t0 #1",
                "t3 = new int (v)",
                "t4 = new bool (ok)",
                "*t3 = t1",
                "*t4 = t2",
                "t5 = make closure escape2$1 [t3]",
                "t6 = make closure escape2$2 [t4]",
                "return t5, t6",
            ],
        ),
        ("field", &["t0 = <-ch", "t1 = g()", "t2 = &t1.x [#0]", "*t2 = t0", "return"]),
        (
            "multi2",
            &[
                "t0 = <-ch,ok",
                "t1 = extract t0 #0",
                "t2 = extract t0 #1",
                "t3 = f()",
                "t4 = &a[t3]",
                "*t4 = t1",
                "return",
            ],
        ),
    ];
    for (fname, want) in cases {
        let asm = build(SINGLE_SELECT_SRC, fname);
        let got = instrs(&asm);
        assert_eq!(got, *want, "{fname}:\n{asm}");
    }
}

/// A select of two or more cases is unchanged: the LHS of each receive is
/// evaluated in its case body before the extract (as in x/tools v0.52).
#[test]
fn test_multi_case_select_order_unchanged() {
    let asm = build(SINGLE_SELECT_SRC, "two");
    let body: Vec<&str> = asm
        .lines()
        .map(str::trim)
        .skip_while(|l| !l.starts_with("2:"))
        .skip(1)
        .take_while(|l| !l.starts_with("3:"))
        .collect();
    assert!(body[0].starts_with("t3 = f()"), "{asm}");
    assert!(body[1].starts_with("t4 = extract t0 #2"), "{asm}");
    assert!(body[2].starts_with("t5 = &a[t3]"), "{asm}");
}
