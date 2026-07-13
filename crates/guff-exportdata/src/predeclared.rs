//! Port of `internal/gcimporter/predeclared.go`.

use guff_types::basic::BasicKind;
use guff_types::universe::Universe;
use guff_types::TypeId;

/// Predeclared types in the same order as Go's `predeclared()` slice (32 entries).
pub fn predeclared_types(universe: &Universe) -> Vec<TypeId> {
    let u = universe;
    vec![
        u.typ[BasicKind::Bool as usize],
        u.typ[BasicKind::Int as usize],
        u.typ[BasicKind::Int8 as usize],
        u.typ[BasicKind::Int16 as usize],
        u.typ[BasicKind::Int32 as usize],
        u.typ[BasicKind::Int64 as usize],
        u.typ[BasicKind::Uint as usize],
        u.typ[BasicKind::Uint8 as usize],
        u.typ[BasicKind::Uint16 as usize],
        u.typ[BasicKind::Uint32 as usize],
        u.typ[BasicKind::Uint64 as usize],
        u.typ[BasicKind::Uintptr as usize],
        u.typ[BasicKind::Float32 as usize],
        u.typ[BasicKind::Float64 as usize],
        u.typ[BasicKind::Complex64 as usize],
        u.typ[BasicKind::Complex128 as usize],
        u.typ[BasicKind::String as usize],
        // byte, rune aliases
        u.byte_typename
            .typ(&u.object_arena)
            .expect("byte typename"),
        u.rune_typename
            .typ(&u.object_arena)
            .expect("rune typename"),
        u.error,
        u.typ[BasicKind::UntypedBool as usize],
        u.typ[BasicKind::UntypedInt as usize],
        u.typ[BasicKind::UntypedRune as usize],
        u.typ[BasicKind::UntypedFloat as usize],
        u.typ[BasicKind::UntypedComplex as usize],
        u.typ[BasicKind::UntypedString as usize],
        u.typ[BasicKind::UntypedNil as usize],
        u.typ[BasicKind::UnsafePointer as usize],
        u.typ[BasicKind::Invalid as usize],
        // internal gc `anyType` — never appears in .a files; use `any`.
        u.any,
        u.comparable,
        u.any,
    ]
}
