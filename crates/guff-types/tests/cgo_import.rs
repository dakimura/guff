//! Tests for `import "C"` stub (`FakeImportC`).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_types::{Checker, Config};
use guff_types_errors::Code;

fn check_src(src: &str, fake_import_c: bool) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config {
        fake_import_c,
        ..Config::default()
    });
    check.check_files(vec![file]);
    check
}

#[test]
fn fake_import_c_allows_blank_c_import() {
    let check = check_src(
        "package p\n\
         import _ \"C\"\n\
         func f() {}\n",
        true,
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn import_c_without_fake_import_c_leaves_c_unbound() {
    let check = check_src(
        "package p\n\
         import \"C\"\n\
         func f() { _ = C }\n",
        false,
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.code == Code::UndeclaredName),
        "expected undeclared C, got: {:?}",
        check.errors
    );
}
