//! Chunk-6 tests: predeclared universe is wired up correctly.

use guff_constant::{bool_val, int64_val};
use guff_types::{
    init_universe_full, interface_is_comparable, interface_method, interface_num_methods,
    named_underlying, signature_params, signature_results, signature_variadic, tuple_at, tuple_len,
    identical, unalias, BasicKind, BuiltinId, ObjectData, TypeData, TypeKind,
};

#[test]
fn lookup_returns_predeclared_basics() {
    let u = init_universe_full();
    let int = u.lookup("int").expect("int present");
    assert_eq!(int.name(&u.object_arena), "int");
    let int_typ = int.typ(&u.object_arena).unwrap();
    assert_eq!(int_typ, u.typ[BasicKind::Int as usize]);
}

/// `byte` and `rune` are their **own** Basic values, not the `uint8` / `int32`
/// entries — go/types' `aliases` array. They carry the same kinds, so
/// `identical` says yes and nothing about assignability or conversion changes;
/// what the separate values keep is the *name*, and the name is what a
/// diagnostic prints. gosec's G115 says `rune -> byte` for a conversion the
/// source wrote that way, and `int32 -> uint8` if the two are collapsed.
#[test]
fn byte_and_rune_are_distinct_basics_identical_to_uint8_and_int32() {
    let mut u = init_universe_full();
    let byte_typ = u.lookup("byte").unwrap().typ(&u.object_arena).unwrap();
    let rune_typ = u.lookup("rune").unwrap().typ(&u.object_arena).unwrap();
    let uint8_typ = u.typ[BasicKind::Uint8 as usize];
    let int32_typ = u.typ[BasicKind::Int32 as usize];

    assert_ne!(byte_typ, uint8_typ, "byte must not be the uint8 entry");
    assert_ne!(rune_typ, int32_typ, "rune must not be the int32 entry");

    for (alias, canonical, name, kind) in [
        (byte_typ, uint8_typ, "byte", BasicKind::Uint8),
        (rune_typ, int32_typ, "rune", BasicKind::Int32),
    ] {
        let TypeData::Basic(b) = u.type_arena.get(alias) else {
            panic!("{name} is not a Basic");
        };
        assert_eq!(b.name(), name);
        assert_eq!(b.kind(), kind);
        assert!(
            identical(
                &mut u.type_arena,
                &u.object_arena,
                &u.package_arena,
                alias,
                canonical
            ),
            "{name} must be identical to its canonical spelling"
        );
    }
}

#[test]
fn any_aliases_an_empty_interface() {
    let mut u = init_universe_full();
    assert_eq!(u.any.kind(&u.type_arena), TypeKind::Alias);
    // unalias should land on the empty interface.
    let resolved = unalias(&mut u.type_arena, u.any);
    assert_eq!(resolved.kind(&u.type_arena), TypeKind::Interface);
    // Empty interface has 0 methods.
    assert_eq!(
        interface_num_methods(
            &mut u.type_arena,
            &u.object_arena,
            &u.package_arena,
            resolved
        ),
        0
    );
}

#[test]
fn error_named_has_error_string_method() {
    let mut u = init_universe_full();
    assert_eq!(u.error.kind(&u.type_arena), TypeKind::Named);

    let underlying = named_underlying(&u.type_arena, u.error).expect("underlying set");
    assert_eq!(underlying.kind(&u.type_arena), TypeKind::Interface);

    assert_eq!(
        interface_num_methods(
            &mut u.type_arena,
            &u.object_arena,
            &u.package_arena,
            underlying
        ),
        1
    );
    let method = interface_method(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        underlying,
        0,
    );
    assert_eq!(method.name(&u.object_arena), "Error");

    // Method signature: func() string.
    let sig_id = method.typ(&u.object_arena).expect("method has sig");
    assert!(signature_params(&u.type_arena, sig_id).is_none());
    assert!(!signature_variadic(&u.type_arena, sig_id));

    let results = signature_results(&u.type_arena, sig_id).expect("one result");
    assert_eq!(tuple_len(&u.type_arena, Some(results)), 1);
    let res_var = tuple_at(&u.type_arena, results, 0);
    assert_eq!(
        res_var.typ(&u.object_arena),
        Some(u.typ[BasicKind::String as usize])
    );
}

#[test]
fn comparable_named_underlying_has_comparable_bit_set() {
    let mut u = init_universe_full();
    assert_eq!(u.comparable.kind(&u.type_arena), TypeKind::Named);
    let underlying = named_underlying(&u.type_arena, u.comparable).unwrap();
    assert_eq!(underlying.kind(&u.type_arena), TypeKind::Interface);
    assert!(interface_is_comparable(
        &mut u.type_arena,
        &u.object_arena,
        &u.package_arena,
        underlying
    ));
}

#[test]
fn predeclared_consts_have_expected_values() {
    let u = init_universe_full();

    let true_obj = u.lookup("true").unwrap();
    let false_obj = u.lookup("false").unwrap();
    let iota_obj = u.lookup("iota").unwrap();

    match u.object_arena.get(true_obj) {
        ObjectData::Const(c) => {
            assert_eq!(c.name(), "true");
            assert_eq!(bool_val(c.val()), true);
            assert_eq!(c.typ(), u.typ[BasicKind::UntypedBool as usize]);
        }
        _ => panic!("true should be a Const"),
    }
    match u.object_arena.get(false_obj) {
        ObjectData::Const(c) => assert_eq!(bool_val(c.val()), false),
        _ => panic!("false should be a Const"),
    }
    match u.object_arena.get(iota_obj) {
        ObjectData::Const(c) => {
            let (v, exact) = int64_val(c.val());
            assert_eq!(v, 0);
            assert!(exact);
            assert_eq!(c.typ(), u.typ[BasicKind::UntypedInt as usize]);
        }
        _ => panic!("iota should be a Const"),
    }
}

#[test]
fn nil_is_typed_untyped_nil() {
    let u = init_universe_full();
    let nil = u.lookup("nil").unwrap();
    assert_eq!(nil.name(&u.object_arena), "nil");
    assert_eq!(
        nil.typ(&u.object_arena),
        Some(u.typ[BasicKind::UntypedNil as usize])
    );
    assert!(matches!(u.object_arena.get(nil), ObjectData::Nil(_)));
}

#[test]
fn builtins_are_registered() {
    let u = init_universe_full();

    // Spot-check a handful — both universe-scope and unsafe-package
    // builtins (we don't separate the packages in chunk 6).
    let len_obj = u.lookup("len").expect("len builtin");
    assert_eq!(len_obj.name(&u.object_arena), "len");
    match u.object_arena.get(len_obj) {
        ObjectData::Builtin(b) => assert_eq!(b.id(), BuiltinId::Len),
        _ => panic!("len should be a Builtin"),
    }
    assert_eq!(u.builtins[&BuiltinId::Len], len_obj);

    let append_obj = u.lookup("append").unwrap();
    match u.object_arena.get(append_obj) {
        ObjectData::Builtin(b) => assert_eq!(b.id(), BuiltinId::Append),
        _ => panic!(),
    }

    // Unsafe-package builtin (capital-A name "Add").
    let add_obj = u.lookup("Add").unwrap();
    match u.object_arena.get(add_obj) {
        ObjectData::Builtin(b) => assert_eq!(b.id(), BuiltinId::Add),
        _ => panic!(),
    }

    // Builtin typ is Invalid Basic.
    let len_typ = len_obj.typ(&u.object_arena).unwrap();
    match u.type_arena.get(len_typ) {
        TypeData::Basic(b) => assert_eq!(b.kind(), BasicKind::Invalid),
        _ => panic!("Builtin typ should be Invalid Basic"),
    }

    // assert / trace are NOT pre-registered (test-only).
    assert!(u.lookup("assert").is_none());
    assert!(u.lookup("trace").is_none());
}

#[test]
fn universe_lookup_handles_unknown_names() {
    let u = init_universe_full();
    assert!(u.lookup("not_a_real_predeclared").is_none());
    assert!(u.lookup("").is_none());
}
