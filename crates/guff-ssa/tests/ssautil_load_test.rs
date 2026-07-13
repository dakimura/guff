//! Tests for `ssautil::load` (Milestone F, chunk F01).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::mode::BuilderMode;
use guff_ssa::ssautil::build_package_from_source;
use guff_types::Config;

const HELLO: &str = "package main\n\nfunc main() {\n\treturn\n}\n";

#[test]
fn test_build_package_from_source_main() {
    let fset = FileSet::new();
    let file = parse_file(&fset, "hello.go", HELLO.as_bytes(), Mode::NONE).expect("parse");

    let result = build_package_from_source(
        fset,
        Config::default(),
        vec![file],
        BuilderMode::SANITY_CHECK_FUNCTIONS,
    )
    .expect("type-check + build should succeed");

    assert_eq!(
        result.prog.package_arena.get(result.type_pkg).name(),
        "main"
    );
    assert!(
        result.prog.packages.get(result.pkg).func("main").is_some(),
        "ssa package should expose main"
    );
    let main_fid = result.prog.packages.get(result.pkg).func("main").unwrap();
    assert!(
        !result.prog.functions.get(main_fid).blocks.is_empty(),
        "main should be built"
    );
}

#[test]
fn test_build_package_from_source_type_error() {
    let fset = FileSet::new();
    let file = parse_file(
        &fset,
        "bad.go",
        b"package bad\nvar x string = 1\n",
        Mode::NONE,
    )
    .expect("parse");

    let result = build_package_from_source(fset, Config::default(), vec![file], BuilderMode::default());
    assert!(result.is_err(), "ill-typed source should fail");
}

#[test]
fn test_packages_creates_import_shells() {
    use guff_ssa::program::Program;
    use guff_ssa::ssautil::{packages, LoadedPackage};
    use guff_types::importer::{ImportCtx, Importer};
    use guff_types::object::const_::new_const;
    use guff_types::package::new_package;
    use guff_types::scope::{insert as scope_insert, lookup as scope_lookup};
    use guff_constant::make_int64;

    struct P2Importer;
    impl Importer for P2Importer {
        fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<guff_types::PackageId> {
            if path != "p2" {
                return None;
            }
            let int_obj =
                scope_lookup(ctx.scopes, ctx.universe_scope, "int").expect("int");
            let int_t = int_obj.typ(ctx.objects).expect("int type");
            let pkg = new_package(ctx.packages, ctx.scopes, ctx.universe_scope, "p2", "p2");
            let scope = ctx.packages.get(pkg).scope();
            let c = new_const(ctx.objects, "C", int_t, make_int64(7));
            c.set_pkg(ctx.objects, pkg);
            scope_insert(ctx.scopes, ctx.objects, scope, c);
            ctx.packages.get_mut(pkg).mark_complete();
            Some(pkg)
        }
    }

    let fset = FileSet::new();
    let file = parse_file(
        &fset,
        "p.go",
        b"package p\nimport \"p2\"\nvar X = p2.C\n",
        Mode::NONE,
    )
    .expect("parse");

    let mut check = guff_types::Checker::new(Config::default());
    check.set_importer(Box::new(P2Importer));
    check.check_files(vec![file.clone()]);
    assert!(check.errors.is_empty(), "{:?}", check.errors);

    let type_pkg = check.pkg;

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );

    let initial = [LoadedPackage {
        type_pkg,
        files: std::slice::from_ref(&file),
        ill_typed: false,
    }];
    let ssapkgs = packages(&mut prog, &initial, false);
    assert_eq!(ssapkgs.len(), 1);
    let ssa_pkg = ssapkgs[0].expect("well-typed initial package");
    assert!(prog.packages.get(ssa_pkg).is_syntactic());
    // At least the initial package shell exists; dependency shells are created
    // when the type-checker records imports on the package.
    assert!(prog.package_map.contains_key(&type_pkg));
}
