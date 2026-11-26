//! Regression: package-level vars with an explicit type whose initializer
//! forward-references a later var (stdlib `time.UTC = &utcLoc`) must keep the
//! declared type. Also covers inferred forward refs (`var y = x; var x = 3`).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_types::predicates::is_valid;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config, TypeKind};

fn check_src(src: &str) -> Checker {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", src.as_bytes(), Mode::NONE).unwrap();
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file]);
    check
}

#[test]
fn forward_var_keeps_explicit_pointer_type() {
    // Mirrors time/zoneinfo.go:
    //   var UTC *Location = &utcLoc
    //   var utcLoc = Location{name: "UTC"}
    let check = check_src(
        "package p\n\
         type Location struct{ name string }\n\
         var UTC *Location = &utcLoc\n\
         var utcLoc = Location{name: \"UTC\"}\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let scope = check.packages.get(check.pkg).scope();
    let utc = scope_lookup(&check.scopes, scope, "UTC").expect("UTC");
    let t = utc.typ(&check.objects).expect("UTC typ");
    assert!(
        is_valid(&check.types, t),
        "UTC must keep declared *Location, got invalid"
    );
    assert_eq!(t.kind(&check.types), TypeKind::Pointer);
}

#[test]
fn forward_inferred_var_adopts_later_type() {
    let check = check_src("package p\nvar y = x\nvar x = 3\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let scope = check.packages.get(check.pkg).scope();
    let y = scope_lookup(&check.scopes, scope, "y").expect("y");
    let t = y.typ(&check.objects).expect("y typ");
    assert!(is_valid(&check.types, t), "y should adopt x's type");
    assert_eq!(t.kind(&check.types), TypeKind::Basic);
}
