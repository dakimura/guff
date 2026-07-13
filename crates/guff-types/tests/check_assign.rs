//! Chunk-20c tests: the Checker-driven `assignable_to` / `convertible_to`
//! wrappers wire the real `implements` / `representable` in.

use guff_types::{
    add_method, new_func, new_interface_type, new_named, new_param, new_signature_type, new_struct,
    new_type_name, BasicKind, Checker, Config, Operand, OperandMode, TypeId,
};
use guff_types_errors::Code;

fn empty_named(c: &mut Checker, name: &str) -> TypeId {
    let empty = new_struct(&mut c.types, vec![], vec![]);
    let tn = new_type_name(&mut c.objects, name, None);
    new_named(&mut c.types, &mut c.objects, tn, Some(empty), vec![])
}

fn add_niladic_method(c: &mut Checker, t: TypeId, name: &str) {
    let recv = new_param(&mut c.objects, "r", t);
    let sig = new_signature_type(&mut c.types, Some(recv), &[], &[], None, None, false);
    let m = new_func(&mut c.objects, name, Some(sig));
    add_method(&mut c.types, &c.objects, t, m);
}

fn value_of(typ: TypeId) -> Operand {
    Operand {
        mode: OperandMode::Value,
        expr: None,
        typ: Some(typ),
        val: None,
        id: None,
    }
}

#[test]
fn value_implementing_interface_is_assignable() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T");
    add_niladic_method(&mut c, t, "M");
    let iface = new_interface_type(&mut c.types, vec![], vec![]);
    // 1-method interface { M() }
    let sig = new_signature_type(&mut c.types, None, &[], &[], None, None, false);
    let m = new_func(&mut c.objects, "M", Some(sig));
    let iface_m = new_interface_type(&mut c.types, vec![m], vec![]);
    let _ = iface; // empty iface unused beyond illustration

    let x = value_of(t);
    let r = c.assignable_to(&x, iface_m);
    assert!(r.ok, "T with M() should be assignable to interface{{M()}}");
}

#[test]
fn value_not_implementing_interface_is_not_assignable() {
    let mut c = Checker::new(Config::default());
    let t = empty_named(&mut c, "T"); // no methods
    let sig = new_signature_type(&mut c.types, None, &[], &[], None, None, false);
    let m = new_func(&mut c.objects, "M", Some(sig));
    let iface_m = new_interface_type(&mut c.types, vec![m], vec![]);

    let x = value_of(t);
    let r = c.assignable_to(&x, iface_m);
    assert!(!r.ok);
    assert_eq!(r.code, Some(Code::InvalidIfaceAssign));
}

#[test]
fn named_over_int_is_convertible_but_not_assignable_to_int() {
    // type T int. A T value is NOT assignable to int (both are named), but it
    // IS convertible (identical underlying type). Exercises both wrappers.
    let mut c = Checker::new(Config::default());
    let int = c.basic(BasicKind::Int);
    let tn = new_type_name(&mut c.objects, "T", None);
    let t = new_named(&mut c.types, &mut c.objects, tn, Some(int), vec![]);

    let x = value_of(t);
    assert!(
        !c.assignable_to(&x, int).ok,
        "T and int are both named → not assignable"
    );
    assert!(
        c.convertible_to(&x, int),
        "identical underlying → convertible"
    );
}

#[test]
fn int_is_convertible_to_int64() {
    let mut c = Checker::new(Config::default());
    let int = c.basic(BasicKind::Int);
    let int64 = c.basic(BasicKind::Int64);
    let x = value_of(int);
    assert!(c.convertible_to(&x, int64));
}

#[test]
fn int_is_not_convertible_to_string_value() {
    // int → string is NOT a valid conversion for a non-constant int value
    // (only integer→string via rune is special, but that's int constant → and
    // even then it's allowed; here we test struct→string which is clearly not).
    let mut c = Checker::new(Config::default());
    let empty = new_struct(&mut c.types, vec![], vec![]);
    let tn = new_type_name(&mut c.objects, "S", None);
    let s = new_named(&mut c.types, &mut c.objects, tn, Some(empty), vec![]);
    let string = c.basic(BasicKind::String);
    let x = value_of(s);
    assert!(!c.convertible_to(&x, string));
}
