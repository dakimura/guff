//! Every identifier that *declares* something must appear in `Info.Defs`.
//!
//! go/types records the defining identifier through `Checker.declare`, which
//! takes the ident alongside the object. guff's `declare` takes only the
//! object, so each declaration site has to remember to call `record_def`
//! itself — and two of them did not: a `const` inside a function body (the
//! `var` arm next to it did) and a type parameter. Both were found by accident,
//! through varnamelen, which is the only linter whose fixture happened to reach
//! them (COMPAT-HARDENING 2026-08-24, 続き 31).
//!
//! Analyzers start from `Defs` constantly, and a missing entry is silence
//! rather than an error, so this enumerates the declaration forms instead of
//! waiting for a linter to stumble over the next one.
//!
//! Named imports are not covered here: this harness has no importer, so an
//! import cannot resolve and the file would not type-check.

use std::collections::HashSet;

use guff::ast::File;
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::walk::{inspect, NodeRef};

use guff_types::{Checker, Config};

fn check_src(src: &str) -> (Checker, File) {
    let fset = FileSet::new();
    let file = parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse");
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    (check, file)
}

/// The names of every identifier recorded in `Info.Defs`.
fn defined_names(check: &Checker, file: &File) -> HashSet<String> {
    let mut by_id = std::collections::HashMap::new();
    inspect(NodeRef::File(file), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            by_id.insert(id.id(), id.name.clone());
        }
        true
    });
    check
        .info
        .defs
        .keys()
        .filter_map(|id| by_id.get(id).cloned())
        .collect()
}

const SRC: &str = r#"
package p

const packageConst = 1

var packageVar = 2

type PackageType struct {
	StructField int
	embedded
}

type embedded struct{}

type Iface interface {
	IfaceMethod(ifaceParam int) error
}

func packageFunc(param int, variadic ...string) (namedResult int) {
	localConst := 0
	_ = localConst

	const bodyConst = 1
	_ = bodyConst

	var bodyVar = 2
	_ = bodyVar

	shortVar := 3
	_ = shortVar

	for rangeKey, rangeValue := range []int{1} {
		_, _ = rangeKey, rangeValue
	}

	if ifVar := 4; ifVar > 0 {
		_ = ifVar
	}

	switch switchVar := 5; switchVar {
	default:
	}

	var anyVal any
	switch typeSwitchVar := anyVal.(type) {
	default:
		_ = typeSwitchVar
	}

	closure := func(closureParam int) {}
	_ = closure

Label:
	for {
		break Label
	}

	return 0
}

func (receiver PackageType) Method() {}

func generic[TypeParam any](v TypeParam) TypeParam { return v }
"#;

/// The declaration forms guff records today. A name moving out of this list is
/// a regression; a name moving in is a hole closed.
const EXPECTED: &[&str] = &[
    "packageConst",
    "packageVar",
    "PackageType",
    "StructField",
    "embedded",
    "Iface",
    "IfaceMethod",
    "packageFunc",
    "param",
    "variadic",
    "namedResult",
    "bodyConst",
    "bodyVar",
    "shortVar",
    "localConst",
    "rangeKey",
    "rangeValue",
    "ifVar",
    "switchVar",
    "closure",
    "closureParam",
    "receiver",
    "Method",
    "generic",
    "TypeParam",
    "v",
];

/// Forms go/types records and guff does not, each with the reason.
///
/// This is a ratchet, not a permission slip: `known_gaps_are_still_gaps` fails
/// when one of these starts working, so closing a hole forces the name up into
/// `EXPECTED` and the entry here to be deleted.
const KNOWN_MISSING: &[(&str, &str)] = &[(
    "Label",
    "go/types allocates a *Label object (`labels.go`, `check.recordDef(s.Label, lbl)`); \
     guff's label pass only tracks names and positions in a map, and `ObjectData` has no \
     Label variant to record. Adding one touches the object arena and its size guard, and \
     nothing reads it yet — so it is named here rather than half-built.",
)];

#[test]
fn every_declaration_form_records_its_defining_ident() {
    let (check, file) = check_src(SRC);
    assert!(check.errors.is_empty(), "unexpected errors: {:?}", check.errors);

    let got = defined_names(&check, &file);
    let missing: Vec<&str> = EXPECTED.iter().copied().filter(|n| !got.contains(*n)).collect();
    assert!(
        missing.is_empty(),
        "these declarations are absent from Info.Defs, so anything starting \
         from Defs cannot see them: {missing:?}"
    );
}

#[test]
fn known_gaps_are_still_gaps() {
    let (check, file) = check_src(SRC);
    let got = defined_names(&check, &file);
    for (name, why) in KNOWN_MISSING {
        assert!(
            !got.contains(*name),
            "`{name}` is recorded now — move it into EXPECTED and delete its \
             KNOWN_MISSING entry. The entry said: {why}"
        );
    }
}

/// The two that were missing until 2026-08-24, called out on their own so the
/// reason survives even if the list above is reorganised.
#[test]
fn a_const_inside_a_function_is_recorded_like_the_var_beside_it() {
    let (check, file) = check_src(SRC);
    let got = defined_names(&check, &file);
    assert!(got.contains("bodyVar"), "the var arm was always recorded");
    assert!(
        got.contains("bodyConst"),
        "`decl_stmt`'s const arm skipped `record_def` while its var arm did not, \
         so a package-level `const` resolved and a local one did not"
    );
}

#[test]
fn a_type_parameter_is_recorded() {
    let (check, file) = check_src(SRC);
    assert!(
        defined_names(&check, &file).contains("TypeParam"),
        "go/types records this inside `declare(scope, id, obj, pos)`; guff's \
         `declare` takes no ident, so `declareTypeParam` has to do it"
    );
}

// ---------------------------------------------------------------------------
// Info.Implicits
// ---------------------------------------------------------------------------
//
// The same question one map over: go/types calls `recordImplicit` from four
// places — an unnamed import spec, an unnamed receiver, an unnamed parameter,
// and a type-switch case clause. All four are covered today, and this is here
// so the next one added upstream has somewhere to fail.

const IMPLICITS_SRC: &str = r#"
package p

import "unsafe"

type T struct{}

// Unnamed receiver: the Field gets the recv Var.
func (T) UnnamedRecv() {}

// Unnamed parameter: the Field gets the param Var.
func unnamedParam(int, string) {}

// A type switch records a per-clause object only when it *binds* a variable:
// `switch v.(type)` declares nothing, so its clauses are not implicits. Checked
// against go/types rather than assumed — the first version of this test guessed
// seven for the non-binding form and would have called guff wrong.
func typeSwitch(v any) {
	switch v.(type) {
	case int:
	case string:
	default:
	}
}

func bindingTypeSwitch(v any) {
	switch tv := v.(type) {
	case int:
		_ = tv
	case string:
		_ = tv
	default:
		_ = tv
	}
}

var _ = unsafe.Sizeof(T{})
"#;

#[test]
fn every_implicit_form_is_recorded() {
    let (check, _file) = check_src(IMPLICITS_SRC);
    assert!(check.errors.is_empty(), "unexpected errors: {:?}", check.errors);

    // Ground truth from go/types on the same source: one unnamed import, one
    // unnamed receiver, two unnamed params, and three clauses of the *binding*
    // type switch (`default` included). The non-binding switch contributes
    // nothing.
    let n = check.info.implicits.len();
    assert_eq!(
        n, 7,
        "Info.Implicits has {n} entries; go/types records 7 for this file — an \
         unnamed import spec, an unnamed receiver, each unnamed parameter, and \
         each case clause of a type switch that binds a variable"
    );
}

