//! `Program::emit_const(None, T)` — go/ssa's `NewConst` normalization.
//!
//! A zero value whose type set agrees on what its zero *looks like* carries
//! that value: `false`, `0`, `""`. Everything else keeps `None`, which is what
//! "nil" means. Without this, "no value" meant two different things and every
//! consumer had to tell them apart: `unparam` compared a call passing
//! `var s string` against one passing `""` and answered "different constants".

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::value::Value;
use guff_types::{Checker, Config};

const SRC: &str = "\
package p

type MyInt int

type S struct{ A int }

var (
	B  bool
	I  int
	F  float64
	St string
	P  *int
	Iface any
	Sl []int
	M  map[string]int
	Str S
	MI MyInt
)
";

/// The value `emit_const(None, T)` gives each of these types, by the name the
/// source spells them with.
fn zero_values() -> Vec<(String, Option<String>)> {
    let fset = FileSet::new();
    let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);

    // The declared type of each package-level var, taken from the checker's
    // `defs` before `Program::new` takes ownership of it.
    let mut declared: Vec<(String, guff_types::TypeId)> = Vec::new();
    guff::walk::preorder(guff::walk::NodeRef::File(&file), |n| {
        if let guff::walk::NodeRef::ValueSpec(spec) = n {
            for id in &spec.names {
                if let Some(Some(obj)) = check.info.defs.get(&id.id) {
                    if let Some(t) = obj.typ(&check.objects) {
                        declared.push((id.name.clone(), t));
                    }
                }
            }
        }
        true
    });

    let mut prog = Program::new(
        BuilderMode::default(),
        check.info,
        check.types,
        check.objects,
        check.packages,
    );

    let mut out = Vec::new();
    for (name, typ) in declared {
        let Value::Const(cid) = prog.emit_const(None, typ) else {
            panic!("emit_const did not answer a constant");
        };
        let val = prog.constants.get(cid).val.as_ref().map(|v| v.to_string());
        out.push((name, val));
    }
    out
}

#[test]
fn zero_constants_carry_the_value_go_ssa_gives_them() {
    let got = zero_values();
    // Counted and spelled out. `soleTypeKind` folds float and complex into
    // `IsInteger`, so a `float64` zero is the *integer* 0 there too — this
    // follows it rather than inventing a float zero.
    assert_eq!(
        got,
        vec![
            ("B".to_string(), Some("false".to_string())),
            ("I".to_string(), Some("0".to_string())),
            ("F".to_string(), Some("0".to_string())),
            ("St".to_string(), Some("\"\"".to_string())),
            ("P".to_string(), None),
            ("Iface".to_string(), None),
            ("Sl".to_string(), None),
            ("M".to_string(), None),
            ("Str".to_string(), None),
            // A named type is normalized by its underlying.
            ("MI".to_string(), Some("0".to_string())),
        ],
        "{got:?}"
    );
}
