//! Tests for `Info.Defs` / `Info.Uses` recording (port of the `recordDef` /
//! `recordUse` parts of `recording.go`, chunk 49).
//!
//! The `Info` maps are keyed on the parser-assigned `Ident::id()` (Go keys on
//! the `*syntax.Name` pointer; clones make pointers unusable here). These tests
//! parse a package, run the checker, then assert on the recorded entries —
//! mostly by the *names* of the recorded objects (robust to id values), plus
//! one test that resolves a specific identifier node to its id and looks it up.

use std::collections::HashSet;

use guff::ast::{File, Ident};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;
use guff::walk::{inspect, NodeRef};

use guff_types::operand::OperandMode;
use guff_types::selection::SelectionKind;
use guff_types::{Checker, Config};

fn parse(src: &str) -> File {
    let fset = FileSet::new();
    parse_file(&fset, "test.go", src.as_bytes(), Mode::NONE).expect("parse should succeed")
}

fn check_src(src: &str) -> (Checker, File) {
    let file = parse(src);
    let mut check = Checker::new(Config::default());
    check.check_files(vec![file.clone()]);
    (check, file)
}

/// Multiset of object names recorded in `Info.Defs` (values).
fn def_names(check: &Checker) -> Vec<String> {
    let mut v: Vec<String> = check
        .info
        .defs
        .values()
        .filter_map(|o| o.map(|id| id.name(&check.objects).to_string()))
        .collect();
    v.sort();
    v
}

/// Set of object names recorded in `Info.Uses` (values).
fn use_names(check: &Checker) -> HashSet<String> {
    check
        .info
        .uses
        .values()
        .map(|&id| id.name(&check.objects).to_string())
        .collect()
}

/// Collect every identifier in `file` as `(name, id)` pairs.
fn idents(file: &File) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    inspect(NodeRef::File(file), |n| {
        if let Some(NodeRef::Ident(id)) = n {
            out.push((id.name.clone(), id.id()));
        }
        true
    });
    out
}

#[test]
fn defs_records_package_level_decls() {
    let (check, _f) = check_src("package p\nconst c = 1\nvar v = 2\ntype T int\nfunc f() {}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    // Each package-level const/var/type/func name is defined exactly once.
    assert_eq!(def_names(&check), vec!["T", "c", "f", "v"]);
}

#[test]
fn uses_records_value_reference() {
    let (check, _f) = check_src("package p\nvar x = 1\nvar y = x\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    // The `x` in `var y = x` is a use of the variable x.
    assert!(
        use_names(&check).contains("x"),
        "uses: {:?}",
        use_names(&check)
    );
}

#[test]
fn uses_records_type_name_reference() {
    let (check, _f) = check_src("package p\ntype T int\nvar v T\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let uses = use_names(&check);
    // `var v T` uses T; `type T int` uses the predeclared `int`.
    assert!(uses.contains("T"), "uses: {:?}", uses);
    assert!(uses.contains("int"), "uses: {:?}", uses);
}

#[test]
fn uses_records_call_inside_func_body() {
    let (check, _f) =
        check_src("package p\nfunc f() int { return g() }\nfunc g() int { return 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        use_names(&check).contains("g"),
        "uses: {:?}",
        use_names(&check)
    );
}

#[test]
fn uses_records_field_selector() {
    let (check, _f) =
        check_src("package p\ntype T struct { a int }\nfunc f(t T) int { return t.a }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    // The `.a` selector records a use of the struct field `a`.
    assert!(
        use_names(&check).contains("a"),
        "uses: {:?}",
        use_names(&check)
    );
}

#[test]
fn uses_records_unsafe_qualified_ident() {
    let (check, _f) = check_src("package p\nimport \"unsafe\"\nvar x = unsafe.Sizeof(0)\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let uses = use_names(&check);
    // Both the `unsafe` package name and the `Sizeof` builtin are uses.
    assert!(uses.contains("unsafe"), "uses: {:?}", uses);
    assert!(uses.contains("Sizeof"), "uses: {:?}", uses);
}

#[test]
fn def_and_use_keyed_on_distinct_ident_ids() {
    // `var x = 1` defines x; `var y = x` uses it. The two `x` identifiers are
    // distinct nodes with distinct ids: the def id lands in Defs, the use id in
    // Uses, and they map to the *same* object.
    let (check, f) = check_src("package p\nvar x = 1\nvar y = x\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let x_ids: Vec<u32> = idents(&f)
        .into_iter()
        .filter(|(name, _)| name == "x")
        .map(|(_, id)| id)
        .collect();
    assert_eq!(x_ids.len(), 2, "expected two `x` identifiers");
    assert_ne!(x_ids[0], x_ids[1], "ident ids must be distinct");

    // Exactly one of the two `x` ids is a def, the other a use.
    let def_x: Vec<u32> = x_ids
        .iter()
        .copied()
        .filter(|id| check.info.defs.contains_key(id))
        .collect();
    let use_x: Vec<u32> = x_ids
        .iter()
        .copied()
        .filter(|id| check.info.uses.contains_key(id))
        .collect();
    assert_eq!(def_x.len(), 1, "exactly one def-x");
    assert_eq!(use_x.len(), 1, "exactly one use-x");

    // Both denote the same variable object.
    let def_obj = check.info.defs[&def_x[0]].expect("def has an object");
    let use_obj = check.info.uses[&use_x[0]];
    assert_eq!(
        def_obj, use_obj,
        "def and use should resolve to the same object"
    );
    assert_eq!(def_obj.name(&check.objects), "x");
}

#[test]
fn unstamped_idents_are_not_recorded() {
    // A hand-built Ident has id 0 and must never be recorded.
    let mut check = Checker::new(Config::default());
    check.check_files(vec![parse("package p\nvar x = 1\n")]);
    let synthetic = Ident::new_ident("x");
    assert_eq!(synthetic.id(), 0);
    assert!(!check.info.defs.contains_key(&0));
    assert!(!check.info.uses.contains_key(&0));
    assert!(!check.info.types.contains_key(&0));
}

// ----------------------------------------------------------------------------
// Def sites beyond package level (chunk 65): `:=` and range locals
// ----------------------------------------------------------------------------

#[test]
fn defs_records_short_var_decl_local() {
    let (check, _f) = check_src("package p\nfunc f() {\n\tx := 1\n\t_ = x\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        def_names(&check).contains(&"x".to_string()),
        "defs: {:?}",
        def_names(&check)
    );
}

#[test]
fn defs_records_range_local() {
    let (check, _f) =
        check_src("package p\nfunc f(s []int) {\n\tfor i := range s {\n\t\t_ = i\n\t}\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        def_names(&check).contains(&"i".to_string()),
        "defs: {:?}",
        def_names(&check)
    );
}

#[test]
fn short_var_redeclaration_records_use_not_def() {
    // `x := 1` defines x; `x, y := 2, 3` redeclares x (a use) and defines y.
    // x must appear exactly once in Defs, and x must appear in Uses.
    let src = "package p\nfunc f() {\n\tx := 1\n\tx, y := 2, 3\n\t_ = x\n\t_ = y\n}\n";
    let (check, _f) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let defs = def_names(&check);
    assert_eq!(
        defs.iter().filter(|n| *n == "x").count(),
        1,
        "defs: {:?}",
        defs
    );
    assert_eq!(
        defs.iter().filter(|n| *n == "y").count(),
        1,
        "defs: {:?}",
        defs
    );
    assert!(
        use_names(&check).contains("x"),
        "uses: {:?}",
        use_names(&check)
    );
}

// ----------------------------------------------------------------------------
// Function parameter / result / receiver Defs (chunk 66)
// ----------------------------------------------------------------------------

#[test]
fn defs_records_function_params() {
    let (check, _f) = check_src("package p\nfunc f(a int, b string) {}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let defs = def_names(&check);
    assert!(defs.contains(&"a".to_string()), "defs: {:?}", defs);
    assert!(defs.contains(&"b".to_string()), "defs: {:?}", defs);
}

#[test]
fn defs_records_named_results() {
    let (check, _f) = check_src("package p\nfunc f() (r int) { return }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        def_names(&check).contains(&"r".to_string()),
        "defs: {:?}",
        def_names(&check)
    );
}

#[test]
fn defs_records_method_receiver() {
    let (check, _f) = check_src("package p\ntype T int\nfunc (r T) m() {}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        def_names(&check).contains(&"r".to_string()),
        "defs: {:?}",
        def_names(&check)
    );
}

// ----------------------------------------------------------------------------
// Info.Types recording (chunk 50)
// ----------------------------------------------------------------------------

/// Id of the first `BinaryExpr` node in `file`.
fn first_binary_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::BinaryExpr(b)) = n {
                id = Some(b.id);
            }
        }
        true
    });
    id
}

/// Id of the first `CallExpr` node in `file`.
fn first_call_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::CallExpr(c)) = n {
                id = Some(c.id);
            }
        }
        true
    });
    id
}

/// Id of the first identifier named `name`.
fn first_ident_id(file: &File, name: &str) -> Option<u32> {
    idents(file)
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, id)| id)
}

#[test]
fn types_records_constant_literal() {
    let (check, f) = check_src("package p\nconst c = 42\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // The literal `42` is the rhs; it is recorded as a constant.
    let mut lit_id = None;
    inspect(NodeRef::File(&f), |n| {
        if let Some(NodeRef::BasicLit(b)) = n {
            lit_id = Some(b.id);
        }
        true
    });
    let id = lit_id.expect("found the basic literal");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("literal recorded in Types");
    assert_eq!(tv.mode, OperandMode::Constant);
    assert!(tv.val.is_some(), "constant literal must carry a value");
    // The const has no explicit type, so the literal is never materialised
    // into a typed context — `record_untyped` flushes it as untyped int.
    assert_eq!(check.type_str(tv.typ), "untyped int");
}

#[test]
fn types_records_constant_binary_expr() {
    let (check, f) = check_src("package p\nconst c = 1 + 2\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_binary_id(&f).expect("found `1 + 2`");
    let tv = check.info.types.get(&id).expect("binary expr recorded");
    assert_eq!(tv.mode, OperandMode::Constant);
    assert!(tv.val.is_some());
    // Untyped const, no context type: flushed by `record_untyped`.
    assert_eq!(check.type_str(tv.typ), "untyped int");
}

#[test]
fn types_records_variable_use() {
    let (check, f) = check_src("package p\nvar x int = 1\nvar y = x\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // The `x` in `var y = x` is recorded both as a use and (as an expression)
    // in Types, with addressing mode `Variable`.
    let mut use_x_id = None;
    for (&id, &obj) in &check.info.uses {
        if obj.name(&check.objects) == "x" {
            use_x_id = Some(id);
        }
    }
    let id = use_x_id.expect("a use of x");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("variable use recorded in Types");
    assert_eq!(tv.mode, OperandMode::Variable);
    assert_eq!(check.type_str(tv.typ), "int");
}

#[test]
fn types_records_type_expression() {
    let (check, f) = check_src("package p\nvar v int\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // The `int` in `var v int` is a type expression: recorded with mode
    // `TypeExpr`.
    let id = first_ident_id(&f, "int").expect("found the `int` type ident");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("type expr recorded in Types");
    assert_eq!(tv.mode, OperandMode::TypeExpr);
    assert_eq!(check.type_str(tv.typ), "int");
}

#[test]
fn types_records_call_result() {
    let (check, f) =
        check_src("package p\nfunc f() int { return g() }\nfunc g() int { return 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_call_id(&f).expect("found `g()`");
    let tv = check.info.types.get(&id).expect("call recorded in Types");
    assert_eq!(tv.mode, OperandMode::Value);
    assert_eq!(check.type_str(tv.typ), "int");
}

// ----------------------------------------------------------------------------
// Untyped delay / narrowing (chunk 51)
// ----------------------------------------------------------------------------

/// Id of the first `BasicLit` node in `file`.
fn first_basic_lit_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::BasicLit(b)) = n {
                id = Some(b.id);
            }
        }
        true
    });
    id
}

#[test]
fn var_init_narrows_untyped_literal_to_default() {
    // `var x = 1` has no explicit type, so the literal is materialised into its
    // default type (int) when assigned — not left untyped.
    let (check, f) = check_src("package p\nvar x = 1\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_basic_lit_id(&f).expect("found `1`");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("literal recorded in Types");
    assert_eq!(tv.mode, OperandMode::Constant);
    assert_eq!(check.type_str(tv.typ), "int");
}

#[test]
fn typed_var_init_narrows_binary_to_context_type() {
    // `var x int = 1 + 2`: the whole constant expression is narrowed to `int`
    // by the explicit context type (updateExprType via convert_untyped).
    let (check, f) = check_src("package p\nvar x int = 1 + 2\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_binary_id(&f).expect("found `1 + 2`");
    let tv = check.info.types.get(&id).expect("binary expr recorded");
    assert_eq!(tv.mode, OperandMode::Constant);
    assert_eq!(check.type_str(tv.typ), "int");
}

#[test]
fn constant_binary_operands_stay_untyped() {
    // The operands of a *constant* binary expression are never materialised
    // individually (updateExprType breaks on `old.val != nil`), so they remain
    // untyped and are flushed by record_untyped — even though `1 + 2` is `int`.
    let (check, f) = check_src("package p\nvar x int = 1 + 2\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_basic_lit_id(&f).expect("found the operand `1`");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("operand recorded in Types");
    assert_eq!(check.type_str(tv.typ), "untyped int");
}

#[test]
fn nonconstant_binary_operand_is_narrowed() {
    // In `a + 1` (a is a variable, so the sum is non-constant), the untyped
    // literal `1` is materialised to `a`'s type when the operands are matched.
    let (check, f) = check_src("package p\nfunc f(a int) int { return a + 1 }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let id = first_basic_lit_id(&f).expect("found `1`");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("operand recorded in Types");
    assert_eq!(check.type_str(tv.typ), "int");
}

#[test]
fn untyped_map_is_drained_after_check() {
    // record_untyped drains the map at the end of check_files; nothing should
    // be left dangling.
    let (check, _f) = check_src("package p\nvar x int = 1 + 2\nvar y = 3\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        check.untyped.is_empty(),
        "untyped map not drained: {:?}",
        check.untyped
    );
}

// ----------------------------------------------------------------------------
// Comma-ok promotion (chunk 52)
// ----------------------------------------------------------------------------

/// Id of the first `IndexExpr` node in `file`.
fn first_index_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::IndexExpr(x)) = n {
                id = Some(x.id);
            }
        }
        true
    });
    id
}

/// Id of the first `TypeAssertExpr` node in `file`.
fn first_type_assert_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::TypeAssertExpr(x)) = n {
                id = Some(x.id);
            }
        }
        true
    });
    id
}

/// Id of the first `UnaryExpr` node in `file` (here always the `<-ch` receive).
fn first_unary_id(file: &File) -> Option<u32> {
    let mut id = None;
    inspect(NodeRef::File(file), |n| {
        if id.is_none() {
            if let Some(NodeRef::UnaryExpr(x)) = n {
                id = Some(x.id);
            }
        }
        true
    });
    id
}

/// Assert that `id`'s recorded type is the 2-tuple `(int, bool)`.
fn assert_int_bool_tuple(check: &Checker, id: u32) {
    let tv = check
        .info
        .types
        .get(&id)
        .expect("comma-ok expr recorded in Types");
    let s = check.type_str(tv.typ);
    assert!(
        s.starts_with('(') && s.contains("int") && s.contains("bool"),
        "expected a 2-tuple (int, bool), got `{}`",
        s
    );
}

#[test]
fn comma_ok_map_index_records_tuple() {
    // `v, ok := m["x"]`: the map-index expression `m["x"]`, recorded singly as
    // `int` when first checked, is promoted to `(int, bool)`.
    let (check, f) = check_src(
        "package p\nfunc f() { m := make(map[string]int); v, ok := m[\"x\"]; _ = v; _ = ok }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let id = first_index_id(&f).expect("found `m[\"x\"]`");
    assert_int_bool_tuple(&check, id);
}

#[test]
fn comma_ok_type_assert_records_tuple() {
    // `v, ok := i.(int)`: the assertion `i.(int)` is promoted to `(int, bool)`.
    let (check, f) =
        check_src("package p\nfunc f(i interface{}) { v, ok := i.(int); _ = v; _ = ok }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let id = first_type_assert_id(&f).expect("found `i.(int)`");
    assert_int_bool_tuple(&check, id);
}

#[test]
fn comma_ok_channel_receive_records_tuple() {
    // `v, ok := <-ch`: the receive `<-ch` is promoted to `(int, bool)`.
    let (check, f) =
        check_src("package p\nfunc f() { ch := make(chan int); v, ok := <-ch; _ = v; _ = ok }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let id = first_unary_id(&f).expect("found `<-ch`");
    assert_int_bool_tuple(&check, id);
}

#[test]
fn comma_ok_assignment_records_tuple() {
    // The promotion also fires for plain assignment (`assignVars`), not just
    // `:=` declarations.
    let (check, f) = check_src(
        "package p\nfunc f() { m := make(map[string]int); var v int; var ok bool; v, ok = m[\"x\"]; _ = v; _ = ok }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let id = first_index_id(&f).expect("found `m[\"x\"]`");
    assert_int_bool_tuple(&check, id);
}

#[test]
fn single_value_map_index_not_promoted() {
    // A map index used in a *single*-value context keeps its value type and is
    // not promoted to a tuple.
    let (check, f) =
        check_src("package p\nfunc f() { m := make(map[string]int); v := m[\"x\"]; _ = v }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let id = first_index_id(&f).expect("found `m[\"x\"]`");
    let tv = check
        .info
        .types
        .get(&id)
        .expect("map index recorded in Types");
    assert_eq!(check.type_str(tv.typ), "int");
}

// ---------------------------------------------------------------------------
// Selections (recordSelection, chunk 53)
// ---------------------------------------------------------------------------

/// Collect every recorded selection as `(kind, obj-name, recv-type)`, sorted
/// for stable assertions.
fn selections(check: &Checker) -> Vec<(SelectionKind, String, String)> {
    let mut v: Vec<(SelectionKind, String, String)> = check
        .info
        .selections
        .values()
        .map(|s| {
            (
                s.kind(),
                s.obj().name(&check.objects).to_string(),
                check.type_str(s.recv()),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn selections_records_field_val() {
    let (check, _f) =
        check_src("package p\ntype T struct { a int }\nfunc f(t T) int { return t.a }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(
        selections(&check),
        vec![(SelectionKind::FieldVal, "a".to_string(), "T".to_string())]
    );
}

#[test]
fn selections_records_method_val() {
    // `t.M` where M is a method, used as a value, is a MethodVal selection on
    // the receiver type T.
    let (check, _f) = check_src(
        "package p\ntype T struct{}\nfunc (T) M() int { return 0 }\nfunc f(t T) int { return t.M() }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(
        selections(&check),
        vec![(SelectionKind::MethodVal, "M".to_string(), "T".to_string())]
    );
}

#[test]
fn selections_records_method_expr() {
    // `T.M` (a method expression) is a MethodExpr selection on T.
    let (check, _f) =
        check_src("package p\ntype T struct{}\nfunc (T) M() int { return 0 }\nvar g = T.M\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(
        selections(&check),
        vec![(SelectionKind::MethodExpr, "M".to_string(), "T".to_string())]
    );
}

#[test]
fn selections_excludes_qualified_idents() {
    // A qualified identifier `unsafe.Sizeof` is *not* a selection (Go records
    // it as a plain use, not in the Selections map).
    let (check, _f) = check_src("package p\nimport \"unsafe\"\nvar x = unsafe.Sizeof(0)\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert!(
        check.info.selections.is_empty(),
        "qualified ident should not be a selection: {:?}",
        selections(&check)
    );
    // ...but the use of `Sizeof` is still recorded.
    assert!(use_names(&check).contains("Sizeof"));
}

#[test]
fn selections_records_offsetof_field() {
    // unsafe.Offsetof's selector argument is recorded as a FieldVal selection
    // (builtins.go:804).
    let (check, _f) = check_src(
        "package p\nimport \"unsafe\"\ntype T struct { a, b int }\nvar o = unsafe.Offsetof(T{}.b)\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    assert_eq!(
        selections(&check),
        vec![(SelectionKind::FieldVal, "b".to_string(), "T".to_string())]
    );
}

// ---------------------------------------------------------------------------
// Instances (recordInstance, chunk 53)
// ---------------------------------------------------------------------------

/// Collect every recorded instance as `(type-args, instantiated-type)`,
/// rendered to strings and sorted.
fn instances(check: &Checker) -> Vec<(Vec<String>, String)> {
    let mut v: Vec<(Vec<String>, String)> = check
        .info
        .instances
        .values()
        .map(|inst| {
            (
                inst.type_args.iter().map(|&t| check.type_str(t)).collect(),
                check.type_str(inst.typ),
            )
        })
        .collect();
    v.sort();
    v
}

#[test]
fn instances_records_type_instantiation() {
    // `Vec[int]` records an instance keyed on the `Vec` identifier with type
    // argument `int`.
    let (check, _f) = check_src("package p\ntype Vec[T any] []T\nvar v Vec[int]\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let insts = instances(&check);
    assert_eq!(insts.len(), 1, "instances: {:?}", insts);
    assert_eq!(insts[0].0, vec!["int".to_string()]);
    // The instantiated type's name carries the type argument.
    assert!(insts[0].1.contains("Vec"), "typ: {}", insts[0].1);
}

#[test]
fn instances_records_two_type_args() {
    let (check, _f) =
        check_src("package p\ntype Pair[A, B any] struct { a A; b B }\nvar x Pair[int, string]\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let insts = instances(&check);
    assert_eq!(insts.len(), 1, "instances: {:?}", insts);
    assert_eq!(insts[0].0, vec!["int".to_string(), "string".to_string()]);
}

#[test]
fn instances_records_inferred_call() {
    // An inferred generic call `Id(a)` records an instance keyed on the callee
    // identifier `Id` with the inferred type argument.
    let (check, _f) = check_src(
        "package p\nfunc Id[T any](x T) T { return x }\nfunc f() { var a int; _ = Id(a) }\n",
    );
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let insts = instances(&check);
    assert_eq!(insts.len(), 1, "instances: {:?}", insts);
    assert_eq!(insts[0].0, vec!["int".to_string()]);
    // The instantiated signature is no longer generic; rendered as a func type.
    assert!(insts[0].1.starts_with("func"), "typ: {}", insts[0].1);
}
