//! Tests for dot-imports (chunk 77): `import . "path"` merges the imported
//! package's exported objects into the file scope, so they can be referred to
//! without qualification. Uses the built-in source importer.

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_with_sources(main: &str, deps: &[(&str, &str)]) -> Checker {
    let mut check = Checker::new(Config::default());
    for (path, src) in deps {
        check.add_dependency_source(*path, vec![parse(src)]);
    }
    check.check_files(vec![parse(main)]);
    check
}

fn unused_import_errors(check: &Checker) -> Vec<&str> {
    check
        .errors
        .iter()
        .filter(|e| e.code == Code::UnusedImport)
        .map(|e| e.msg.as_str())
        .collect()
}

#[test]
fn dot_imported_value_resolves_unqualified() {
    // `C` (from p2) is used directly, without a `p2.` qualifier.
    let dep = "package p2\nconst C = 42\n";
    let main = "package p\nimport . \"p2\"\nconst D = C\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let pkg_scope = check.packages.get(check.pkg).scope();
    let d = scope_lookup(&check.scopes, pkg_scope, "D").expect("const D declared");
    match check.objects.get(d) {
        guff_types::arena::ObjectData::Const(c) => {
            let (v, exact) = guff_constant::int64_val(c.val());
            assert!(exact && v == 42, "D should equal dot-imported C == 42, got {v}");
        }
        _ => panic!("D is not a const"),
    }
}

#[test]
fn dot_imported_type_resolves_unqualified() {
    // `T` (from p2) names a type without qualification.
    let dep = "package p2\ntype T int\n";
    let main = "package p\nimport . \"p2\"\nvar v T\nvar _ = v\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn used_dot_import_is_not_reported_unused() {
    let dep = "package p2\nconst C = 1\n";
    let main = "package p\nimport . \"p2\"\nconst D = C\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        unused_import_errors(&check).is_empty(),
        "used dot-import must not be reported unused: {:?}",
        check.errors
    );
}

#[test]
fn unused_dot_import_is_reported() {
    // Nothing from p2 is referenced.
    let dep = "package p2\nconst C = 1\n";
    let main = "package p\nimport . \"p2\"\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    let errs = unused_import_errors(&check);
    assert_eq!(
        errs,
        vec!["\"p2\" imported and not used"],
        "unused dot-import must be reported; all errors: {:?}",
        check.errors
    );
}

#[test]
fn dot_import_only_merges_exported_names() {
    // p2's unexported `secret` must NOT be visible in the importing file, so
    // referencing it is an undefined-name error.
    let dep = "package p2\nconst C = 1\nconst secret = 2\n";
    let main = "package p\nimport . \"p2\"\nconst D = secret\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == Code::UndeclaredName && e.msg.contains("secret")),
        "unexported name must not be dot-imported; got: {:?}",
        check.errors
    );
}

#[test]
fn dot_import_clashing_with_local_decl_is_reported() {
    // A package-level `C` collides with the dot-imported `C`.
    let dep = "package p2\nconst C = 1\n";
    let main = "package p\nimport . \"p2\"\nconst C = 2\n";
    let check = check_with_sources(main, &[("p2", dep)]);
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == Code::DuplicateDecl && e.msg.contains("dot-import")),
        "clash between dot-import and local decl must be reported; got: {:?}",
        check.errors
    );
}
