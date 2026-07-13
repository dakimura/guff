//! Chunk-17 tests: `typestring.rs` — `type_string` / `signature_string`.

use guff_types::PackageArena;
use guff_types::{
    bind_tparams, init_universe_full, named_set_type_params, new_array, new_chan, new_field,
    new_func, new_interface_type, new_map, new_named, new_package, new_param, new_pointer,
    new_signature_type, new_slice, new_struct, new_term, new_tuple, new_type_name, new_type_param,
    new_union, set_constraint, signature_string, type_string, BasicKind, ChanDir, PackageId,
    TypeId, Universe,
};

fn b(u: &Universe, k: BasicKind) -> TypeId {
    u.typ[k as usize]
}

/// Render with the default (full import-path) qualifier.
fn ts(u: &Universe, t: TypeId) -> String {
    type_string(&u.type_arena, &u.object_arena, &u.package_arena, t, None)
}

// ---------------------------------------------------------------------------
// basics & simple composites

#[test]
fn basic_types() {
    let u = init_universe_full();
    assert_eq!(ts(&u, b(&u, BasicKind::Int)), "int");
    assert_eq!(ts(&u, b(&u, BasicKind::String)), "string");
    assert_eq!(ts(&u, b(&u, BasicKind::Bool)), "bool");
    // Exported basic lives in package unsafe.
    assert_eq!(ts(&u, b(&u, BasicKind::UnsafePointer)), "unsafe.Pointer");
}

#[test]
fn slice_array_pointer_map() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let si = new_slice(&mut u.type_arena, int);
    let arr = new_array(&mut u.type_arena, int, 3);
    let pi = new_pointer(&mut u.type_arena, int);
    let m = new_map(&mut u.type_arena, string, int);
    assert_eq!(ts(&u, si), "[]int");
    assert_eq!(ts(&u, arr), "[3]int");
    assert_eq!(ts(&u, pi), "*int");
    assert_eq!(ts(&u, m), "map[string]int");
    // nested
    let ssi = new_slice(&mut u.type_arena, si);
    assert_eq!(ts(&u, ssi), "[][]int");
}

// ---------------------------------------------------------------------------
// channels (incl. the chan (<-chan T) parenthesisation)

#[test]
fn channels() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let both = new_chan(&mut u.type_arena, ChanDir::SendRecv, int);
    let send = new_chan(&mut u.type_arena, ChanDir::SendOnly, int);
    let recv = new_chan(&mut u.type_arena, ChanDir::RecvOnly, int);
    assert_eq!(ts(&u, both), "chan int");
    assert_eq!(ts(&u, send), "chan<- int");
    assert_eq!(ts(&u, recv), "<-chan int");

    // chan (<-chan int) needs parentheses
    let chan_of_recv = new_chan(&mut u.type_arena, ChanDir::SendRecv, recv);
    assert_eq!(ts(&u, chan_of_recv), "chan (<-chan int)");
    // chan chan<- int does NOT need parens
    let chan_of_send = new_chan(&mut u.type_arena, ChanDir::SendRecv, send);
    assert_eq!(ts(&u, chan_of_send), "chan chan<- int");
}

// ---------------------------------------------------------------------------
// structs (named fields, embedded, tags)

#[test]
fn structs() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);

    let fx = new_field(&mut u.object_arena, "x", int, false);
    let fy = new_field(&mut u.object_arena, "y", string, false);
    let s = new_struct(&mut u.type_arena, vec![fx, fy], vec![]);
    assert_eq!(ts(&u, s), "struct{x int; y string}");

    // with a tag on the first field
    let fa = new_field(&mut u.object_arena, "A", int, false);
    let s2 = new_struct(&mut u.type_arena, vec![fa], vec![r#"json:"a""#.to_string()]);
    assert_eq!(ts(&u, s2), r#"struct{A int "json:\"a\""}"#);

    // empty struct
    let s3 = new_struct(&mut u.type_arena, vec![], vec![]);
    assert_eq!(ts(&u, s3), "struct{}");
}

#[test]
fn embedded_struct_field() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    // type MyInt named, embedded field prints just the type name.
    let tn = new_type_name(&mut u.object_arena, "MyInt", None);
    let myint = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );
    let emb = new_field(&mut u.object_arena, "MyInt", myint, true);
    let s = new_struct(&mut u.type_arena, vec![emb], vec![]);
    assert_eq!(ts(&u, s), "struct{MyInt}");
}

// ---------------------------------------------------------------------------
// signatures

#[test]
fn signatures() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let bool_ = b(&u, BasicKind::Bool);

    // func(int, string) bool
    let p1 = new_param(&mut u.object_arena, "", int);
    let p2 = new_param(&mut u.object_arena, "", string);
    let params = new_tuple(&mut u.type_arena, &[p1, p2]);
    let r = new_param(&mut u.object_arena, "", bool_);
    let results = new_tuple(&mut u.type_arena, &[r]);
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], params, results, false);
    assert_eq!(ts(&u, sig), "func(int, string) bool");
    // signature_string drops the leading "func"
    assert_eq!(
        signature_string(&u.type_arena, &u.object_arena, &u.package_arena, sig, None),
        "(int, string) bool"
    );

    // no results
    let sig2 = new_signature_type(&mut u.type_arena, None, &[], &[], params, None, false);
    assert_eq!(ts(&u, sig2), "func(int, string)");

    // multiple results -> parenthesised
    let ra = new_param(&mut u.object_arena, "", int);
    let rb = new_param(&mut u.object_arena, "", string);
    let results2 = new_tuple(&mut u.type_arena, &[ra, rb]);
    let sig3 = new_signature_type(&mut u.type_arena, None, &[], &[], None, results2, false);
    assert_eq!(ts(&u, sig3), "func() (int, string)");
}

#[test]
fn variadic_signature() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let si = new_slice(&mut u.type_arena, int); // last param type is []int
    let p = new_param(&mut u.object_arena, "", si);
    let params = new_tuple(&mut u.type_arena, &[p]);
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], params, None, true);
    assert_eq!(ts(&u, sig), "func(...int)");
}

#[test]
fn signature_with_param_names() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let p = new_param(&mut u.object_arena, "n", int);
    let params = new_tuple(&mut u.type_arena, &[p]);
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], params, None, false);
    assert_eq!(ts(&u, sig), "func(n int)");
}

// ---------------------------------------------------------------------------
// unions & interfaces

#[test]
fn unions_in_interface() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    // int | ~string
    let t1 = new_term(false, int);
    let t2 = new_term(true, string);
    let union = new_union(&mut u.type_arena, vec![t1, t2]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    assert_eq!(ts(&u, iface), "interface{int | ~string}");
}

#[test]
fn empty_and_method_interfaces() {
    let mut u = init_universe_full();
    let empty = new_interface_type(&mut u.type_arena, vec![], vec![]);
    assert_eq!(ts(&u, empty), "interface{}");

    // interface{ Foo() }
    let sig = new_signature_type(&mut u.type_arena, None, &[], &[], None, None, false);
    let foo = new_func(&mut u.object_arena, "Foo", Some(sig));
    let iface = new_interface_type(&mut u.type_arena, vec![foo], vec![]);
    assert_eq!(ts(&u, iface), "interface{Foo()}");
}

// ---------------------------------------------------------------------------
// named & parameterised types, qualifiers

#[test]
fn named_without_package() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let tn = new_type_name(&mut u.object_arena, "Foo", None);
    let foo = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );
    assert_eq!(ts(&u, foo), "Foo");
}

#[test]
fn named_with_package_and_qualifier() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let pkg: PackageId = new_package(
        &mut u.package_arena,
        &mut u.scope_arena,
        u.universe_scope,
        "encoding/json",
        "json",
    );
    let tn = new_type_name(&mut u.object_arena, "Encoder", None);
    tn.set_pkg(&mut u.object_arena, pkg);
    let enc = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );

    // default qualifier = full import path
    assert_eq!(ts(&u, enc), "encoding/json.Encoder");

    // custom qualifier returning "" prints the bare name
    let bare: guff_types::Qualifier = Some(&|_p: PackageId, _pa: &PackageArena| String::new());
    assert_eq!(
        type_string(&u.type_arena, &u.object_arena, &u.package_arena, enc, bare),
        "Encoder"
    );
}

#[test]
fn parameterised_named_type() {
    // type List[T any] ... — printed as "List[T interface{}]".
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let empty_iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn_t, Some(empty_iface));

    let tn = new_type_name(&mut u.object_arena, "List", None);
    let list = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );
    let tpl = bind_tparams(&mut u.type_arena, vec![tp]).unwrap();
    named_set_type_params(&mut u.type_arena, list, tpl);

    assert_eq!(ts(&u, list), "List[T interface{}]");
}

// ---------------------------------------------------------------------------
// type parameter constraint rendering inside tparam lists

#[test]
fn type_param_with_named_constraint() {
    // [T C] where C is a named interface.
    let mut u = init_universe_full();
    let empty_iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    let c_name = new_type_name(&mut u.object_arena, "C", None);
    let c = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        c_name,
        Some(empty_iface),
        vec![],
    );

    let tn_t = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn_t, Some(c));
    set_constraint(&mut u.type_arena, tp, c);

    let tn = new_type_name(&mut u.object_arena, "G", None);
    let int = b(&u, BasicKind::Int);
    let g = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tn,
        Some(int),
        vec![],
    );
    let tpl = bind_tparams(&mut u.type_arena, vec![tp]).unwrap();
    named_set_type_params(&mut u.type_arena, g, tpl);

    assert_eq!(ts(&u, g), "G[T C]");
}
