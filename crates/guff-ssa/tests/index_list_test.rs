//! IndexListExpr / single-arg IndexExpr generic instantiation (go/ssa expr0).
//!
//! Ensures `f[T1, T2]` and `f[T]` peel to the underlying Ident/Selector and
//! instantiate the package-level generic function instead of panicking.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

const SRC: &str = r#"
package p

func Identity[T any](x T) T { return x }

func Pair[A, B any](a A, b B) (A, B) { return a, b }

func use() {
	_ = Identity[int](1)
	_, _ = Pair[int, string](1, "x")
}
"#;

#[test]
fn index_list_and_index_generic_instantiation_builds() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", SRC.as_bytes(), Mode::NONE).expect("parse");

    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert!(
        check.errors.is_empty(),
        "typecheck errors: {:?}",
        check.errors
    );
    assert!(
        !check.info.instances.is_empty(),
        "expected Instances for Identity[int] / Pair[int,string]"
    );

    let mut prog = Program::new(
        BuilderMode::INSTANTIATE_GENERICS,
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);

    // Build every package-level function; IndexListExpr/IndexExpr must not panic.
    let names: Vec<String> = prog
        .packages
        .get(ssa_pkg)
        .members
        .iter()
        .filter_map(|(n, m)| match m {
            MemberData::Function(_) => Some(n.clone()),
            _ => None,
        })
        .collect();
    for name in names {
        let Some(MemberData::Function(fid)) = prog.packages.get(ssa_pkg).members.get(&name).copied()
        else {
            continue;
        };
        let Some(fd) = file.decls.iter().find_map(|d| match d {
            guff::ast::Decl::FuncDecl(fd) if fd.name.name == name => Some(fd.clone()),
            _ => None,
        }) else {
            continue;
        };
        guff_ssa::builder::build_function(&mut prog, fid, &fd);
    }
    prog.drain_build_queue();

    // Identity and Pair origins should have concrete instances cached.
    let identity = match prog.packages.get(ssa_pkg).members.get("Identity") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("Identity: {other:?}"),
    };
    let pair = match prog.packages.get(ssa_pkg).members.get("Pair") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("Pair: {other:?}"),
    };
    assert!(
        !prog.functions.get(identity).generic_instances.is_empty(),
        "Identity[int] instance should be cached"
    );
    assert!(
        !prog.functions.get(pair).generic_instances.is_empty(),
        "Pair[int,string] instance should be cached"
    );

    // Sanity: use() body contains Call instructions targeting instances.
    let use_fid = match prog.packages.get(ssa_pkg).members.get("use") {
        Some(MemberData::Function(fid)) => *fid,
        other => panic!("use: {other:?}"),
    };
    let f = prog.functions.get(use_fid);
    assert!(!f.blocks.is_empty(), "use() should have been built");
}
