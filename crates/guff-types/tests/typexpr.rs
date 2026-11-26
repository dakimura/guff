//! Chunk-21a tests: `Checker::typ` for identifiers, `*T`, `[]T`, parens.

use guff::ast::{
    ArrayType, BasicLit, ChanDir, ChanType, Expr, Ident, MapType, ParenExpr, StarExpr,
};
use guff::token::Token;
use guff::Pos;
use guff_types::arena::TypeData;
use guff_types::{
    array_elem, array_len, chan_dir, chan_elem, map_elem, map_key, new_named, new_type_name,
    scope_insert, slice_elem, BasicKind, Checker, Config,
};

fn ident(name: &str) -> Expr {
    Expr::Ident(Ident::new_ident(name))
}

fn int_lit(n: &str) -> Expr {
    Expr::BasicLit(BasicLit {
        id: 0,
        value_pos: Pos::default(),
        value_end: Pos::default(),
        kind: Some(Token::INT),
        value: n.to_string(),
    })
}

#[test]
fn resolves_predeclared_int() {
    let mut c = Checker::new(Config::default());
    let t = c.typ(&ident("int"));
    assert_eq!(t, c.basic(BasicKind::Int));
}

#[test]
fn pointer_to_int() {
    let mut c = Checker::new(Config::default());
    let e = Expr::StarExpr(StarExpr {
        id: 0,
        star: Pos::default(),
        x: Box::new(ident("int")),
    });
    let t = c.typ(&e);
    match c.types.get(t) {
        TypeData::Pointer(_) => {
            let elem = guff_types::pointer_elem(&c.types, t);
            assert_eq!(elem, c.basic(BasicKind::Int));
        }
        other => panic!("expected pointer, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn slice_of_int() {
    let mut c = Checker::new(Config::default());
    let e = Expr::ArrayType(ArrayType {
        id: 0,
        lbrack: Pos::default(),
        len: None,
        elt: Box::new(ident("int")),
    });
    let t = c.typ(&e);
    match c.types.get(t) {
        TypeData::Slice(_) => {
            assert_eq!(slice_elem(&c.types, t), c.basic(BasicKind::Int));
        }
        other => panic!("expected slice, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn sized_array_of_int() {
    let mut c = Checker::new(Config::default());
    let e = Expr::ArrayType(ArrayType {
        id: 0,
        lbrack: Pos::default(),
        len: Some(Box::new(int_lit("3"))),
        elt: Box::new(ident("int")),
    });
    let t = c.typ(&e);
    match c.types.get(t) {
        TypeData::Array(_) => {
            assert_eq!(array_len(&c.types, t), 3);
            assert_eq!(array_elem(&c.types, t), c.basic(BasicKind::Int));
        }
        other => panic!("expected array, got {:?}", std::mem::discriminant(other)),
    }
    assert!(c.errors.is_empty());
}

#[test]
fn map_string_int() {
    let mut c = Checker::new(Config::default());
    let e = Expr::MapType(MapType {
        id: 0,
        map_: Pos::default(),
        key: Box::new(ident("string")),
        value: Box::new(ident("int")),
    });
    let t = c.typ(&e);
    match c.types.get(t) {
        TypeData::Map(_) => {
            assert_eq!(map_key(&c.types, t), c.basic(BasicKind::String));
            assert_eq!(map_elem(&c.types, t), c.basic(BasicKind::Int));
        }
        other => panic!("expected map, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn channel_directions() {
    let mut c = Checker::new(Config::default());
    // chan int (bidirectional)
    let bidi = Expr::ChanType(ChanType {
        id: 0,
        begin: Pos::default(),
        arrow: Pos::default(),
        dir: ChanDir(ChanDir::SEND.0 | ChanDir::RECV.0),
        value: Box::new(ident("int")),
    });
    let t = c.typ(&bidi);
    assert!(matches!(c.types.get(t), TypeData::Chan(_)));
    assert_eq!(chan_dir(&c.types, t), guff_types::ChanDir::SendRecv);
    assert_eq!(chan_elem(&c.types, t), c.basic(BasicKind::Int));

    // chan<- int (send only)
    let send = Expr::ChanType(ChanType {
        id: 0,
        begin: Pos::default(),
        arrow: Pos::default(),
        dir: ChanDir::SEND,
        value: Box::new(ident("int")),
    });
    let ts = c.typ(&send);
    assert_eq!(chan_dir(&c.types, ts), guff_types::ChanDir::SendOnly);

    // <-chan int (recv only)
    let recv = Expr::ChanType(ChanType {
        id: 0,
        begin: Pos::default(),
        arrow: Pos::default(),
        dir: ChanDir::RECV,
        value: Box::new(ident("int")),
    });
    let tr = c.typ(&recv);
    assert_eq!(chan_dir(&c.types, tr), guff_types::ChanDir::RecvOnly);
}

#[test]
fn parenthesized_type() {
    let mut c = Checker::new(Config::default());
    let e = Expr::ParenExpr(ParenExpr {
        id: 0,
        lparen: Pos::default(),
        x: Box::new(ident("string")),
        rparen: Pos::default(),
    });
    assert_eq!(c.typ(&e), c.basic(BasicKind::String));
}

#[test]
fn undefined_name_errors() {
    let mut c = Checker::new(Config::default());
    let t = c.typ(&ident("Nonesuch"));
    assert_eq!(t, c.invalid_type());
    assert_eq!(c.errors.len(), 1);
    assert!(c.errors[0].msg.contains("undefined: Nonesuch"));
}

#[test]
fn resolves_user_named_type_from_package_scope() {
    let mut c = Checker::new(Config::default());
    // type T int, declared in the package scope.
    let int = c.basic(BasicKind::Int);
    let tn = new_type_name(&mut c.objects, "T", None);
    let named = new_named(&mut c.types, &mut c.objects, tn, Some(int), vec![]);

    let pkg_scope = c.packages.get(c.pkg).scope();
    scope_insert(&mut c.scopes, &mut c.objects, pkg_scope, tn);
    c.env.scope = Some(pkg_scope);

    let t = c.typ(&ident("T"));
    assert_eq!(t, named);
}

#[test]
fn value_object_used_as_type_errors() {
    let mut c = Checker::new(Config::default());
    // Insert a Var named "v" into the package scope; using it as a type fails.
    let int = c.basic(BasicKind::Int);
    let v = guff_types::new_var(&mut c.objects, "v", int);
    let pkg_scope = c.packages.get(c.pkg).scope();
    scope_insert(&mut c.scopes, &mut c.objects, pkg_scope, v);
    c.env.scope = Some(pkg_scope);

    let t = c.typ(&ident("v"));
    assert_eq!(t, c.invalid_type());
    assert!(c.errors.iter().any(|e| e.msg.contains("v is not a type")));
}
