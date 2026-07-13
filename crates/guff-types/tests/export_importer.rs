//! Tests that [`guff_exportdata::ExportImporter`] resolves exports from `.a` files.

use std::path::PathBuf;

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_constant::int64_val;
use guff_exportdata::ExportImporter;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config};

fn simple_export_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../guff-exportdata/tests/testdata/export/simple/simple.a")
}

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

#[test]
fn export_importer_resolves_const_and_type_from_export_data() {
    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset);
    importer.set_path("example.com/simple", simple_export_path());

    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));

    let src = r#"
package main

import "example.com/simple"

const D = simple.X
var x simple.T
var _ = x
var _ = D
"#;
    check.check_files(vec![parse(src)]);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let pkg_scope = check.packages.get(check.pkg).scope();
    let d = scope_lookup(&check.scopes, pkg_scope, "D").expect("const D declared");
    match check.objects.get(d) {
        guff_types::arena::ObjectData::Const(c) => {
            let (v, exact) = int64_val(c.val());
            assert!(exact && v == 42, "D should equal simple.X == 42, got {v}");
        }
        _ => panic!("D is not a const"),
    }
}
