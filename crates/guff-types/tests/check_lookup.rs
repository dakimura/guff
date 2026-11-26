//! Chunk-20a tests: `Checker::implements` / `Checker::missing_method`.

use guff_types::{
    add_method, new_func, new_interface_type, new_named, new_param, new_signature_type, new_struct,
    new_type_name, BasicKind, Checker, Config,
};

/// Build `type T struct{}` (a Named over an empty struct) on the checker's
/// arenas, returning its `TypeId`.
fn empty_named(c: &mut Checker, name: &str) -> guff_types::TypeId {
    let empty = new_struct(&mut c.types, vec![], vec![]);
    let tn = new_type_name(&mut c.objects, name, None);
    new_named(&mut c.types, &mut c.objects, tn, Some(empty), vec![])
}

/// Add a niladic method `name()` to Named type `t`.
fn add_niladic_method(c: &mut Checker, t: guff_types::TypeId, name: &str) {
    let recv = new_param(&mut c.objects, "r", t);
    let sig = new_signature_type(&mut c.types, Some(recv), &[], &[], None, None, false);
    let m = new_func(&mut c.objects, name, Some(sig));
    add_method(&mut c.types, &c.objects, t, m);
}

/// Build `interface { <names...>() }` with niladic methods.
fn niladic_iface(c: &mut Checker, names: &[&str]) -> guff_types::TypeId {
    let mut methods = Vec::new();
    for name in names {
        let sig = new_signature_type(&mut c.types, None, &[], &[], None, None, false);
        methods.push(new_func(&mut c.objects, *name, Some(sig)));
    }
    new_interface_type(&mut c.types, methods, vec![])
}

#[test]
fn empty_interface_is_satisfied_by_anything() {
    let mut c = Checker::new(Config::default());
    let empty_iface = new_interface_type(&mut c.types, vec![], vec![]);
    let int = c.basic(BasicKind::Int);
    assert!(c.implements(int, empty_iface, false).is_ok());

    let t = empty_named(&mut c, "T");
    assert!(c.implements(t, empty_iface, false).is_ok());
}

#[test]
fn named_type_with_method_implements_one_method_interface() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T");
    add_niladic_method(&mut c, t, "M");

    let iface = niladic_iface(&mut c, &["M"]);
    assert!(c.implements(t, iface, false).is_ok());
    assert!(c.missing_method(t, iface, true).is_none());
}

#[test]
fn type_lacking_method_does_not_implement() {
    let mut c = Checker::new(Config::default());
    // T has no methods at all.
    let t = empty_named(&mut c, "T");
    let iface = niladic_iface(&mut c, &["M"]);

    let res = c.implements(t, iface, false);
    assert!(res.is_err());
    let cause = res.unwrap_err();
    assert!(cause.contains("missing method M"), "cause was: {cause}");

    // missing_method pinpoints M.
    let mm = c.missing_method(t, iface, true).expect("M is missing");
    assert_eq!(mm.method.name(&c.objects), "M");
    assert!(!mm.wrong_type); // absent, not wrong signature
}

#[test]
fn type_with_different_method_does_not_implement() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T");
    add_niladic_method(&mut c, t, "N"); // has N, not M
    let iface = niladic_iface(&mut c, &["M"]);

    assert!(c.implements(t, iface, false).is_err());
}

#[test]
fn non_interface_target_is_not_implemented() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T");
    let int = c.basic(BasicKind::Int);
    let res = c.implements(t, int, false);
    assert!(res.is_err());
    assert!(res.unwrap_err().contains("not an interface"));
}

#[test]
fn implements_bool_wrapper_matches() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T");
    add_niladic_method(&mut c, t, "M");
    let iface = niladic_iface(&mut c, &["M"]);
    assert!(c.implements_bool(t, iface));

    let t2 = empty_named(&mut c, "T2");
    assert!(!c.implements_bool(t2, iface));
}
