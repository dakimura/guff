//! Chunk-16 tests: `assignments.rs` — `assignable_to` (structural core of
//! Go's `operand.assignableTo`).
//!
//! The Checker-dependent decisions are injected: `IMPLEMENTS_NONE` /
//! `IMPLEMENTS_ALL` for interface satisfaction, and `REPR_NONE` /
//! `REPR_ALL` for untyped-value representability. Tests pick whichever stub
//! isolates the structural rule under test.

use guff_types::{
    assignable_to, init_universe_full, new_chan, new_interface_type, new_named, new_pointer,
    new_slice, new_term, new_type_name, new_type_param, new_union, BasicKind, ChanDir, ObjectArena,
    Operand, PackageArena, TypeArena, TypeId, Universe,
};
use guff_types_errors::Code;

type ImplementsFn = dyn Fn(&mut TypeArena, &ObjectArena, &PackageArena, TypeId, TypeId) -> bool;
type RepresentableFn = dyn Fn(&TypeArena, &Operand, TypeId) -> bool;

const IMPLEMENTS_NONE: &ImplementsFn = &|_, _, _, _, _| false;
const IMPLEMENTS_ALL: &ImplementsFn = &|_, _, _, _, _| true;
const REPR_NONE: &RepresentableFn = &|_, _, _| false;
const REPR_ALL: &RepresentableFn = &|_, _, _| true;

fn b(u: &Universe, k: BasicKind) -> TypeId {
    u.typ[k as usize]
}

/// Build a typed `value`-mode operand of type `typ`.
fn val(typ: TypeId) -> Operand {
    let mut x = Operand::invalid();
    x.mode = guff_types::OperandMode::Value;
    x.typ = Some(typ);
    x
}

/// Build an untyped constant-mode operand of type `typ`.
fn untyped(typ: TypeId) -> Operand {
    let mut x = Operand::invalid();
    x.mode = guff_types::OperandMode::Constant;
    x.typ = Some(typ);
    x
}

fn assignable(
    u: &mut Universe,
    x: &Operand,
    target: TypeId,
    implements: &ImplementsFn,
    representable: &RepresentableFn,
) -> bool {
    assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        x,
        target,
        implements,
        representable,
    )
    .ok
}

// ---------------------------------------------------------------------------
// trivial / identical

#[test]
fn invalid_operand_is_assignable() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let x = Operand::invalid(); // mode == Invalid
    assert!(assignable(&mut u, &x, int, IMPLEMENTS_NONE, REPR_NONE));
}

#[test]
fn identical_types_are_assignable() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let x = val(int);
    assert!(assignable(&mut u, &x, int, IMPLEMENTS_NONE, REPR_NONE));
}

#[test]
fn distinct_basic_types_are_not_assignable() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let x = val(int);
    let r = assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        &x,
        string,
        IMPLEMENTS_NONE,
        REPR_NONE,
    );
    assert!(!r.ok);
    assert_eq!(r.code, Some(Code::IncompatibleAssign));
}

// ---------------------------------------------------------------------------
// identical underlying, one side unnamed

#[test]
fn named_to_unnamed_identical_underlying_is_assignable() {
    // type S []int;  var x []int = s  — assignable, because []int is unnamed
    // and S and []int share an underlying type. (A Basic like `int` is itself
    // "named" per Go's hasName, so `type MyInt int` would NOT be assignable to
    // int — we use an unnamed composite underlying to exercise this rule.)
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let si = new_slice(&mut u.type_arena, int); // []int (unnamed)
    let tn = new_type_name(&mut u.object_arena, "S", None);
    let s = new_named(&mut u.type_arena, &mut u.object_arena, tn, Some(si), vec![]);

    // S -> []int : identical underlying, target []int unnamed ⇒ assignable
    let x = val(s);
    let target = new_slice(&mut u.type_arena, int);
    assert!(assignable(&mut u, &x, target, IMPLEMENTS_NONE, REPR_NONE));

    // []int -> S : identical underlying, source []int unnamed ⇒ assignable
    let src = new_slice(&mut u.type_arena, int);
    let y = val(src);
    assert!(assignable(&mut u, &y, s, IMPLEMENTS_NONE, REPR_NONE));
}

#[test]
fn two_distinct_named_types_are_not_assignable() {
    // type A []int; type B []int;  A not assignable to B (both named).
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let sa = new_slice(&mut u.type_arena, int);
    let tna = new_type_name(&mut u.object_arena, "A", None);
    let a = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tna,
        Some(sa),
        vec![],
    );
    let sb = new_slice(&mut u.type_arena, int);
    let tnb = new_type_name(&mut u.object_arena, "B", None);
    let bb = new_named(
        &mut u.type_arena,
        &mut u.object_arena,
        tnb,
        Some(sb),
        vec![],
    );

    let x = val(a);
    assert!(!assignable(&mut u, &x, bb, IMPLEMENTS_NONE, REPR_NONE));
}

// ---------------------------------------------------------------------------
// untyped representability (injected)

#[test]
fn untyped_value_assignable_iff_representable() {
    let mut u = init_universe_full();
    let untyped_int = b(&u, BasicKind::UntypedInt);
    let int = b(&u, BasicKind::Int);
    let x = untyped(untyped_int);
    // representable stub says yes ⇒ assignable
    assert!(assignable(&mut u, &x, int, IMPLEMENTS_NONE, REPR_ALL));
    // representable stub says no ⇒ not assignable
    assert!(!assignable(&mut u, &x, int, IMPLEMENTS_NONE, REPR_NONE));
}

// ---------------------------------------------------------------------------
// interface satisfaction (injected)

#[test]
fn value_assignable_to_interface_iff_implements() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let empty_iface = new_interface_type(&mut u.type_arena, vec![], vec![]);

    let x = val(int);
    // implements stub says yes ⇒ assignable
    assert!(assignable(
        &mut u,
        &x,
        empty_iface,
        IMPLEMENTS_ALL,
        REPR_NONE
    ));
    // implements stub says no ⇒ not assignable, InvalidIfaceAssign
    let r = assignable_to(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        &x,
        empty_iface,
        IMPLEMENTS_NONE,
        REPR_NONE,
    );
    assert!(!r.ok);
    assert_eq!(r.code, Some(Code::InvalidIfaceAssign));
}

// ---------------------------------------------------------------------------
// bidirectional channel assignment

#[test]
fn bidirectional_chan_assignable_to_directed_unnamed_chan() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let both = new_chan(&mut u.type_arena, ChanDir::SendRecv, int);
    let send_only = new_chan(&mut u.type_arena, ChanDir::SendOnly, int);

    let x = val(both);
    // chan int -> chan<- int : identical elem, both unnamed ⇒ assignable
    assert!(assignable(
        &mut u,
        &x,
        send_only,
        IMPLEMENTS_NONE,
        REPR_NONE
    ));

    // different element type ⇒ not assignable
    let string = b(&u, BasicKind::String);
    let send_str = new_chan(&mut u.type_arena, ChanDir::SendOnly, string);
    assert!(!assignable(
        &mut u,
        &x,
        send_str,
        IMPLEMENTS_NONE,
        REPR_NONE
    ));
}

// ---------------------------------------------------------------------------
// pointers / composites that should NOT be assignable

#[test]
fn distinct_pointers_not_assignable() {
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let string = b(&u, BasicKind::String);
    let pi = new_pointer(&mut u.type_arena, int);
    let ps = new_pointer(&mut u.type_arena, string);
    let x = val(pi);
    assert!(!assignable(&mut u, &x, ps, IMPLEMENTS_NONE, REPR_NONE));
    // but identical-element unnamed pointers ARE assignable (identical types)
    let pi2 = new_pointer(&mut u.type_arena, int);
    assert!(assignable(&mut u, &x, pi2, IMPLEMENTS_NONE, REPR_NONE));
}

// ---------------------------------------------------------------------------
// type-parameter target: V (unnamed) assignable to each term

#[test]
fn unnamed_value_assignable_to_type_param_when_each_term_matches() {
    // T's type set is { []int }. A value of type []int is assignable to T
    // because it's assignable (identical) to each specific term.
    let mut u = init_universe_full();
    let int = b(&u, BasicKind::Int);
    let si = new_slice(&mut u.type_arena, int);
    let term = new_term(false, si);
    let union = new_union(&mut u.type_arena, vec![term]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    // a fresh, unnamed []int operand
    let si2 = new_slice(&mut u.type_arena, int);
    let x = val(si2);
    assert!(assignable(&mut u, &x, tp, IMPLEMENTS_NONE, REPR_NONE));

    // a []string is NOT assignable to T
    let string = b(&u, BasicKind::String);
    let ss = new_slice(&mut u.type_arena, string);
    let y = val(ss);
    assert!(!assignable(&mut u, &y, tp, IMPLEMENTS_NONE, REPR_NONE));
}

#[test]
fn type_param_with_no_terms_target_is_not_assignable() {
    // T is `any` (no specific terms): a []int value is not assignable to T
    // via the structural term rule (would need implements, stubbed false).
    let mut u = init_universe_full();
    let empty_iface = new_interface_type(&mut u.type_arena, vec![], vec![]);
    let tn = new_type_name(&mut u.object_arena, "T", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(empty_iface));

    let int = b(&u, BasicKind::Int);
    let si = new_slice(&mut u.type_arena, int);
    let x = val(si);
    assert!(!assignable(&mut u, &x, tp, IMPLEMENTS_NONE, REPR_NONE));
}
