//! Chunk-8 tests: VarKind + embedded flag, ObjectId::same_id, object::cmp,
//! Var.sameId in struct identity, Object.cmp ordering in interface methods.

use guff_types::{
    identical, init_universe, new_field, new_func, new_interface_type, new_package, new_param,
    new_scope, new_signature_type, new_struct, new_var, BasicKind, ObjectArena, PackageArena,
    ScopeArena, VarKind,
};

// ----------------------------------------------------------------------------
// VarKind + embedded

#[test]
fn new_var_defaults_to_package_kind() {
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let v = new_var(&mut o, "x", int);
    match o.get(v) {
        guff_types::ObjectData::Var(var) => {
            assert_eq!(var.kind(), VarKind::Package);
            assert!(!var.embedded());
            assert!(!var.is_field());
            assert!(!var.is_param());
        }
        _ => panic!(),
    }
}

#[test]
fn new_param_sets_param_kind() {
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let p = new_param(&mut o, "i", int);
    match o.get(p) {
        guff_types::ObjectData::Var(v) => {
            assert_eq!(v.kind(), VarKind::Param);
            assert!(v.is_param());
            assert!(!v.is_field());
            assert!(!v.embedded());
        }
        _ => panic!(),
    }
}

#[test]
fn new_field_sets_field_kind_and_embedded() {
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let int = table[BasicKind::Int as usize];
    let f = new_field(&mut o, "x", int, false);
    let e = new_field(&mut o, "T", int, true);
    match o.get(f) {
        guff_types::ObjectData::Var(v) => {
            assert!(v.is_field());
            assert!(!v.embedded());
        }
        _ => panic!(),
    }
    match o.get(e) {
        guff_types::ObjectData::Var(v) => {
            assert!(v.is_field());
            assert!(v.embedded());
            assert!(v.anonymous());
        }
        _ => panic!(),
    }
}

// ----------------------------------------------------------------------------
// same_id

#[test]
fn same_id_unexported_different_packages_are_distinct() {
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let int = table[BasicKind::Int as usize];
    let x = new_var(&mut o, "x", int);
    x.set_pkg(&mut o, pkg_a);

    // Same name + same package → same id.
    assert!(x.same_id(&o, &p_arena, Some(pkg_a), "x", false));
    // Same name + different package → unexported, so NOT same id.
    assert!(!x.same_id(&o, &p_arena, Some(pkg_b), "x", false));
    // Different name → not same id.
    assert!(!x.same_id(&o, &p_arena, Some(pkg_a), "y", false));
    // fold_case: case-insensitive name match → same id regardless of pkg.
    assert!(x.same_id(&o, &p_arena, Some(pkg_b), "X", true));
}

#[test]
fn same_id_exported_ignores_package() {
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let int = table[BasicKind::Int as usize];
    let x = new_var(&mut o, "X", int); // exported
    x.set_pkg(&mut o, pkg_a);

    // Exported names — packages don't matter.
    assert!(x.same_id(&o, &p_arena, Some(pkg_b), "X", false));
    assert!(x.same_id(&o, &p_arena, None, "X", false));
}

// ----------------------------------------------------------------------------
// identical_structs uses sameId for unexported field comparison

#[test]
fn identical_structs_unexported_fields_distinct_across_packages() {
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    // Struct with unexported field "x" in pkg_a.
    let f_a = new_field(&mut o, "x", int, false);
    f_a.set_pkg(&mut o, pkg_a);
    let s_a = new_struct(&mut t, vec![f_a], vec![]);

    // Struct with unexported field "x" in pkg_b.
    let f_b = new_field(&mut o, "x", int, false);
    f_b.set_pkg(&mut o, pkg_b);
    let s_b = new_struct(&mut t, vec![f_b], vec![]);

    // Per Go spec: unexported fields with same name in different packages
    // are different identifiers, so the structs are NOT identical.
    assert!(!identical(&mut t, &o, &p_arena, s_a, s_b));

    // Same package → identical.
    let f_c = new_field(&mut o, "x", int, false);
    f_c.set_pkg(&mut o, pkg_a);
    let s_c = new_struct(&mut t, vec![f_c], vec![]);
    assert!(identical(&mut t, &o, &p_arena, s_a, s_c));
}

#[test]
fn identical_structs_exported_fields_match_across_packages() {
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let f_a = new_field(&mut o, "X", int, false);
    f_a.set_pkg(&mut o, pkg_a);
    let s_a = new_struct(&mut t, vec![f_a], vec![]);

    let f_b = new_field(&mut o, "X", int, false);
    f_b.set_pkg(&mut o, pkg_b);
    let s_b = new_struct(&mut t, vec![f_b], vec![]);

    // Exported field names are equal regardless of package.
    assert!(identical(&mut t, &o, &p_arena, s_a, s_b));
}

#[test]
fn identical_structs_embedded_flag_must_match() {
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let p_arena = PackageArena::new();

    let (_, table) = init_universe();
    let int = table[BasicKind::Int as usize];

    let plain = new_field(&mut o, "X", int, false);
    let embed = new_field(&mut o, "X", int, true);
    let s_plain = new_struct(&mut t, vec![plain], vec![]);
    let s_embed = new_struct(&mut t, vec![embed], vec![]);
    assert!(!identical(&mut t, &o, &p_arena, s_plain, s_embed));
}

// ----------------------------------------------------------------------------
// object::cmp ordering: exported first, then by name, then by package path

#[test]
fn object_cmp_orders_exported_before_unexported() {
    use std::cmp::Ordering;
    let (_, table) = init_universe();
    let mut o = ObjectArena::new();
    let p_arena = PackageArena::new();
    let int = table[BasicKind::Int as usize];

    let big_a = new_var(&mut o, "A", int);
    let small_a = new_var(&mut o, "a", int);
    let big_b = new_var(&mut o, "B", int);

    // Exported "A" before unexported "a".
    assert_eq!(
        guff_types::object_cmp(&o, &p_arena, big_a, small_a),
        Ordering::Less
    );
    // "A" vs "B" — by name.
    assert_eq!(
        guff_types::object_cmp(&o, &p_arena, big_a, big_b),
        Ordering::Less
    );
}

#[test]
fn interface_method_sort_uses_object_cmp() {
    // Two unexported methods with the same name in different packages
    // should both appear (Object.id() differs) and sort by package path.
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_z = new_package(&mut p_arena, &mut s_arena, universe, "z/zz", "z");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "a/aa", "a");

    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m_z = new_func(&mut o, "m", Some(sig));
    m_z.set_pkg(&mut o, pkg_z);
    let m_a = new_func(&mut o, "m", Some(sig));
    m_a.set_pkg(&mut o, pkg_a);

    let iface = new_interface_type(&mut t, vec![m_z, m_a], vec![]);
    assert_eq!(
        guff_types::interface_num_methods(&mut t, &o, &p_arena, iface),
        2
    );
    // Sorted: same name → unexported → by package path. "a/aa" < "z/zz".
    let first = guff_types::interface_method(&mut t, &o, &p_arena, iface, 0);
    let second = guff_types::interface_method(&mut t, &o, &p_arena, iface, 1);
    assert_eq!(first, m_a);
    assert_eq!(second, m_z);
}

#[test]
fn interface_method_dedup_keeps_distinct_packages() {
    // Same unexported method name from two different packages — both
    // should remain (Object.id() distinguishes them).
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg_a = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");
    let pkg_b = new_package(&mut p_arena, &mut s_arena, universe, "pkg/b", "b");

    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m_a = new_func(&mut o, "m", Some(sig));
    m_a.set_pkg(&mut o, pkg_a);
    let m_b = new_func(&mut o, "m", Some(sig));
    m_b.set_pkg(&mut o, pkg_b);

    let iface = new_interface_type(&mut t, vec![m_a, m_b], vec![]);
    // Different packages → not deduped.
    assert_eq!(
        guff_types::interface_num_methods(&mut t, &o, &p_arena, iface),
        2
    );
}

#[test]
fn interface_method_dedup_collapses_same_package() {
    // Same unexported method name + same package → deduped.
    let mut t = init_universe().0;
    let mut o = ObjectArena::new();
    let mut p_arena = PackageArena::new();
    let mut s_arena = ScopeArena::new();
    let universe = new_scope(&mut s_arena, None, None, 0, 0, "universe");
    let pkg = new_package(&mut p_arena, &mut s_arena, universe, "pkg/a", "a");

    let sig = new_signature_type(&mut t, None, &[], &[], None, None, false);
    let m1 = new_func(&mut o, "m", Some(sig));
    m1.set_pkg(&mut o, pkg);
    let m2 = new_func(&mut o, "m", Some(sig));
    m2.set_pkg(&mut o, pkg);

    let iface = new_interface_type(&mut t, vec![m1, m2], vec![]);
    assert_eq!(
        guff_types::interface_num_methods(&mut t, &o, &p_arena, iface),
        1
    );
}
