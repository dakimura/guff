//! CREATE-phase member population tests (Milestone D, chunk D07).
//!
//! Verifies that `populate_package_members` fills `Package.members` (name →
//! MemberData) and `Package.objects` (object → Value) from a type-checked
//! package's declarations, mirroring go/ssa's `CreatePackage` member loop:
//! package-level types/consts/vars/funcs become members; methods do not; and
//! consts/vars/funcs are additionally recorded in `objects`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::{Checker, Config, TypeData};

const SRC: &str = "\
package p

type T int

const C = 42

var V int

func F() {}

func (t T) M() {}
";

fn build() -> Program {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse failed");

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
    populate_package_members(&mut prog, ssa_pkg_id, &[file]);
    prog
}

#[test]
fn test_package_members_by_kind() {
    let prog = build();
    let ssa_pkg_id = *prog.package_map.values().next().unwrap();
    let pkg = prog.packages.get(ssa_pkg_id);

    assert!(
        matches!(pkg.members.get("T"), Some(MemberData::Type(_))),
        "type T should be a Type member; got {:?}",
        pkg.members.get("T")
    );
    assert!(
        matches!(pkg.members.get("C"), Some(MemberData::NamedConst(_))),
        "const C should be a NamedConst member; got {:?}",
        pkg.members.get("C")
    );
    assert!(
        matches!(pkg.members.get("V"), Some(MemberData::Global(_))),
        "var V should be a Global member; got {:?}",
        pkg.members.get("V")
    );
    assert!(
        matches!(pkg.members.get("F"), Some(MemberData::Function(_))),
        "func F should be a Function member; got {:?}",
        pkg.members.get("F")
    );
    // A method is not a package-level member.
    assert!(
        pkg.members.get("M").is_none(),
        "method M must not be a package member"
    );
}

#[test]
fn test_package_objects_recorded() {
    let prog = build();
    let ssa_pkg_id = *prog.package_map.values().next().unwrap();
    let pkg = prog.packages.get(ssa_pkg_id);

    let n_funcs = pkg.objects.values().filter(|v| matches!(v, Value::Function(_))).count();
    let n_globals = pkg.objects.values().filter(|v| matches!(v, Value::Global(_))).count();
    let n_consts = pkg.objects.values().filter(|v| matches!(v, Value::Const(_))).count();

    // Both F and the method M are recorded as function objects (only F is a
    // member; both appear in `objects`).
    assert_eq!(n_funcs, 2, "F and M should both be recorded in objects");
    assert_eq!(n_globals, 1, "V should be recorded in objects");
    assert_eq!(n_consts, 1, "C should be recorded in objects");
    // Types are not values, so they are never put in `objects`.
}

#[test]
fn test_global_type_is_pointer() {
    let prog = build();
    let ssa_pkg_id = *prog.package_map.values().next().unwrap();
    let pkg = prog.packages.get(ssa_pkg_id);

    let gid = match pkg.members.get("V") {
        Some(MemberData::Global(gid)) => *gid,
        other => panic!("expected V to be a Global, got {other:?}"),
    };
    let g = prog.globals.get(gid);
    // A Global holds the *address* of the variable: its type is a pointer.
    assert!(
        matches!(prog.type_arena.get(g.typ), TypeData::Pointer(_)),
        "a Global's type should be a pointer to the variable's type"
    );
    assert_eq!(
        g.object.map(|o| o.name(&prog.object_arena).to_string()).as_deref(),
        Some("V")
    );
}

const FN_SRC: &str = "package p\nfunc G(a int, b string) {}\n";

#[test]
fn test_create_params() {
    use guff::ast::Decl;

    let fset = FileSet::new();
    let file = parse_file(&fset, "g.go", FN_SRC.as_bytes(), Mode::NONE).expect("parse failed");
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

    let fd = file.decls.iter().find_map(|d| match d {
        Decl::FuncDecl(fd) => Some(fd),
        _ => None,
    }).unwrap();

    let fn_id = guff_ssa::create::create_function(&mut prog, "G".to_string(), None, Some(ssa_pkg_id));
    let sig = prog.info.defs.get(&fd.name.id).unwrap().unwrap().typ(&prog.object_arena).unwrap();
    prog.functions.get_mut(fn_id).signature = Some(sig);

    guff_ssa::create::create_params(&mut prog, fn_id);

    let f = prog.functions.get(fn_id);
    assert_eq!(f.params.len(), 2, "G has two parameters");
    let names: Vec<&str> = f.params.iter().map(|(_, p)| p.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b"]);
    // Each parameter's object maps back to its Value::Param.
    assert_eq!(f.objects.len(), 2, "both parameter objects are recorded");
    for (pid, p) in f.params.iter() {
        let obj = p.object.expect("param carries its object");
        assert_eq!(f.objects.get(&obj), Some(&Value::Param(pid)));
    }
}
