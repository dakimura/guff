//! Chunk-10 tests: `under.rs` — `all`, `under_is`, `common_under`,
//! `TypeError`.

use guff_types::{
    all, common_under, init_universe_full, new_chan, new_interface_type, new_pointer, new_term,
    new_type_name, new_type_param, new_union, type_errorf, under_is, BasicKind, ChanDir,
};

#[test]
fn under_is_non_typeparam_calls_f_with_underlying() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let ptr = new_pointer(&mut u.type_arena, int);

    let mut seen: Vec<guff_types::TypeId> = Vec::new();
    let ok = under_is(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        ptr,
        |x| {
            if let Some(t) = x {
                seen.push(t);
            }
            true
        },
    );
    assert!(ok);
    // For a non-TypeParam, the underlying of a Pointer is the Pointer itself.
    assert_eq!(seen, vec![ptr]);
}

#[test]
fn all_calls_callback_once_for_non_typeparam() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];

    let mut calls = 0;
    let mut got_t = None;
    let mut got_u = None;
    let ok = all(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        |t, u_| {
            calls += 1;
            got_t = t;
            got_u = u_;
            true
        },
    );
    assert!(ok);
    assert_eq!(calls, 1);
    assert_eq!(got_t, Some(int));
    assert_eq!(got_u, Some(int)); // Basic's underlying is itself
}

#[test]
fn all_typeparam_with_no_terms_yields_nil_nil() {
    let mut u = init_universe_full();
    // Bound: empty interface → no specific terms → callback gets (None,
    // None) exactly once.
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);

    let mut calls = 0;
    let mut saw_both_none = false;
    let _ = all(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        |t, u_| {
            calls += 1;
            if t.is_none() && u_.is_none() {
                saw_both_none = true;
            }
            true
        },
    );
    assert_eq!(calls, 1);
    assert!(
        saw_both_none,
        "expected (None, None) callback for typeparam with no terms"
    );
}

#[test]
fn all_typeparam_with_union_constraint_yields_each_term() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];

    // type P interface { int | string }
    let term_int = new_term(false, int);
    let term_s = new_term(false, s);
    let union = new_union(&mut u.type_arena, vec![term_int, term_s]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);

    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    let mut pairs: Vec<(
        Option<guff_types::TypeId>,
        Option<guff_types::TypeId>,
    )> = Vec::new();
    let ok = all(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        |t, u_| {
            pairs.push((t, u_));
            true
        },
    );
    assert!(ok);
    assert_eq!(pairs.len(), 2);
    let typs: Vec<_> = pairs.iter().filter_map(|(t, _)| *t).collect();
    assert!(typs.contains(&int));
    assert!(typs.contains(&s));
}

#[test]
fn common_under_returns_underlying_for_non_typeparam() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        None,
    );
    assert!(err.is_none(), "got err: {:?}", err);
    assert_eq!(cu, Some(int));
}

#[test]
fn common_under_typeparam_with_no_terms_errors() {
    let mut u = init_universe_full();
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, None);
    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        None,
    );
    assert!(cu.is_none());
    let err = err.expect("typeparam with no terms is an error");
    assert!(err.format().contains("no specific type"));
}

#[test]
fn common_under_typeparam_with_chan_terms_picks_most_restricted() {
    // type P interface { chan int | chan<- int }  →  chan<- int wins.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let bidir = new_chan(&mut u.type_arena, ChanDir::SendRecv, int);
    let sendonly = new_chan(&mut u.type_arena, ChanDir::SendOnly, int);

    let t1 = new_term(false, bidir);
    let t2 = new_term(false, sendonly);
    let union = new_union(&mut u.type_arena, vec![t1, t2]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        None,
    );
    assert!(err.is_none(), "got err: {:?}", err);
    let cu = cu.expect("common underlying exists");
    // The result must be a sendonly channel (the restricted one).
    match u.type_arena.get(cu) {
        guff_types::TypeData::Chan(c) => {
            assert_eq!(c.dir(), ChanDir::SendOnly);
            assert_eq!(c.elem(), int);
        }
        other => panic!("expected Chan, got {:?}", std::mem::discriminant(other)),
    }
}

#[test]
fn common_under_typeparam_with_conflicting_chan_dirs_errors() {
    // type P interface { chan<- int | <-chan int }  →  conflict.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let send = new_chan(&mut u.type_arena, ChanDir::SendOnly, int);
    let recv = new_chan(&mut u.type_arena, ChanDir::RecvOnly, int);
    let t1 = new_term(false, send);
    let t2 = new_term(false, recv);
    let union = new_union(&mut u.type_arena, vec![t1, t2]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        None,
    );
    assert!(cu.is_none());
    let err = err.expect("conflict produces err");
    assert!(err.format().contains("conflicting directions"));
}

#[test]
fn common_under_typeparam_with_two_distinct_underlyings_errors() {
    // type P interface { int | string }  →  different underlyings.
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];
    let s = u.typ[BasicKind::String as usize];
    let t1 = new_term(false, int);
    let t2 = new_term(false, s);
    let union = new_union(&mut u.type_arena, vec![t1, t2]);
    let iface = new_interface_type(&mut u.type_arena, vec![], vec![union]);
    let tn = new_type_name(&mut u.object_arena, "P", None);
    let tp = new_type_param(&mut u.type_arena, tn, Some(iface));

    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        tp,
        None,
    );
    assert!(cu.is_none());
    let err = err.expect("distinct underlyings → err");
    assert!(err.format().contains("different underlying types"));
}

#[test]
fn common_under_cond_callback_short_circuits() {
    let mut u = init_universe_full();
    let int = u.typ[BasicKind::Int as usize];

    let mut cond = |_t, _u_| -> Option<guff_types::TypeError> {
        Some(type_errorf("custom cond rejected", Vec::new()))
    };
    let (cu, err) = common_under(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        int,
        Some(&mut cond),
    );
    assert!(cu.is_none());
    assert_eq!(err.unwrap().format(), "custom cond rejected");
}

#[test]
fn type_errorf_substitutes_pct_s_args() {
    let e = type_errorf("want %s, got %s", vec!["int".into(), "string".into()]);
    assert_eq!(e.format(), "want int, got string");
}

#[test]
fn type_errorf_empty_format_is_canonical_empty() {
    let e = type_errorf("", Vec::new());
    assert!(e.is_empty());
}
