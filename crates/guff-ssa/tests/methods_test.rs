//! methods.rs / method-call / bound-method tests (E25–E27).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{Checker, Config, SelectionKind};

const METHOD_CALL_SRC: &str = "\
package p

type T int

func (t T) M() int { return 42 }

func f(t T) int { return t.M() }
";

const BOUND_METHOD_SRC: &str = "\
package p

type T int

func (t T) M() int { return 42 }

func f(t T) func() int { return t.M }
";

fn build_prog(src: &str) -> (Program, guff_ssa::ids::PackageId) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).expect("parse failed");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::SANITY_CHECK_FUNCTIONS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg_id = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg_id, &[file.clone()]);
    build_package(&mut prog, ssa_pkg_id, &[file]);
    (prog, ssa_pkg_id)
}

#[test]
fn test_object_method_returns_package_member() {
    let (mut prog, pkg_id) = build_prog(METHOD_CALL_SRC);
    let pkg = prog.packages.get(pkg_id);
    let m_obj = pkg
        .objects
        .iter()
        .find(|(o, _)| o.name(&prog.object_arena) == "M")
        .map(|(o, _)| *o)
        .expect("method M in objects");

    let fid = prog.object_method(m_obj, &[]);
    let f = prog.functions.get(fid);
    assert_eq!(f.name, "M");
    assert_eq!(f.object, Some(m_obj));
}

#[test]
fn test_method_call_emits_static_dispatch() {
    let (prog, pkg_id) = build_prog(METHOD_CALL_SRC);
    let f_fid = match prog.packages.get(pkg_id).members.get("f") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected f member, got {other:?}"),
    };
    let asm = disassemble_function(prog.functions.get(f_fid), &prog);
    println!("{asm}");
    assert!(asm.contains("func f(t T) int:"));
    assert!(asm.contains("M(t)"), "static method call:\n{asm}");
}

#[test]
fn test_bound_method_value_emits_make_closure() {
    let (prog, pkg_id) = build_prog(BOUND_METHOD_SRC);
    let f_fid = match prog.packages.get(pkg_id).members.get("f") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected f member, got {other:?}"),
    };
    let asm = disassemble_function(prog.functions.get(f_fid), &prog);
    println!("{asm}");
    assert!(asm.contains("make closure M$bound"), "bound closure:\n{asm}");
}

#[test]
fn test_method_set_contains_declared_method() {
    let (mut prog, pkg_id) = build_prog(METHOD_CALL_SRC);
    let t_typ = match prog.packages.get(pkg_id).members.get("T") {
        Some(MemberData::Type(t)) => *t,
        other => panic!("expected type T, got {other:?}"),
    };
    let ptr = guff_types::pointer::new_pointer(&mut prog.type_arena, t_typ);
    let mset: Vec<_> = prog.method_set(ptr).to_vec();
    assert!(
        mset.iter().any(|s| {
            s.kind() == SelectionKind::MethodVal && s.obj().name(&prog.object_arena) == "M"
        }),
        "method set should contain M"
    );
}
