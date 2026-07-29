//! Chunk-20b tests: `Checker::representable` / `representable_const`.

use guff_constant::Value;
use guff_types::operand::{Operand, OperandMode};
use guff_types::{representable_const, BasicKind, Checker, Config};
use guff_types_errors::Code;

/// Build a Constant operand with untyped-int type holding `n`.
fn untyped_int_const(c: &Checker, n: i64) -> Operand<'static> {
    Operand {
        mode: OperandMode::Constant,
        expr: None,
        typ: Some(c.basic(BasicKind::UntypedInt)),
        val: Some(Value::Int64(n)),
        id: None,
    }
}

#[test]
fn representable_const_int_ranges() {
    let c = Checker::new(Config::default());
    let int8 = c.basic(BasicKind::Int8);
    let uint8 = c.basic(BasicKind::Uint8);

    assert!(representable_const(&c.types, &Value::Int64(100), int8).is_some());
    assert!(representable_const(&c.types, &Value::Int64(127), int8).is_some());
    assert!(representable_const(&c.types, &Value::Int64(128), int8).is_none());
    assert!(representable_const(&c.types, &Value::Int64(-128), int8).is_some());
    assert!(representable_const(&c.types, &Value::Int64(-129), int8).is_none());
    assert!(representable_const(&c.types, &Value::Int64(1000), int8).is_none());

    assert!(representable_const(&c.types, &Value::Int64(255), uint8).is_some());
    assert!(representable_const(&c.types, &Value::Int64(256), uint8).is_none());
    assert!(representable_const(&c.types, &Value::Int64(-1), uint8).is_none());
}

#[test]
fn representable_const_string_and_bool() {
    let c = Checker::new(Config::default());
    let s = c.basic(BasicKind::String);
    let b = c.basic(BasicKind::Bool);
    let int = c.basic(BasicKind::Int);

    let str_val = Value::String(std::sync::Arc::new("hi".to_string()));
    assert!(representable_const(&c.types, &str_val, s).is_some());
    assert!(representable_const(&c.types, &str_val, int).is_none());

    assert!(representable_const(&c.types, &Value::Bool(true), b).is_some());
    assert!(representable_const(&c.types, &Value::Bool(true), int).is_none());
}

#[test]
fn representable_succeeds_and_keeps_constant_mode() {
    let mut c = Checker::new(Config::default());
    let int8 = c.basic(BasicKind::Int8);
    let mut x = untyped_int_const(&c, 100);

    c.representable(&mut x, int8);

    assert_eq!(x.mode, OperandMode::Constant);
    assert!(matches!(x.val, Some(Value::Int64(100))));
    assert!(c.errors.is_empty());
}

#[test]
fn representable_overflow_reports_error_and_invalidates() {
    let mut c = Checker::new(Config::default());
    let int8 = c.basic(BasicKind::Int8);
    let mut x = untyped_int_const(&c, 1000);

    c.representable(&mut x, int8);

    assert_eq!(x.mode, OperandMode::Invalid);
    assert_eq!(c.errors.len(), 1);
    assert_eq!(c.errors[0].code, Code::NumericOverflow);
    assert!(
        c.errors[0].msg.contains("overflows int8"),
        "msg: {}",
        c.errors[0].msg
    );
}

#[test]
fn representable_bool_wrapper() {
    let c = Checker::new(Config::default());
    let int8 = c.basic(BasicKind::Int8);
    let ok = untyped_int_const(&c, 50);
    let bad = untyped_int_const(&c, 9999);
    assert!(c.representable_bool(&ok, int8));
    assert!(!c.representable_bool(&bad, int8));
}
