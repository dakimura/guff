//! Chunk-5 tests: predicate suite + structural Identical.

use guff_types::{bind_tparams, signature_set_type_params};
use guff_types::{
    comparable, default_type, has_name, has_nil, identical, is_boolean, is_complex, is_const_type,
    is_float, is_integer, is_interface, is_numeric, is_string, is_type_lit, is_type_param,
    is_typed, is_unsigned, is_untyped, is_untyped_numeric, is_valid, is_valid_name, max_type,
    new_chan, new_func, new_interface_type, new_map, new_named, new_pointer, new_signature_type,
    new_slice, new_struct, new_type_name, new_type_param, new_var, set_constraint, BasicKind,
    ChanDir, ObjectArena, PackageArena,
};

// ----------------------------------------------------------------------------
// Simple predicates

#[test]
fn basic_info_predicates() {
    let (mut t, table) = guff_types::init_universe();
    let _ = &mut t; // silence the unused warning for some checks
    let bool_ = table[BasicKind::Bool as usize];
    let int = table[BasicKind::Int as usize];
    let u = table[BasicKind::Uint as usize];
    let f64 = table[BasicKind::Float64 as usize];
    let c64 = table[BasicKind::Complex64 as usize];
    let s = table[BasicKind::String as usize];
    let untyped_int = table[BasicKind::UntypedInt as usize];

    assert!(is_boolean(&t, bool_));
    assert!(is_integer(&t, int));
    assert!(!is_unsigned(&t, int));
    assert!(is_unsigned(&t, u));
    assert!(is_float(&t, f64));
    assert!(is_complex(&t, c64));
    assert!(is_numeric(&t, int));
    assert!(is_numeric(&t, f64));
    assert!(is_numeric(&t, c64));
    assert!(!is_numeric(&t, s));
    assert!(is_string(&t, s));
    assert!(is_const_type(&t, int));
    assert!(is_const_type(&t, s));
    assert!(is_const_type(&t, bool_));
    assert!(is_untyped(&t, untyped_int));
    assert!(is_typed(&t, int));
    assert!(is_untyped_numeric(&t, untyped_int));
    assert!(!is_untyped_numeric(&t, int)); // typed, not untyped
}

#[test]
fn identity_class_predicates() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    // Named type
    let tn = new_type_name(&mut o, "MyInt", None);
    let named = new_named(&mut t, &mut o, tn, Some(int), vec![]);

    assert!(has_name(&t, int)); // Basic
    assert!(has_name(&t, named)); // Named
    assert!(is_type_lit(&t, int)); // Basic counts as literal
    assert!(!is_type_lit(&t, named)); // Named does not

    // Interface
    let iface = new_interface_type(&mut t, vec![], vec![]);
    assert!(is_interface(&t, iface));
    assert!(!is_interface(&t, int));

    // TypeParam
    let tn_p = new_type_name(&mut o, "P", None);
    let tp = new_type_param(&mut t, tn_p, None);
    assert!(is_type_param(&t, tp));
    assert!(!is_type_param(&t, int));
    assert!(has_name(&t, tp));
}

#[test]
fn validity_predicate() {
    let (t, table) = guff_types::init_universe();
    let invalid = table[BasicKind::Invalid as usize];
    let int = table[BasicKind::Int as usize];
    assert!(!is_valid(&t, invalid));
    assert!(is_valid(&t, int));
}

#[test]
fn has_nil_predicate() {
    let (mut t, table) = guff_types::init_universe();
    let int = table[BasicKind::Int as usize];
    let unsafe_ptr = table[BasicKind::UnsafePointer as usize];

    let ptr = new_pointer(&mut t, int);
    let sl = new_slice(&mut t, int);
    let mp = new_map(&mut t, int, int);
    let ch = new_chan(&mut t, ChanDir::SendRecv, int);
    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);

    // `has_nil` takes the object and package arenas because a *type
    // parameter*'s answer depends on its constraint's type set, which is
    // computed lazily; none of the concrete types below reach that path.
    let o = ObjectArena::new();
    let pk = PackageArena::new();
    assert!(has_nil(&mut t, &o, &pk, ptr));
    assert!(has_nil(&mut t, &o, &pk, sl));
    assert!(has_nil(&mut t, &o, &pk, mp));
    assert!(has_nil(&mut t, &o, &pk, ch));
    assert!(has_nil(&mut t, &o, &pk, sig));
    assert!(has_nil(&mut t, &o, &pk, unsafe_ptr));
    assert!(!has_nil(&mut t, &o, &pk, int));
}

#[test]
fn is_valid_name_basics() {
    assert!(is_valid_name("foo"));
    assert!(is_valid_name("_bar"));
    assert!(is_valid_name("Foo123"));
    assert!(is_valid_name("_"));
    assert!(!is_valid_name(""));
    assert!(!is_valid_name("1foo")); // starts with digit
    assert!(!is_valid_name("foo-bar")); // dash not allowed
    assert!(!is_valid_name("foo bar")); // space not allowed
}

// ----------------------------------------------------------------------------
// Default / max_type

#[test]
fn default_type_maps_untyped_to_typed() {
    let (t, table) = guff_types::init_universe();
    assert_eq!(
        default_type(&t, &table, table[BasicKind::UntypedBool as usize]),
        table[BasicKind::Bool as usize]
    );
    assert_eq!(
        default_type(&t, &table, table[BasicKind::UntypedInt as usize]),
        table[BasicKind::Int as usize]
    );
    assert_eq!(
        default_type(&t, &table, table[BasicKind::UntypedFloat as usize]),
        table[BasicKind::Float64 as usize]
    );
    assert_eq!(
        default_type(&t, &table, table[BasicKind::UntypedString as usize]),
        table[BasicKind::String as usize]
    );
    // Typed types pass through unchanged.
    let int = table[BasicKind::Int as usize];
    assert_eq!(default_type(&t, &table, int), int);
}

#[test]
fn max_type_picks_larger_untyped_numeric() {
    let (t, table) = guff_types::init_universe();
    let ui = table[BasicKind::UntypedInt as usize];
    let uf = table[BasicKind::UntypedFloat as usize];
    let uc = table[BasicKind::UntypedComplex as usize];

    // UntypedInt < UntypedRune < UntypedFloat < UntypedComplex.
    assert_eq!(max_type(&t, ui, uf), Some(uf));
    assert_eq!(max_type(&t, uf, ui), Some(uf));
    assert_eq!(max_type(&t, uf, uc), Some(uc));
    assert_eq!(max_type(&t, ui, ui), Some(ui)); // same-type
                                                // Mixed typed + untyped → None
    let int = table[BasicKind::Int as usize];
    assert_eq!(max_type(&t, ui, int), None);
}

// ----------------------------------------------------------------------------
// Identical

#[test]
fn identical_basics_use_kind() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let s = table[BasicKind::String as usize];
    assert!(identical(&mut t, &o, &PackageArena::new(), int, int));
    assert!(!identical(&mut t, &o, &PackageArena::new(), int, s));
}

#[test]
fn identical_anonymous_slices_structurally() {
    // Two independently-allocated `[]int` slices are Identical.
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let s1 = new_slice(&mut t, int);
    let s2 = new_slice(&mut t, int);
    assert_ne!(s1, s2); // different IDs
    assert!(identical(&mut t, &o, &PackageArena::new(), s1, s2));
}

#[test]
fn identical_arrays_check_length_and_elem() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];
    let a1 = guff_types::new_array(&mut t, int, 5);
    let a2 = guff_types::new_array(&mut t, int, 5);
    let a3 = guff_types::new_array(&mut t, int, 6);
    let a4 = guff_types::new_array(&mut t, str_id, 5);
    assert!(identical(&mut t, &o, &PackageArena::new(), a1, a2));
    assert!(!identical(&mut t, &o, &PackageArena::new(), a1, a3)); // length differs
    assert!(!identical(&mut t, &o, &PackageArena::new(), a1, a4)); // elem differs
                                                                   // Negative length (unknown) is treated as equal.
    let a5 = guff_types::new_array(&mut t, int, -1);
    assert!(identical(&mut t, &o, &PackageArena::new(), a1, a5));
}

#[test]
fn identical_pointers_and_maps_and_chans() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];

    let p1 = new_pointer(&mut t, int);
    let p2 = new_pointer(&mut t, int);
    assert!(identical(&mut t, &o, &PackageArena::new(), p1, p2));

    let m1 = new_map(&mut t, str_id, int);
    let m2 = new_map(&mut t, str_id, int);
    let m3 = new_map(&mut t, int, str_id);
    assert!(identical(&mut t, &o, &PackageArena::new(), m1, m2));
    assert!(!identical(&mut t, &o, &PackageArena::new(), m1, m3));

    let c1 = new_chan(&mut t, ChanDir::SendRecv, int);
    let c2 = new_chan(&mut t, ChanDir::SendRecv, int);
    let c3 = new_chan(&mut t, ChanDir::SendOnly, int);
    assert!(identical(&mut t, &o, &PackageArena::new(), c1, c2));
    assert!(!identical(&mut t, &o, &PackageArena::new(), c1, c3)); // direction differs
}

#[test]
fn identical_structs_match_fields_by_name_and_type() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let str_id = table[BasicKind::String as usize];

    let f1 = new_var(&mut o, "x", int);
    let f2 = new_var(&mut o, "y", str_id);
    let s1 = new_struct(&mut t, vec![f1, f2], vec![]);

    let f1b = new_var(&mut o, "x", int);
    let f2b = new_var(&mut o, "y", str_id);
    let s2 = new_struct(&mut t, vec![f1b, f2b], vec![]);
    assert!(identical(&mut t, &o, &PackageArena::new(), s1, s2));

    // Different field name → not identical
    let f1c = new_var(&mut o, "x", int);
    let f2c = new_var(&mut o, "z", str_id);
    let s3 = new_struct(&mut t, vec![f1c, f2c], vec![]);
    assert!(!identical(&mut t, &o, &PackageArena::new(), s1, s3));
}

#[test]
fn identical_named_compares_by_typename() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    let tn_a = new_type_name(&mut o, "A", None);
    let na = new_named(&mut t, &mut o, tn_a, Some(int), vec![]);
    let na_again = na;
    // Same Named (same id, same TypeName) → identical.
    assert!(identical(&mut t, &o, &PackageArena::new(), na, na_again));

    // Different Named (different TypeName) → not identical even with the
    // same underlying type.
    let tn_b = new_type_name(&mut o, "B", None);
    let nb = new_named(&mut t, &mut o, tn_b, Some(int), vec![]);
    assert!(!identical(&mut t, &o, &PackageArena::new(), na, nb));
}

#[test]
fn identical_signatures_compare_variadic_and_param_results() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    let p1 = new_var(&mut o, "x", int);
    let pt1 = guff_types::new_tuple(&mut t, &[p1]);
    let r1 = new_var(&mut o, "", int);
    let rt1 = guff_types::new_tuple(&mut t, &[r1]);
    let sig1 = new_signature_type(&mut t, None, &[], &[], pt1, rt1, false);

    let p2 = new_var(&mut o, "x", int);
    let pt2 = guff_types::new_tuple(&mut t, &[p2]);
    let r2 = new_var(&mut o, "", int);
    let rt2 = guff_types::new_tuple(&mut t, &[r2]);
    let sig2 = new_signature_type(&mut t, None, &[], &[], pt2, rt2, false);
    assert!(identical(&mut t, &o, &PackageArena::new(), sig1, sig2));

    // Variadic differs.
    let p3 = new_var(&mut o, "xs", new_slice(&mut t, int));
    let pt3 = guff_types::new_tuple(&mut t, &[p3]);
    let sig3 = new_signature_type(&mut t, None, &[], &[], pt3, None, true);
    assert!(!identical(&mut t, &o, &PackageArena::new(), sig1, sig3));
}

#[test]
fn identical_interfaces_compare_method_signatures() {
    let (mut t, _) = guff_types::init_universe();
    let mut o = ObjectArena::new();

    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m1 = new_func(&mut o, "Foo", Some(sig));
    let m1b = new_func(&mut o, "Foo", Some(sig));

    let i1 = new_interface_type(&mut t, vec![m1], vec![]);
    let i2 = new_interface_type(&mut t, vec![m1b], vec![]);
    assert!(identical(&mut t, &o, &PackageArena::new(), i1, i2));

    // Different method name → not identical.
    let m2 = new_func(&mut o, "Bar", Some(sig));
    let i3 = new_interface_type(&mut t, vec![m2], vec![]);
    assert!(!identical(&mut t, &o, &PackageArena::new(), i1, i3));
}

// ----------------------------------------------------------------------------
// Generic-signature identity (D06 — type parameters identical modulo renaming)

/// Build a single-type-parameter identity signature `func[<name> bound](x P) P`.
fn generic_identity_sig(
    t: &mut guff_types::TypeArena,
    o: &mut ObjectArena,
    name: &str,
    bound: Option<guff_types::TypeId>,
) -> guff_types::TypeId {
    let tn = new_type_name(o, name, None);
    let tp = new_type_param(t, tn, bound);
    let tpl = bind_tparams(t, vec![tp]).unwrap();
    let p = new_var(o, "x", tp);
    let pt = guff_types::new_tuple(t, &[p]);
    let r = new_var(o, "", tp);
    let rt = guff_types::new_tuple(t, &[r]);
    let sig = new_signature_type(t, None, &[], &[], pt, rt, false);
    signature_set_type_params(t, sig, tpl);
    sig
}

#[test]
fn identical_generic_signatures_modulo_renaming() {
    let (mut t, _) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let empty = new_interface_type(&mut t, vec![], vec![]);

    // `func[T any](T) T` and `func[U any](U) U` are identical despite the
    // differently-named (distinct-TypeId) type parameters.
    let sig1 = generic_identity_sig(&mut t, &mut o, "T", Some(empty));
    let sig2 = generic_identity_sig(&mut t, &mut o, "U", Some(empty));
    assert!(identical(&mut t, &o, &PackageArena::new(), sig1, sig2));
}

#[test]
fn generic_vs_nongeneric_signature_differ() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let empty = new_interface_type(&mut t, vec![], vec![]);

    // `func[T any](T) T` vs `func(int) int` — the generic one has a type
    // parameter, so they must not be identical.
    let gen = generic_identity_sig(&mut t, &mut o, "T", Some(empty));

    let p = new_var(&mut o, "x", int);
    let pt = guff_types::new_tuple(&mut t, &[p]);
    let r = new_var(&mut o, "", int);
    let rt = guff_types::new_tuple(&mut t, &[r]);
    let plain = new_signature_type(&mut t, None, &[], &[], pt, rt, false);

    assert!(!identical(&mut t, &o, &PackageArena::new(), gen, plain));
}

#[test]
fn generic_signatures_with_different_constraints_differ() {
    let (mut t, _) = guff_types::init_universe();
    let mut o = ObjectArena::new();

    // Bound A: `any` (empty interface). Bound B: `interface{ Foo() }`.
    let empty = new_interface_type(&mut t, vec![], vec![]);
    let m_sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m = new_func(&mut o, "Foo", Some(m_sig));
    let with_method = new_interface_type(&mut t, vec![m], vec![]);

    let sig1 = generic_identity_sig(&mut t, &mut o, "T", Some(empty));
    let sig2 = generic_identity_sig(&mut t, &mut o, "U", Some(with_method));

    // Same shape, but constraints differ after substitution → not identical.
    assert!(!identical(&mut t, &o, &PackageArena::new(), sig1, sig2));

    // Sanity: with the same constraint they *are* identical modulo renaming.
    let sig3 = generic_identity_sig(&mut t, &mut o, "V", Some(empty));
    assert!(identical(&mut t, &o, &PackageArena::new(), sig1, sig3));
}

#[test]
fn generic_signatures_with_different_arity_differ() {
    let (mut t, _) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let empty = new_interface_type(&mut t, vec![], vec![]);

    // One type parameter vs two.
    let sig1 = generic_identity_sig(&mut t, &mut o, "T", Some(empty));

    let tn_a = new_type_name(&mut o, "A", None);
    let tp_a = new_type_param(&mut t, tn_a, Some(empty));
    let tn_b = new_type_name(&mut o, "B", None);
    let tp_b = new_type_param(&mut t, tn_b, Some(empty));
    let tpl = bind_tparams(&mut t, vec![tp_a, tp_b]).unwrap();
    let p = new_var(&mut o, "x", tp_a);
    let pt = guff_types::new_tuple(&mut t, &[p]);
    let r = new_var(&mut o, "", tp_a);
    let rt = guff_types::new_tuple(&mut t, &[r]);
    let sig2 = new_signature_type(&mut t, None, &[], &[], pt, rt, false);
    signature_set_type_params(&mut t, sig2, tpl);

    assert!(!identical(&mut t, &o, &PackageArena::new(), sig1, sig2));
}

// ----------------------------------------------------------------------------
// Comparable

#[test]
fn comparable_basics() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let s = table[BasicKind::String as usize];

    assert!(comparable(&mut t, &o, &PackageArena::new(), int));
    assert!(comparable(&mut t, &o, &PackageArena::new(), s));

    // Pointer / chan are always comparable.
    let p = new_pointer(&mut t, int);
    let c = new_chan(&mut t, ChanDir::SendRecv, int);
    assert!(comparable(&mut t, &o, &PackageArena::new(), p));
    assert!(comparable(&mut t, &o, &PackageArena::new(), c));
}

#[test]
fn comparable_slice_is_not() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let sl = new_slice(&mut t, int);
    assert!(!comparable(&mut t, &o, &PackageArena::new(), sl));

    // Map is also not comparable.
    let mp = new_map(&mut t, int, int);
    assert!(!comparable(&mut t, &o, &PackageArena::new(), mp));
}

#[test]
fn comparable_struct_recurses_into_fields() {
    let (mut t, table) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];

    // Comparable struct.
    let f_ok = new_var(&mut o, "x", int);
    let s_ok = new_struct(&mut t, vec![f_ok], vec![]);
    assert!(comparable(&mut t, &o, &PackageArena::new(), s_ok));

    // Struct containing a slice → not comparable.
    let sl = new_slice(&mut t, int);
    let f_bad = new_var(&mut o, "xs", sl);
    let s_bad = new_struct(&mut t, vec![f_bad], vec![]);
    assert!(!comparable(&mut t, &o, &PackageArena::new(), s_bad));
}

#[test]
fn comparable_array_recurses_into_elem() {
    let (mut t, table) = guff_types::init_universe();
    let o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let sl = new_slice(&mut t, int);
    let a_int = guff_types::new_array(&mut t, int, 3);
    let a_sl = guff_types::new_array(&mut t, sl, 3);
    assert!(comparable(&mut t, &o, &PackageArena::new(), a_int));
    assert!(!comparable(&mut t, &o, &PackageArena::new(), a_sl));
}

#[test]
fn comparable_interface_dynamic() {
    // In dynamic mode (the public Comparable), any non-TypeParam interface
    // counts as comparable.
    let (mut t, _) = guff_types::init_universe();
    let o = ObjectArena::new();
    let iface = new_interface_type(&mut t, vec![], vec![]);
    assert!(comparable(&mut t, &o, &PackageArena::new(), iface));
}

#[test]
fn typeparam_underlying_via_predicate_path() {
    // Smoke: even with the simplified predicates, building a TypeParam
    // with an Interface constraint and asking is_interface should work
    // through Underlying.
    let (mut t, _) = guff_types::init_universe();
    let mut o = ObjectArena::new();
    let i = new_interface_type(&mut t, vec![], vec![]);
    let tn = new_type_name(&mut o, "P", None);
    let tp = new_type_param(&mut t, tn, Some(i));
    set_constraint(&mut t, tp, i);
    // is_type_param is true; is_interface — note this looks at the underlying,
    // which for chunk-3 TypeParam returns the bound's underlying iff Interface
    // (so should be true here).
    assert!(is_type_param(&t, tp));
    assert!(is_interface(&t, tp));
}
