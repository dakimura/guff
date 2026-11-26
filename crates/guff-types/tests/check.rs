//! Tests for chunk-18c: the `Checker` struct, `Checker::new`, and the
//! delayed-action machinery (`later` / `process_delayed`).

use std::cell::RefCell;
use std::rc::Rc;

use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::{Checker, Config};

#[test]
fn new_checker_builds_with_universe() {
    let c = Checker::new(Config::default());

    // The basic table is populated: Typ[Invalid] is an (invalid) Basic.
    let inv = c.invalid_type();
    assert!(matches!(c.types.get(inv), TypeData::Basic(b) if b.kind() == BasicKind::Invalid));

    // A predeclared type like `int` is reachable via `basic`.
    let int = c.basic(BasicKind::Int);
    assert!(matches!(c.types.get(int), TypeData::Basic(b) if b.kind() == BasicKind::Int));

    // The package was allocated.
    let pkg = c.pkg;
    assert_eq!(c.packages.get(pkg).name(), "");

    // next_id starts at 1.
    assert_eq!(c.next_id, 1);
    assert!(c.errors.is_empty());
    assert!(c.delayed.is_empty());
}

#[test]
fn universe_handles_present() {
    let c = Checker::new(Config::default());
    // error/any/comparable are interfaces (or alias to one) — at minimum they
    // resolve to valid arena entries we can fetch.
    let _ = c.types.get(c.universe_error);
    let _ = c.types.get(c.universe_any);
    let _ = c.types.get(c.universe_comparable);
    // builtins map carries entries (len, cap, append, …).
    assert!(!c.builtins.is_empty());
}

#[test]
fn later_and_process_delayed_run_in_fifo_order() {
    let mut c = Checker::new(Config::default());
    let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));

    let l1 = log.clone();
    c.later(move |_chk| l1.borrow_mut().push(1));
    let l2 = log.clone();
    c.later(move |_chk| l2.borrow_mut().push(2));

    assert_eq!(c.delayed.len(), 2);
    c.process_delayed(0);

    assert_eq!(*log.borrow(), vec![1, 2]);
    // Segment truncated back to `top`.
    assert!(c.delayed.is_empty());
}

#[test]
fn process_delayed_runs_actions_appended_during_processing() {
    let mut c = Checker::new(Config::default());
    let log: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));

    let l = log.clone();
    c.later(move |chk: &mut Checker| {
        l.borrow_mut().push(10);
        // A nested action queued while processing must also run.
        let l_inner = l.clone();
        chk.later(move |_| l_inner.borrow_mut().push(20));
    });

    c.process_delayed(0);
    assert_eq!(*log.borrow(), vec![10, 20]);
    assert!(c.delayed.is_empty());
}

#[test]
fn push_pop_track_object_path() {
    let mut c = Checker::new(Config::default());
    assert!(c.obj_path.is_empty());
    // Use the predeclared nil object as a stand-in ObjectId.
    let obj = c.universe_nil;
    c.push(obj);
    assert_eq!(c.obj_path.len(), 1);
    let popped = c.pop();
    assert_eq!(popped, Some(obj));
    assert!(c.obj_path.is_empty());
}
