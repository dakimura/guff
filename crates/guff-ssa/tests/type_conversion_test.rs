//! Explicit `T(x)` type-conversion lowering (go/ssa CallExpr IsType branch).

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::builder::build_package;
use guff_ssa::create::{create_package, populate_package_members};
use guff_ssa::instr::InstrData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_types::{Checker, Config};

/// `int64(n)` must lower via Convert/ChangeType, not panic trying to resolve
/// the `int64` TypeName as a callable.
#[test]
fn test_explicit_type_conversion_int64() {
    const SRC: &str = r#"
package p
func f(n int) int64 {
	return int64(n)
}
"#;
    let fset = FileSet::new();
    let file = parse_file(&fset, "conv.go", SRC.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    assert!(
        check.errors.is_empty(),
        "typecheck errors: {:?}",
        check.errors
    );

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );
    prog.set_fset(fset);
    let ssa_pkg = create_package(&mut prog, check.pkg);
    populate_package_members(&mut prog, ssa_pkg, &[file.clone()]);
    build_package(&mut prog, ssa_pkg, &[file]);

    let has_conv = prog.functions.iter().any(|(_, f)| {
        f.name == "f"
            && f.instrs.iter().any(|(_, data)| {
                matches!(data, InstrData::Convert(_) | InstrData::ChangeType(_))
            })
    });
    assert!(
        has_conv,
        "expected Convert or ChangeType in f for int64(n)"
    );
}
