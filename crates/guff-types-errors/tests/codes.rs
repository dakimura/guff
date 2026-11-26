//! Spot-checks for Code numeric values and Display names.

use guff_types_errors::Code;

#[test]
fn invalid_syntax_tree_is_minus_one() {
    assert_eq!(Code::InvalidSyntaxTree.to_i16(), -1);
    assert_eq!(Code::InvalidSyntaxTree.to_string(), "InvalidSyntaxTree");
}

#[test]
fn anchors_match_go_numeric_values() {
    // Spot-check anchors across each block to catch off-by-one regressions.
    assert_eq!(Code::Test.to_i16(), 1);
    assert_eq!(Code::IncomparableMapKey.to_i16(), 28);
    assert_eq!(Code::InvalidPtrEmbed.to_i16(), 30); // skip 29
    assert_eq!(Code::NonVariadicDotDotDot.to_i16(), 78);
    assert_eq!(Code::InvalidDotDotDot.to_i16(), 81); // skip 79, 80
    assert_eq!(Code::InvalidPostDecl.to_i16(), 106);
    assert_eq!(Code::InvalidIterVar.to_i16(), 108); // skip 107
    assert_eq!(Code::InvalidUnsafeString.to_i16(), 146);
    assert_eq!(Code::InvalidClear.to_i16(), 148); // skip 147
    assert_eq!(Code::TooNew.to_i16(), 151);
}

#[test]
fn display_uses_variant_identifier() {
    assert_eq!(Code::UnusedImport.to_string(), "UnusedImport");
    assert_eq!(Code::InvalidUntypedConversion.to_string(), "InvalidUntypedConversion");
    assert_eq!(Code::MisplacedConstraintIface.to_string(), "MisplacedConstraintIface");
    assert_eq!(Code::TooNew.to_string(), "TooNew");
}

#[test]
fn from_i16_round_trip_known_values() {
    for c in [
        Code::InvalidSyntaxTree,
        Code::Test,
        Code::IncomparableMapKey,
        Code::InvalidPtrEmbed,
        Code::InvalidDotDotDot,
        Code::InvalidIterVar,
        Code::InvalidClear,
        Code::TooNew,
    ] {
        let n = c.to_i16();
        assert_eq!(Code::from_i16(n), Some(c), "round-trip for {:?}", c);
    }
}

#[test]
fn from_i16_returns_none_for_gaps_and_unknowns() {
    // Retired/never-used Go codes.
    for n in [0, 29, 79, 80, 107, 147] {
        assert_eq!(Code::from_i16(n), None, "gap at {}", n);
    }
    // Out of range.
    assert_eq!(Code::from_i16(-2), None);
    assert_eq!(Code::from_i16(152), None);
    assert_eq!(Code::from_i16(9999), None);
}
