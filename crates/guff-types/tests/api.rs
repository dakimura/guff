//! Tests for chunk-18a scaffolding: `Config`, `Info`, `TypeAndValue`,
//! `TypeCheckError`. These mostly assert the types compile and have the
//! expected default shape — the behaviour arrives in later chunks.

use guff_constant::Value;
use guff_types::api::{Config, Info, TypeAndValue, TypeCheckError};
use guff_types::init_universe_full;
use guff_types::operand::OperandMode;
use guff_types_errors::Code;

#[test]
fn config_default_is_empty() {
    let c = Config::default();
    assert_eq!(c.go_version, "");
    assert!(!c.disable_unused_import_check);
    assert!(!c.trace);
}

#[test]
fn info_default_is_empty() {
    let info = Info::default();
    assert!(info.types.is_empty());
    assert!(info.defs.is_empty());
    assert!(info.uses.is_empty());
}

#[test]
fn type_and_value_holds_mode_type_value() {
    let u = init_universe_full();
    let int = u.typ[guff_types::BasicKind::Int as usize];

    let tv = TypeAndValue {
        mode: OperandMode::Constant,
        typ: int,
        val: Some(Value::Int64(7)),
    };
    assert_eq!(tv.mode, OperandMode::Constant);
    assert_eq!(tv.typ, int);
    assert!(matches!(tv.val, Some(Value::Int64(7))));
}

#[test]
fn type_check_error_constructs() {
    let e = TypeCheckError {
        pos: 0,
        code: Code::IncompatibleAssign,
        msg: "boom".to_string(),
    };
    assert_eq!(e.pos, 0);
    assert_eq!(e.code, Code::IncompatibleAssign);
    assert_eq!(e.msg, "boom");
}
