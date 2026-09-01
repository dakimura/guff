//! Tests for `ssautil::visit` (Milestone F, chunk F02).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_function, create_package, populate_package_members};
use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ssautil::{all_functions, main_packages};
use guff_ssa::value::Value;
use guff_types::{Checker, Config};

const MAIN_PKG: &str = "package main\nfunc main() {}\nfunc helper() int { return 1 }\n";

#[test]
fn test_all_functions_reaches_closure_operand() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "main.go", MAIN_PKG.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);
    build_package(&mut prog, ssa_pkg, &[file]);

    let fns = all_functions(&mut prog);
    let main_fid = prog.packages.get(ssa_pkg).func("main").unwrap();
    let helper_fid = prog.packages.get(ssa_pkg).func("helper").unwrap();
    assert!(fns.contains(&main_fid));
    assert!(fns.contains(&helper_fid));
}

/// A closure's code is an operand upstream (`MakeClosure.Operands` returns
/// `&v.Fn` first), so `visitor.function` descends into it. guff keeps it as a
/// `FuncId` field, so the walk used to stop at every capturing closure — and at
/// the bound-method thunk that `x.M` compiles to, which is the only *bare*
/// function a method value produces.
///
/// The thunk is what CHA's `funcsBySig` matches when such a value is later
/// called, and its body's static call is the only edge the real method ever
/// gets. Without it gosec's taint engine saw the method as a function with no
/// callers, i.e. an entry point, and auto-tainted its source-typed parameters.
const METHOD_VALUE: &str = "\
package p

type box struct{}

func (b box) redirect(s string) {}

type handler func(string)

func Dispatch() {
	var b box
	var h handler = b.redirect
	h(\"x\")
}
";

#[test]
fn test_all_functions_follows_a_make_closure_into_a_bound_thunk() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", METHOD_VALUE.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);
    build_package(&mut prog, ssa_pkg, &[file]);

    let fns = all_functions(&mut prog);
    let mut names: Vec<&str> = fns
        .iter()
        .map(|&f| prog.functions.get(f).name.as_str())
        .collect();
    names.sort_unstable();
    // Counted, and the program has exactly these four functions: reachability
    // that misses the thunk answers `["Dispatch", "init"]` and still contains
    // everything a `contains` assertion would look for.
    assert_eq!(
        names,
        vec!["Dispatch", "init", "redirect", "redirect$bound"],
        "the walk must reach the bound thunk and, through its body, the method"
    );
}

#[test]
fn test_main_packages() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "main.go", MAIN_PKG.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);
    build_package(&mut prog, ssa_pkg, &[file]);

    let mains = main_packages(&prog, &[ssa_pkg]);
    assert_eq!(mains.len(), 1);
    assert_eq!(mains[0], ssa_pkg);
}

#[test]
fn test_all_functions_follows_function_operand() {
    let (type_arena, universe) = guff_types::init_universe();
    let mut prog = Program::new(
        BuilderMode::default(),
        guff_types::Info::default(),
        type_arena,
        guff_types::ObjectArena::new(),
        guff_types::PackageArena::new(),
    );
    let pkg = create_package(&mut prog, unsafe {
        std::mem::transmute::<std::num::NonZeroU32, guff_types::PackageId>(
            std::num::NonZeroU32::new(1).unwrap(),
        )
    });

    let callee: FuncId = create_function(&mut prog, "callee".into(), None, Some(pkg));
    let caller: FuncId = create_function(&mut prog, "caller".into(), None, Some(pkg));
    prog.packages.get_mut(pkg).members.insert(
        "callee".into(),
        MemberData::Function(callee),
    );
    prog.packages.get_mut(pkg).members.insert(
        "caller".into(),
        MemberData::Function(caller),
    );
    prog.packages.get_mut(pkg).has_syntax = true;

    // caller: t0 = callee()
    let int_ty = universe[guff_types::BasicKind::Int as usize];
    {
        let mut b = guff_ssa::builder::Builder::new(&mut prog, caller);
        let entry = b.new_basic_block("entry".into());
        b.set_block(Some(entry));
        let call = b.emit(guff_ssa::instr::InstrData::Call(guff_ssa::instr::Call {
            call: guff_ssa::instr::CallCommon {
                value: Value::Function(callee),
                method: None,
                args: vec![],
                ellipsis: false,
            },
            typ: int_ty,
        }));
        b.emit(guff_ssa::instr::InstrData::Return(
            guff_ssa::instr::Return {
                results: vec![Value::Instr(call)],
            },
        ));
    }
    prog.finish_function(caller);

    let fns = all_functions(&mut prog);
    assert!(fns.contains(&caller));
    assert!(fns.contains(&callee));
}
