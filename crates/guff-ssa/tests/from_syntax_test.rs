//! FromSyntax generic instance body build tests (Milestone E, chunk E28).

use guff::ast::Decl;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::function::BuildStrategy;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::print::disassemble_function;
use guff_ssa::program::Program;
use guff_types::{basic::init_universe, Checker, Config};

const SRC_IDENTITY: &str = "\
package p

func F[T any](x T) T {
	return x
}
";

const SRC_BINOP: &str = "\
package p

func F[T any](x T) T {
	return x + x
}
";

fn setup_generic(src: &str) -> (Program, guff_types::TypeId, guff_ssa::ids::FuncId) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    let type_pkg_id = check.pkg;

    let mut prog = Program::new(
        BuilderMode::INSTANTIATE_GENERICS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, type_pkg_id);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);

    let origin = match prog.packages.get(ssa_pkg).members.get("F") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("expected F function, got {other:?}"),
    };

    let fd = file
        .decls
        .iter()
        .find_map(|d| match d {
            Decl::FuncDecl(fd) if fd.name.name == "F" => Some(fd),
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
    (prog, int_ty, origin)
}

/// A concrete `F[int]` instance built via FromSyntax returns its argument.
#[test]
fn test_build_from_syntax_identity() {
    let (mut prog, int_ty, origin) = setup_generic(SRC_IDENTITY);
    let inst = prog.instance(origin, &[], &[int_ty]);
    assert_eq!(prog.functions.get(inst).build_strategy, BuildStrategy::FromSyntax);

    prog.build_instance(inst);

    let f = prog.functions.get(inst);
    assert!(!f.blocks.is_empty(), "instance has a body");
    assert!(f.subst.is_none(), "subst cleared after build");
    assert_eq!(f.params.len(), 1);

    let text = disassemble_function(f, &prog);
    assert!(text.contains("func F[int](x int) int:"), "header:\n{text}");
    assert!(text.contains("return"), "return missing:\n{text}");
    assert!(!text.contains("F("), "should not call origin wrapper:\n{text}");
}

/// FromSyntax applies type substitution to binary expressions.
#[test]
fn test_build_from_syntax_binop() {
    let (mut prog, int_ty, origin) = setup_generic(SRC_BINOP);
    let inst = prog.instance(origin, &[], &[int_ty]);
    prog.build_instance(inst);

    let text = disassemble_function(prog.functions.get(inst), &prog);
    assert!(text.contains(" + "), "expected add in:\n{text}");
    assert!(text.contains("return"), "return missing:\n{text}");
}
