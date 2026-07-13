//! Tests for the post-parse node-id stamping pass (`stamp.rs`).

use std::sync::Arc;

use guff::ast::{Expr, Ident};
use guff::parser::{parse_file, Mode};

/// Package documentation comments are available even without ParseComments.
#[test]
fn package_doc_without_parse_comments() {
    let fset = FileSet::new();
    let src = b"// Deprecated: use New instead.\npackage old\n";
    let file = parse_file(&fset, "old.go", src, Mode::NONE).expect("parse");
    assert!(file.doc.is_some(), "doc={:?}", file.doc);
}

use guff::parser_interface::parse_expr;
use guff::FileSet;

/// Every expression node produced by the parser is stamped with a nonzero id,
/// and sibling/child nodes get distinct ids.
#[test]
fn parsed_expr_nodes_are_stamped_and_distinct() {
    let e = parse_expr("a + b * c").unwrap();
    assert_ne!(e.id(), 0, "top binary expr must be stamped");

    let Expr::BinaryExpr(top) = &e else {
        panic!("expected a binary expr, got {e:?}");
    };
    // `a` and the `b * c` sub-tree.
    assert_ne!(top.x.id(), 0);
    assert_ne!(top.y.id(), 0);

    let Expr::BinaryExpr(mul) = &*top.y else {
        panic!("expected `b * c` to be a binary expr");
    };
    assert_ne!(mul.x.id(), 0);
    assert_ne!(mul.y.id(), 0);

    // All five expression nodes have distinct ids.
    let ids = [e.id(), top.x.id(), top.y.id(), mul.x.id(), mul.y.id()];
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "ids {i} and {j} collide: {ids:?}");
        }
    }
}

/// Cloning an expression preserves its id, so a clone made by the type checker
/// still denotes the same source node.
#[test]
fn clone_inherits_id() {
    let e = parse_expr("foo(x, y)").unwrap();
    let cloned = e.clone();
    assert_ne!(e.id(), 0);
    assert_eq!(e.id(), cloned.id());

    // Children too.
    if let (Expr::CallExpr(a), Expr::CallExpr(b)) = (&e, &cloned) {
        assert_eq!(a.fun.id(), b.fun.id());
        assert_eq!(a.args[0].id(), b.args[0].id());
    } else {
        panic!("expected call exprs");
    }
}

/// Identifiers and exprs built by hand (never parsed) keep id 0 and are thus
/// never recorded by the type checker's `Info` maps.
#[test]
fn synthetic_nodes_are_unstamped() {
    let id = Ident::new_ident("x");
    assert_eq!(id.id(), 0);
    assert_eq!(Expr::Ident(id).id(), 0);
}

/// Expressions nested inside declarations of a parsed file are stamped, and the
/// type expression `int` of a `var` declaration receives an id too.
#[test]
fn file_decls_are_stamped() {
    let fset = Arc::new(FileSet::new());
    let src = b"package p\nvar x int = 1 + 2\n";
    let file = guff::parser::parse_file(&fset, "t.go", src, Mode::NONE).unwrap();

    // Find `var x int = 1 + 2`.
    let mut found = false;
    for d in &file.decls {
        if let guff::ast::Decl::GenDecl(g) = d {
            for sp in &g.specs {
                if let guff::ast::Spec::ValueSpec(vs) = sp {
                    found = true;
                    // The type expression `int`.
                    assert_ne!(vs.ty.as_ref().unwrap().id(), 0, "type expr stamped");
                    // The initializer `1 + 2` and its operands.
                    let init = &vs.values[0];
                    assert_ne!(init.id(), 0, "initializer stamped");
                    if let Expr::BinaryExpr(b) = init {
                        assert_ne!(b.x.id(), 0);
                        assert_ne!(b.y.id(), 0);
                        assert_ne!(b.x.id(), b.y.id());
                    } else {
                        panic!("expected `1 + 2` binary expr");
                    }
                }
            }
        }
    }
    assert!(found, "did not find the value spec");
}

/// Scope-bearing nodes (the `File`, function `BlockStmt`, and the nested
/// `IfStmt`/`ForStmt`/`RangeStmt`/`SwitchStmt` and their blocks) are stamped
/// with nonzero, distinct ids so `types::Info::scopes` can key on them.
#[test]
fn scope_bearing_nodes_are_stamped() {
    use guff::ast::{Decl, Stmt};
    let fset = Arc::new(FileSet::new());
    let src = b"package p\nfunc f(xs []int) {\n\tif true {\n\t}\n\tfor i := 0; i < 1; i++ {\n\t}\n\tfor range xs {\n\t}\n\tswitch {\n\tcase true:\n\t}\n}\n";
    let file = guff::parser::parse_file(&fset, "t.go", src, Mode::NONE).unwrap();

    let mut ids: Vec<u32> = vec![file.id];
    assert_ne!(file.id, 0, "file stamped");

    let Decl::FuncDecl(fd) = &file.decls[0] else {
        panic!("expected func decl");
    };
    let body = fd.body.as_ref().unwrap();
    assert_ne!(body.id, 0, "func body block stamped");
    ids.push(body.id);

    for st in &body.list {
        match st {
            Stmt::IfStmt(s) => {
                assert_ne!(s.id, 0, "if stamped");
                assert_ne!(s.body.id, 0, "if body block stamped");
                ids.push(s.id);
                ids.push(s.body.id);
            }
            Stmt::ForStmt(s) => {
                assert_ne!(s.id, 0, "for stamped");
                assert_ne!(s.body.id, 0, "for body block stamped");
                ids.push(s.id);
                ids.push(s.body.id);
            }
            Stmt::RangeStmt(s) => {
                assert_ne!(s.id, 0, "range stamped");
                ids.push(s.id);
            }
            Stmt::SwitchStmt(s) => {
                assert_ne!(s.id, 0, "switch stamped");
                for c in &s.body.list {
                    if let Stmt::CaseClause(cc) = c {
                        assert_ne!(cc.id, 0, "case clause stamped");
                        ids.push(cc.id);
                    }
                }
                ids.push(s.id);
            }
            _ => {}
        }
    }

    // Every collected scope id is distinct.
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "scope ids {i}/{j} collide: {ids:?}");
        }
    }
    // We should have seen all four statement kinds plus their blocks + file/body.
    assert!(ids.len() >= 8, "expected many scope nodes, got {ids:?}");
}
