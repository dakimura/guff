//! Tests for `Info.Scopes` recording (port of the `recordScope` parts of
//! `recording.go`, chunk 72b).
//!
//! `Info.scopes` is keyed on the node id stamped onto scope-bearing statement /
//! file nodes (`File`, `BlockStmt`, `IfStmt`, `SwitchStmt`, `TypeSwitchStmt`,
//! `CaseClause`, `CommClause`, `ForStmt`, `RangeStmt`). These tests parse a
//! package, run the checker, then look each node up by its id and assert on the
//! recorded scope's comment / parentage.

use guff::ast::{Decl, File, Stmt};
use guff::parser::{parse_file, Mode};
use guff::position::FileSet;

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

/// The comment string of the scope recorded for `node_id`, or `None` if the
/// node was not recorded in `Info.scopes`.
fn scope_comment(check: &Checker, node_id: u32) -> Option<String> {
    check
        .info
        .scopes
        .get(&node_id)
        .map(|&sid| check.scopes.get(sid).comment().to_string())
}

/// The single top-level function body of `file`.
fn func_body(file: &File) -> &guff::ast::BlockStmt {
    for d in &file.decls {
        if let Decl::FuncDecl(fd) = d {
            return fd.body.as_ref().expect("function has a body");
        }
    }
    panic!("no function declaration found");
}

#[test]
fn file_scope_is_recorded() {
    let (check, file) = check_src("package p\nfunc f() {}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let sid = *check
        .info
        .scopes
        .get(&file.id)
        .expect("file node recorded in Info.scopes");
    // The file scope has an empty comment and its parent is the package scope
    // (which itself has the universe as parent).
    assert_eq!(check.scopes.get(sid).comment(), "");
    let parent = check
        .scopes
        .get(sid)
        .parent()
        .expect("file scope has a parent");
    // The file scope's parent is the package scope (comment "package \"<path>\"";
    // the default Config uses an empty import path).
    assert!(
        check.scopes.get(parent).comment().starts_with("package "),
        "file scope parent should be the package scope, got {:?}",
        check.scopes.get(parent).comment()
    );
}

#[test]
fn function_scope_is_recorded_on_functype() {
    let (check, file) = check_src("package p\nfunc f(a int) (b int) {\n\tb = a\n\treturn\n}\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let Decl::FuncDecl(fd) = &file.decls[0] else {
        panic!("expected func decl");
    };
    // The function scope is keyed on the FuncType, not the body block.
    let fscope = *check
        .info
        .scopes
        .get(&fd.ty.id)
        .expect("FuncType recorded in Info.scopes");
    assert_eq!(check.scopes.get(fscope).comment(), "function");
    // The body block is NOT recorded (Go omits the body BlockStmt entry).
    let body = fd.body.as_ref().unwrap();
    assert_eq!(scope_comment(&check, body.id), None);
    // The function scope holds the parameter `a` and named result `b`.
    assert!(check.scopes.get(fscope).lookup_local("a").is_some());
    assert!(check.scopes.get(fscope).lookup_local("b").is_some());
}

#[test]
fn func_literal_scope_is_recorded() {
    let (check, file) = check_src("package p\nvar _ = func(x int) int { return x }\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    // Find the FuncLit in the var initializer and check its FuncType scope.
    let Decl::GenDecl(gd) = &file.decls[0] else {
        panic!("expected gen decl");
    };
    let guff::ast::Spec::ValueSpec(vs) = &gd.specs[0] else {
        panic!("expected value spec");
    };
    let guff::ast::Expr::FuncLit(fl) = &vs.values[0] else {
        panic!("expected func lit");
    };
    let fscope = *check
        .info
        .scopes
        .get(&fl.ty.id)
        .expect("FuncLit's FuncType recorded");
    assert_eq!(check.scopes.get(fscope).comment(), "function");
    assert!(check.scopes.get(fscope).lookup_local("x").is_some());
}

#[test]
fn generic_type_param_scope_is_recorded_on_typespec() {
    let (check, file) = check_src("package p\ntype Vec[T any] []T\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let Decl::GenDecl(gd) = &file.decls[0] else {
        panic!("expected gen decl");
    };
    let guff::ast::Spec::TypeSpec(ts) = &gd.specs[0] else {
        panic!("expected type spec");
    };
    let sid = *check
        .info
        .scopes
        .get(&ts.id)
        .expect("generic TypeSpec recorded in Info.scopes");
    assert_eq!(check.scopes.get(sid).comment(), "type parameters");
    // The type parameter `T` lives in this scope.
    assert!(check.scopes.get(sid).lookup_local("T").is_some());
}

#[test]
fn non_generic_typespec_has_no_scope() {
    let (check, file) = check_src("package p\ntype T int\n");
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );
    let Decl::GenDecl(gd) = &file.decls[0] else {
        panic!("expected gen decl");
    };
    let guff::ast::Spec::TypeSpec(ts) = &gd.specs[0] else {
        panic!("expected type spec");
    };
    assert!(!check.info.scopes.contains_key(&ts.id));
}

#[test]
fn control_flow_scopes_are_recorded() {
    let src = "package p\n\
               func f(xs []int) {\n\
               \tif true {\n\
               \t}\n\
               \tfor i := 0; i < 1; i++ {\n\
               \t}\n\
               \tfor range xs {\n\
               \t}\n\
               \tswitch {\n\
               \tcase true:\n\
               \t}\n\
               }\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let body = func_body(&file);
    // The function body block itself is NOT recorded (Go keys the function
    // scope on the FuncType, which is DEFERRED here).
    assert_eq!(scope_comment(&check, body.id), None);

    let mut saw_if = false;
    let mut saw_for = false;
    let mut saw_range = false;
    let mut saw_switch = false;
    let mut saw_case = false;

    for st in &body.list {
        match st {
            Stmt::IfStmt(s) => {
                saw_if = true;
                assert_eq!(scope_comment(&check, s.id).as_deref(), Some("if"));
                // The if body block gets its own "block" scope.
                assert_eq!(scope_comment(&check, s.body.id).as_deref(), Some("block"));
            }
            Stmt::ForStmt(s) => {
                saw_for = true;
                assert_eq!(scope_comment(&check, s.id).as_deref(), Some("for"));
                assert_eq!(scope_comment(&check, s.body.id).as_deref(), Some("block"));
            }
            Stmt::RangeStmt(s) => {
                saw_range = true;
                assert_eq!(scope_comment(&check, s.id).as_deref(), Some("range"));
            }
            Stmt::SwitchStmt(s) => {
                saw_switch = true;
                assert_eq!(scope_comment(&check, s.id).as_deref(), Some("switch"));
                // The switch body block is NOT recorded (cases live in the
                // switch scope); each case clause gets a "case" scope.
                assert_eq!(scope_comment(&check, s.body.id), None);
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        saw_case = true;
                        assert_eq!(scope_comment(&check, cc.id).as_deref(), Some("case"));
                    }
                }
            }
            _ => {}
        }
    }
    assert!(saw_if && saw_for && saw_range && saw_switch && saw_case);
}

#[test]
fn nested_block_scope_is_recorded_with_control_parent() {
    // A bare nested block inside the if body: the inner block's scope parent is
    // the if body's block scope, whose parent is the "if" scope.
    let src = "package p\nfunc f() {\n\tif true {\n\t\t{\n\t\t}\n\t}\n}\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let body = func_body(&file);
    let Stmt::IfStmt(ifs) = &body.list[0] else {
        panic!("expected if stmt");
    };
    // if body block.
    let if_body_sid = *check
        .info
        .scopes
        .get(&ifs.body.id)
        .expect("if body block recorded");
    assert_eq!(check.scopes.get(if_body_sid).comment(), "block");

    // The inner bare block.
    let Stmt::BlockStmt(inner) = &ifs.body.list[0] else {
        panic!("expected nested block stmt");
    };
    let inner_sid = *check
        .info
        .scopes
        .get(&inner.id)
        .expect("inner block recorded");
    assert_eq!(check.scopes.get(inner_sid).comment(), "block");
    // Parent chain: inner block -> if body block -> "if" scope.
    let inner_parent = check.scopes.get(inner_sid).parent().unwrap();
    assert_eq!(inner_parent, if_body_sid);
    let if_scope = check.scopes.get(if_body_sid).parent().unwrap();
    assert_eq!(check.scopes.get(if_scope).comment(), "if");
}

#[test]
fn type_switch_and_select_scopes_are_recorded() {
    let src = "package p\nfunc f(x interface{}, ch chan int) {\n\
               \tswitch v := x.(type) {\n\
               \tcase int:\n\
               \t\t_ = v\n\
               \t}\n\
               \tselect {\n\
               \tcase ch <- 1:\n\
               \t}\n\
               }\n";
    let (check, file) = check_src(src);
    assert!(
        check.errors.is_empty(),
        "unexpected errors: {:?}",
        check.errors
    );

    let body = func_body(&file);
    let mut saw_type_switch = false;
    let mut saw_ts_case = false;
    let mut saw_comm = false;
    for st in &body.list {
        match st {
            Stmt::TypeSwitchStmt(s) => {
                saw_type_switch = true;
                assert_eq!(scope_comment(&check, s.id).as_deref(), Some("type switch"));
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        saw_ts_case = true;
                        assert_eq!(scope_comment(&check, cc.id).as_deref(), Some("case"));
                    }
                }
            }
            Stmt::SelectStmt(s) => {
                for c in &s.body.list {
                    if let Stmt::CommClause(cc) = c {
                        saw_comm = true;
                        assert_eq!(scope_comment(&check, cc.id).as_deref(), Some("case"));
                    }
                }
            }
            _ => {}
        }
    }
    assert!(
        saw_type_switch && saw_ts_case && saw_comm,
        "ts={saw_type_switch} ts_case={saw_ts_case} comm={saw_comm}"
    );
}
