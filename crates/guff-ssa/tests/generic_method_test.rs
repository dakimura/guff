//! Generic method instantiation data-model tests (E29).

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::function::BuildStrategy;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::{basic::init_universe, Checker, Config};

const SRC: &str = "\
package p

type G[T any] int

func (g G[T]) Zero() T {
	var z T
	return z
}
";

fn setup() -> (Program, guff_ssa::ids::FuncId, guff_types::TypeId) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    let mut prog = Program::new(
        BuilderMode::INSTANTIATE_GENERICS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);

    let origin = prog
        .packages
        .get(ssa_pkg)
        .objects
        .iter()
        .find(|(o, v)| {
            o.name(&prog.object_arena) == "Zero"
                && matches!(v, guff_ssa::value::Value::Function(_))
        })
        .map(|(_, v)| match v {
            guff_ssa::value::Value::Function(fid) => *fid,
            _ => unreachable!(),
        })
        .expect("Zero method in objects");
    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "Zero" => Some(fd),
            _ => None,
        })
        .unwrap();
    {
        let f = prog.functions.get_mut(origin);
        f.from_syntax = fd.body.is_some();
        f.syntax_decl = Some(fd.clone());
    }

    let (_, table) = init_universe();
    let int_ty = table[guff_types::BasicKind::Int as usize];
    (prog, origin, int_ty)
}

/// `create_instance` on a method with `rtargs=[int]` picks FromSyntax and
/// substitutes the result type to `int`.
#[test]
fn test_method_create_instance_concrete() {
    let (mut prog, origin, int_ty) = setup();
    let inst = prog.create_instance(origin, &[int_ty], &[]);
    let f = prog.functions.get(inst);
    assert_eq!(f.build_strategy, BuildStrategy::FromSyntax);
    assert!(f.subst.is_some());
    assert_eq!(f.recv_type_args, vec![int_ty]);
    assert!(f.signature.is_some());
}

/// Cached `instance` returns the same `FuncId`.
#[test]
fn test_method_instance_cached() {
    let (mut prog, origin, int_ty) = setup();
    let a = prog.instance(origin, &[int_ty], &[]);
    let b = prog.instance(origin, &[int_ty], &[]);
    assert_eq!(a, b);
    assert_eq!(prog.functions.get(origin).generic_instances.len(), 1);
}
