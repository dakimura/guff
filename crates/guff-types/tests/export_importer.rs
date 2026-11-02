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

/// go1.27.3 export data (pkgbits V5) for go1.27's
/// `go/internal/gcimporter/testdata/genmeth.go`: generic methods arrive as a
/// second method list, and the V5 index puts them back in source order.
#[test]
fn export_importer_reads_go127_generic_methods() {
    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset.clone());
    importer.set_path(
        "example.com/genmeth/genmeth",
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-exportdata/tests/testdata/export/genmeth/genmeth.a"),
    );
    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));
    let src = r#"
package main

import "example.com/genmeth/genmeth"

func use() {
	var o genmeth.Option[int]
	var e genmeth.Either[int, string] = o.GetOrDefault("x")
	_ = e
	var l genmeth.List[int]
	var z genmeth.List[bool] = l.Zip(genmeth.List[string]{}, func(int, string) bool { return true })
	_ = z
}
"#;
    let mut file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    file.go_version = "go1.27".to_string();
    check.check_files(vec![file]);
    assert!(check.errors.is_empty(), "unexpected errors: {:?}", check.errors);

    let imp = check.packages.get(check.pkg).imports()[0];
    let scope = check.packages.get(imp).scope();
    // (type, [(method, own type parameter count)]) in source order.
    let want: &[(&str, &[(&str, usize)])] = &[
        ("Option", &[("Get", 0), ("GetOrDefault", 1), ("MapIfPresent", 1)]),
        ("Box", &[("Get", 0), ("Set", 0)]),
        ("List", &[("Map", 1), ("FlatMap", 1), ("Zip", 2)]),
        (
            "BiList",
            &[("Flip", 0), ("MapKeys", 1), ("MapValues", 1), ("FlatMapValues", 1), ("MapEntries", 2)],
        ),
    ];
    for (tname, methods) in want {
        let obj = scope_lookup(&check.scopes, scope, tname).unwrap_or_else(|| panic!("missing {tname}"));
        let named = obj.typ(&check.objects).expect("type");
        let got: Vec<(String, usize)> = (0..guff_types::named::named_num_methods(&check.types, named))
            .map(|i| {
                let m = guff_types::named::named_method(&check.types, named, i);
                let sig = m.typ(&check.objects).expect("signature");
                let n = guff_types::signature::signature_type_params(&check.types, sig).map_or(0, |l| l.len());
                (m.name(&check.objects).to_string(), n)
            })
            .collect();
        let want: Vec<(String, usize)> = methods.iter().map(|(n, k)| (n.to_string(), *k)).collect();
        assert_eq!(got, want, "methods of {tname}");
    }
    // The export data also lists `Option.GetOrDefault` at package scope; it
    // must not be declared there.
    assert!(scope_lookup(&check.scopes, scope, "Option.GetOrDefault").is_none());
}

/// The root package's name comes from the export data, not from the last
/// element of its path: `example.com/vname/v2` is `vname` (as
/// `math/rand/v2` is `rand`). guff used to name it `v2`, so `vname.IntN` was
/// undefined and the importing package went ill-typed.
#[test]
fn export_importer_names_a_versioned_path_from_the_export_data() {
    let fset = FileSet::new();
    let mut importer = ExportImporter::with_fset(fset.clone());
    importer.set_path(
        "example.com/vname/v2",
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-exportdata/tests/testdata/export/vname/vname.a"),
    );
    let mut check = Checker::new(Config::default());
    check.set_importer(Box::new(importer));
    let src = r#"
package main

import "example.com/vname/v2"

var _ = vname.IntN(1) + vname.R{}.N(2)
"#;
    let mut file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    file.go_version = "go1.27".to_string();
    check.check_files(vec![file]);
    assert!(check.errors.is_empty(), "unexpected errors: {:?}", check.errors);
}
