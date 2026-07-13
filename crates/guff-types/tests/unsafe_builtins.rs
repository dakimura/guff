//! End-to-end tests for `import "unsafe"` + the `unsafe.Sizeof`/`Alignof`/
//! `Offsetof` built-ins (chunk 46). Exercises the whole pipeline: the resolver
//! binds a `PkgName` for `unsafe`, the selector's qualified-identifier fast
//! path resolves `unsafe.X`, and `builtins.rs` computes sizes via the default
//! (gc/amd64) `Sizes`.

use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

use guff_constant::int64_val;
use guff_types::arena::ObjectData;
use guff_types::scope::lookup as scope_lookup;
use guff_types::{Checker, Config, TypeKind};

fn parse(src: &str) -> guff::ast::File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> Checker {
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse(src)]);
    check
}

/// The compile-time integer value of package-level constant `name`.
fn const_int(check: &Checker, name: &str) -> i64 {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let obj =
        scope_lookup(&check.scopes, pkg_scope, name).unwrap_or_else(|| panic!("no const {name}"));
    match check.objects.get(obj) {
        ObjectData::Const(c) => {
            let (v, exact) = int64_val(c.val());
            assert!(exact, "const {name} value not an exact int64");
            v
        }
        _ => panic!("{name} is not a constant"),
    }
}

#[test]
fn import_unsafe_binds_pkgname() {
    // Importing and using `unsafe` (here via `unsafe.Pointer`) must bind a
    // PkgName in file scope and produce no errors. (An *unused* `import
    // "unsafe"` is a separate case, covered in tests/unused_imports.rs.)
    let check = check_src("package p\nimport \"unsafe\"\nvar x unsafe.Pointer\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
}

#[test]
fn sizeof_basic_types() {
    // gc/amd64 defaults: int/int64/pointer == 8, int32 == 4, bool == 1.
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var i int\n\
         var b bool\n\
         var w int32\n\
         const si = unsafe.Sizeof(i)\n\
         const sb = unsafe.Sizeof(b)\n\
         const sw = unsafe.Sizeof(w)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(const_int(&check, "si"), 8);
    assert_eq!(const_int(&check, "sb"), 1);
    assert_eq!(const_int(&check, "sw"), 4);
}

#[test]
fn sizeof_result_is_uintptr_constant() {
    let check = check_src("package p\nimport \"unsafe\"\nvar i int\nconst n = unsafe.Sizeof(i)\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    let n = scope_lookup(&check.scopes, pkg_scope, "n").unwrap();
    // Typed as uintptr (a Basic).
    let t = n.typ(&check.objects).unwrap();
    assert_eq!(t.kind(&check.types), TypeKind::Basic);
}

#[test]
fn sizeof_array() {
    // [4]int32 is 16 bytes on gc/amd64.
    let check =
        check_src("package p\nimport \"unsafe\"\nvar a [4]int32\nconst n = unsafe.Sizeof(a)\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(const_int(&check, "n"), 16);
}

#[test]
fn alignof_basic_types() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var i int\n\
         var w int32\n\
         const ai = unsafe.Alignof(i)\n\
         const aw = unsafe.Alignof(w)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(const_int(&check, "ai"), 8);
    assert_eq!(const_int(&check, "aw"), 4);
}

#[test]
fn offsetof_struct_fields() {
    // struct{ a int32; b int64 }: a at 0, b padded to offset 8.
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         type T struct { a int32; b int64 }\n\
         var t T\n\
         const oa = unsafe.Offsetof(t.a)\n\
         const ob = unsafe.Offsetof(t.b)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(const_int(&check, "oa"), 0);
    assert_eq!(const_int(&check, "ob"), 8);
}

#[test]
fn unsafe_pointer_as_type() {
    // unsafe.Pointer resolves as a type expression.
    let check = check_src("package p\nimport \"unsafe\"\nvar p unsafe.Pointer\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let pkg_scope = check.packages.get(check.pkg).scope();
    let p = scope_lookup(&check.scopes, pkg_scope, "p").unwrap();
    let t = p.typ(&check.objects).unwrap();
    assert_eq!(t.kind(&check.types), TypeKind::Basic);
    // Must resolve to the *valid* unsafe.Pointer basic, not the Invalid
    // placeholder (which is also a Basic — guards against a silent regression
    // in qualified type-name resolution).
    assert_eq!(check.type_str(t), "unsafe.Pointer");
}

#[test]
fn import_alias() {
    let check = check_src("package p\nimport u \"unsafe\"\nvar i int\nconst n = u.Sizeof(i)\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(const_int(&check, "n"), 8);
}

#[test]
fn bare_package_use_is_error() {
    // Using a package name outside a selector is an error.
    let check = check_src("package p\nimport \"unsafe\"\nvar x = unsafe\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("not in selector")),
        "expected 'not in selector' error, got: {:?}",
        check.errors
    );
}

#[test]
fn undefined_imported_name_is_error() {
    let check = check_src("package p\nimport \"unsafe\"\nvar i int\nconst n = unsafe.Nope(i)\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("undefined: unsafe.Nope")),
        "expected undefined imported name error, got: {:?}",
        check.errors
    );
}

#[test]
fn offsetof_non_selector_is_error() {
    let check =
        check_src("package p\nimport \"unsafe\"\nvar i int\nconst n = unsafe.Offsetof(i)\n");
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("not a selector expression")),
        "expected BadOffsetofSyntax error, got: {:?}",
        check.errors
    );
}

// ---- unsafe.Add / Slice / SliceData / String / StringData (chunk 47) -------

/// Render the type of package-level variable `name` via `Checker::type_str`.
fn var_type_str(check: &Checker, name: &str) -> String {
    let pkg_scope = check.packages.get(check.pkg).scope();
    let obj =
        scope_lookup(&check.scopes, pkg_scope, name).unwrap_or_else(|| panic!("no var {name}"));
    let t = obj
        .typ(&check.objects)
        .unwrap_or_else(|| panic!("var {name} has no type"));
    check.type_str(t)
}

#[test]
fn add_returns_unsafe_pointer() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var p unsafe.Pointer\n\
         var q = unsafe.Add(p, 3)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(var_type_str(&check, "q"), "unsafe.Pointer");
}

#[test]
fn add_non_integer_length_is_error() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var p unsafe.Pointer\n\
         var q = unsafe.Add(p, \"x\")\n",
    );
    // A non-integer length is rejected: `isValidIndex` converts the untyped
    // string to int first, which fails ("cannot convert untyped string ...").
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("convert untyped string") || e.msg.contains("must be integer")),
        "expected non-integer length error, got: {:?}",
        check.errors
    );
}

#[test]
fn slice_of_pointer_returns_slice() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var p *int\n\
         var s = unsafe.Slice(p, 3)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(var_type_str(&check, "s"), "[]int");
}

#[test]
fn slice_of_non_pointer_is_error() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var n int\n\
         var s = unsafe.Slice(n, 3)\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("is not a pointer")),
        "expected not-a-pointer error, got: {:?}",
        check.errors
    );
}

#[test]
fn slice_data_returns_pointer() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var s []int\n\
         var p = unsafe.SliceData(s)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(var_type_str(&check, "p"), "*int");
}

#[test]
fn slice_data_of_non_slice_is_error() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var n int\n\
         var p = unsafe.SliceData(n)\n",
    );
    assert!(
        check
            .errors
            .iter()
            .any(|e| e.msg.contains("is not a slice")),
        "expected not-a-slice error, got: {:?}",
        check.errors
    );
}

#[test]
fn string_returns_string() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var b *byte\n\
         var s = unsafe.String(b, 3)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(var_type_str(&check, "s"), "string");
}

#[test]
fn string_data_returns_byte_pointer() {
    let check = check_src(
        "package p\n\
         import \"unsafe\"\n\
         var s string\n\
         var p = unsafe.StringData(s)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    // byte is an alias for uint8; the renderer prints the underlying name.
    let t = var_type_str(&check, "p");
    assert!(t == "*byte" || t == "*uint8", "got {t}");
}
