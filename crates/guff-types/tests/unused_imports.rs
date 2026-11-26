//! Tests for the unused-import check (chunk 74): an `import` whose package
//! name is never used in a qualified identifier (`pkg.X`) is reported with
//! `UnusedImport`, mirroring Go's `Checker.unusedImports`/`errorUnusedPkg`.

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
use guff_types_errors::Code;

/// A test importer that synthesises one package, `"p2"`, exporting
/// `const C int = 42` and `type T int`. Every other path is unresolvable.
struct TestImporter;

impl Importer for TestImporter {
    fn import(&mut self, ctx: &mut ImportCtx<'_>, path: &str) -> Option<PackageId> {
        if path != "p2" {
            return None;
        }
        let int_obj =
            scope_lookup(ctx.scopes, ctx.universe_scope, "int").expect("universe has int");
        let int_t = int_obj.typ(ctx.objects).expect("int has a type");

        let pkg = new_package(ctx.packages, ctx.scopes, ctx.universe_scope, "p2", "p2");
        let pkg_scope = ctx.packages.get(pkg).scope();

        let c = new_const(ctx.objects, "C", int_t, make_int64(42));
        c.set_pkg(ctx.objects, pkg);
        scope_insert(ctx.scopes, ctx.objects, pkg_scope, c);

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

fn check_plain(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
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
fn imported_and_used_is_not_reported() {
    let check = check_with_importer("package p\nimport \"p2\"\nconst D = p2.C\n");
    assert!(
        unused_import_errors(&check).is_empty(),
        "used import must not be reported: {:?}",
        check.errors
    );
}

#[test]
fn imported_and_not_used_is_reported() {
    let check = check_with_importer("package p\nimport \"p2\"\n");
    let errs = unused_import_errors(&check);
    assert_eq!(
        errs,
        vec!["\"p2\" imported and not used"],
        "unused import must be reported once; all errors: {:?}",
        check.errors
    );
}

#[test]
fn renamed_import_not_used_shows_alias() {
    let check = check_with_importer("package p\nimport x \"p2\"\n");
    let errs = unused_import_errors(&check);
    assert_eq!(
        errs,
        vec!["\"p2\" imported as x and not used"],
        "renamed unused import must name the alias; all errors: {:?}",
        check.errors
    );
}

#[test]
fn renamed_import_used_is_not_reported() {
    let check = check_with_importer("package p\nimport x \"p2\"\nconst D = x.C\n");
    assert!(
        unused_import_errors(&check).is_empty(),
        "used renamed import must not be reported: {:?}",
        check.errors
    );
}

#[test]
fn used_in_type_position_is_not_reported() {
    // `var v p2.T` refers to `p2` via a qualified type selector.
    let check = check_with_importer("package p\nimport \"p2\"\nvar v p2.T\n");
    assert!(
        unused_import_errors(&check).is_empty(),
        "import used in a type position must not be reported: {:?}",
        check.errors
    );
}

#[test]
fn unsafe_imported_and_not_used_is_reported() {
    // `unsafe` needs no importer; an unused `import "unsafe"` is still flagged.
    let check = check_plain("package p\nimport \"unsafe\"\n");
    let errs = unused_import_errors(&check);
    assert_eq!(
        errs,
        vec!["\"unsafe\" imported and not used"],
        "unused unsafe import must be reported; all errors: {:?}",
        check.errors
    );
}

#[test]
fn unsafe_imported_and_used_is_not_reported() {
    let check = check_plain("package p\nimport \"unsafe\"\nvar p unsafe.Pointer\n");
    assert!(
        unused_import_errors(&check).is_empty(),
        "used unsafe import must not be reported: {:?}",
        check.errors
    );
}

#[test]
fn blank_import_is_not_reported() {
    // `import _ "p2"` is imported for side effects only — never "unused".
    let check = check_with_importer("package p\nimport _ \"p2\"\n");
    assert!(
        unused_import_errors(&check).is_empty(),
        "blank import must not be reported as unused: {:?}",
        check.errors
    );
}

#[test]
fn blank_import_still_resolves_dependency_errors() {
    // A blank import still triggers checking the dependency, so its type
    // errors surface (they are not silently dropped).
    let mut check = Checker::new(Config::default());
    check.add_dependency_source("p2", vec![parse("package p2\nvar C int = \"bad\"\n")]);
    check.check_files(vec![parse("package p\nimport _ \"p2\"\n")]);
    assert!(
        !check.errors.is_empty(),
        "blank import of a broken dependency should surface its error"
    );
    // The only errors should come from the dependency, not an unused-import
    // report for the blank import itself.
    assert!(
        unused_import_errors(&check).is_empty(),
        "blank import must not be flagged unused: {:?}",
        check.errors
    );
}
