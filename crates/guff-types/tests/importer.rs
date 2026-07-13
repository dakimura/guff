//! Tests for the `Importer` plumbing (chunk 73a): a custom importer resolves
//! non-`unsafe` import paths, the resolver binds a `PkgName`, and `pkg.X`
//! selectors (value and type position) resolve the imported package's exports.

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_constant::make_int64;
use guff_types::importer::{ImportCtx, Importer};
use guff_types::named::new_named;
use guff_types::object::const_::new_const;
use guff_types::object::type_name::new_type_name;
use guff_types::package::new_package;
use guff_types::scope::{insert as scope_insert, lookup as scope_lookup};
use guff_types::{Checker, Config, PackageId};

/// A test importer that synthesises one package, `"p2"`, exporting
/// `const C int = 42` and `type T int`. Every other path is unresolvable.
struct TestImporter;

impl Importer for TestImporter {
    fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<PackageId> {
        if path != "p2" {
            return None;
        }
        // `int` from the universe, used as the type of the exports.
        let int_obj =
            scope_lookup(ctx.scopes, ctx.universe_scope, "int").expect("universe has int");
        let int_t = int_obj.typ(ctx.objects).expect("int has a type");

        let pkg = new_package(ctx.packages, ctx.scopes, ctx.universe_scope, "p2", "p2");
        let pkg_scope = ctx.packages.get(pkg).scope();

        // const C int = 42
        let c = new_const(ctx.objects, "C", int_t, make_int64(42));
        c.set_pkg(ctx.objects, pkg);
        scope_insert(ctx.scopes, ctx.objects, pkg_scope, c);

        // type T int
        let t_obj = new_type_name(ctx.objects, "T", None);
        t_obj.set_pkg(ctx.objects, pkg);
        new_named(ctx.types, ctx.objects, t_obj, Some(int_t), Vec::new());
        scope_insert(ctx.scopes, ctx.objects, pkg_scope, t_obj);

        ctx.packages.get_mut(pkg).mark_complete();
        Some(pkg)
    }
}

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_with_importer(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(TestImporter));
    check.check_files(vec![parse(src)]);
    check
}

#[test]
fn imported_const_resolves_in_value_position() {
    let check = check_with_importer("package p\nimport \"p2\"\nconst D = p2.C\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // D inherited p2.C's value (42).
    let pkg_scope = check.packages.get(check.pkg).scope();
    let d = scope_lookup(&check.scopes, pkg_scope, "D").expect("const D declared");
    match check.objects.get(d) {
        guff_types::arena::ObjectData::Const(c) => {
            let (v, exact) = guff_constant::int64_val(c.val());
            assert!(exact && v == 42, "D should equal p2.C == 42, got {v}");
        }
        _ => panic!("D is not a const"),
    }
}

#[test]
fn imported_type_resolves_in_type_position() {
    // `var x p2.T` must type-check: the qualified type resolves to p2.T.
    let check = check_with_importer("package p\nimport \"p2\"\nvar x p2.T\nvar _ = x\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn unresolvable_import_is_left_unbound() {
    // "nope" is not resolvable; referencing it fails (undefined package name).
    let check = check_with_importer("package p\nimport \"nope\"\nvar _ = nope.X\n");
    assert!(
        !check.errors.is_empty(),
        "expected an error referencing the unresolved package"
    );
}

#[test]
fn no_importer_leaves_nonunsafe_imports_unbound() {
    // Without an importer, only `unsafe` resolves; `p2` stays unbound.
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse("package p\nimport \"p2\"\nvar _ = p2.C\n")]);
    assert!(
        !check.errors.is_empty(),
        "p2 should be unresolved without an importer"
    );
}

#[test]
fn same_import_path_is_cached() {
    // Importing p2 once caches it; a package-level and body use both resolve to
    // the same package (no duplicate/mismatch), so the program type-checks.
    let check = check_with_importer(
        "package p\nimport \"p2\"\nconst D = p2.C\nfunc f() { var y p2.T; _ = y }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(check.import_cache.len(), 1, "p2 imported exactly once");
}

// ---- built-in source importer -------------------------------------------

/// Type-check `main`, resolving each `(path, src)` dependency from source.
fn check_with_sources(main: &str, deps: &[(&str, &str)]) -> Checker {
    let mut check = Checker::new(Config::default());
    for (path, src) in deps {
        check.add_dependency_source(*path, vec![parse(src)]);
    }
    check.check_files(vec![parse(main)]);
    check
}

#[test]
fn source_dependency_is_checked_and_used() {
    let dep = "package p2\nconst C = 42\ntype T int\nfunc F() int { return C }\n";
    let main = "package p\n\
                import \"p2\"\n\
                const D = p2.C\n\
                var x p2.T\n\
                var _ = x\n\
                var _ = p2.F()\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // p2.C (== 42) flowed into D.
    let pkg_scope = check.packages.get(check.pkg).scope();
    let d = scope_lookup(&check.scopes, pkg_scope, "D").expect("const D declared");
    match check.objects.get(d) {
        guff_types::arena::ObjectData::Const(c) => {
            let (v, exact) = guff_constant::int64_val(c.val());
            assert!(exact && v == 42, "D should equal p2.C == 42, got {v}");
        }
        _ => panic!("D is not a const"),
    }
}

#[test]
fn source_dependency_error_surfaces() {
    // The dependency has a type error; it should surface (not be silently
    // dropped) when the main package imports it.
    let dep = "package p2\nvar C int = \"not an int\"\n";
    let main = "package p\nimport \"p2\"\nvar _ = p2.C\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        !check.errors.is_empty(),
        "the dependency's type error should surface"
    );
}

#[test]
fn transitive_source_dependencies_resolve() {
    // p3 imports p2; main imports p3.
    let p2 = "package p2\nconst V = 7\n";
    let p3 = "package p3\nimport \"p2\"\nconst W = p2.V\n";
    let main = "package p\nimport \"p3\"\nconst Z = p3.W\n";
    let check = check_with_sources(main, &[("p2", p2), ("p3", p3)]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let pkg_scope = check.packages.get(check.pkg).scope();
    let z = scope_lookup(&check.scopes, pkg_scope, "Z").expect("const Z declared");
    match check.objects.get(z) {
        guff_types::arena::ObjectData::Const(c) => {
            let (v, exact) = guff_constant::int64_val(c.val());
            assert!(exact && v == 7, "Z should equal p3.W == p2.V == 7, got {v}");
        }
        _ => panic!("Z is not a const"),
    }
    // Both p2 and p3 imported.
    assert_eq!(check.import_cache.len(), 2);
}

#[test]
fn import_cycle_does_not_hang() {
    // pa imports pb, pb imports pa. The cycle guard must break the recursion
    // rather than loop forever; the program still terminates.
    let pa = "package pa\nimport \"pb\"\nconst A = pb.B\n";
    let pb = "package pb\nimport \"pa\"\nconst B = pa.A\n";
    let main = "package p\nimport \"pa\"\nvar _ = pa.A\n";
    let check = check_with_sources(main, &[("pa", pa), ("pb", pb)]);
    // We only assert termination here (a cycle is an error condition).
    let _ = check.errors.len();
}

#[test]
fn import_name_clashes_with_package_decl() {
    // A package-level `var p2` collides with the imported package name `p2`.
    let check = check_with_importer("package p\nimport \"p2\"\nvar p2 int\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateDecl
                && e.msg.contains("already declared through import")),
        "expected an import-vs-decl clash error; got: {:?}",
        check.errors
    );
}

#[test]
fn renamed_import_name_clashes_with_package_decl() {
    // The alias `x` collides with a package-level `func x`.
    let check = check_with_importer("package p\nimport x \"p2\"\nfunc x() {}\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == guff_types_errors::Code::DuplicateDecl
                && e.msg.contains("x already declared through import")),
        "expected a clash on alias x; got: {:?}",
        check.errors
    );
}

#[test]
fn import_without_clash_is_ok() {
    // Distinct names: the import `p2` and a package-level `q` don't collide.
    let check = check_with_importer("package p\nimport \"p2\"\nvar q = p2.C\n");
    assert!(
        !check
            .errors
            .iter()
            .any(|e| e.msg.contains("already declared through import")),
        "no clash expected; got: {:?}",
        check.errors
    );
}
