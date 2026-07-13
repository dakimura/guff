//! Port of `cmd/compile/internal/types2/selection.go`.
//!
//! A [`Selection`] describes a selector expression `x.f` (excluding qualified
//! identifiers). It is produced by the lookup machinery (see `lookup.go`,
//! later chunk) and stored on the Checker's `Info.Selections` map.
//!
//! In Go, `Selection.Type()` synthesises a fresh `Signature` for
//! [`SelectionKind::MethodVal`] and [`SelectionKind::MethodExpr`] cases —
//! patching the receiver or promoting it to a first parameter respectively.
//! Our port matches that, allocating the new Var(s) / Tuple / Signature in
//! the arenas the caller provides.
//!
//! The string formatter [`selection_string`] is currently a stub: until
//! `typestring.go` lands (Tier 5), it just names the receiver / type by their
//! object name. The output shape (e.g. `"field (T) f int"`) matches Go's
//! prefix conventions so tests can lock the structure now and let
//! `typestring.go` fill in real type rendering later.

use crate::arena::{ObjectArena, ObjectData, ObjectId, TypeArena, TypeData, TypeId};
use crate::object::var::{new_param, VarKind};
use crate::signature::new_signature_type;
use crate::tuple::new_tuple;

/// Kind of selector expression `x.f`.
///
/// Equivalent to `types2.SelectionKind`. Discriminants match Go's `iota`
/// ordering for cross-tool compatibility.
#[repr(u8)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub enum SelectionKind {
    /// `x.f` is a struct field selector.
    FieldVal = 0,
    /// `x.f` is a method selector.
    MethodVal = 1,
    /// `x.f` is a method expression `T.f`.
    MethodExpr = 2,
}

/// A selector expression `x.f`.
///
/// Equivalent to `types2.Selection`.
#[derive(Debug, Clone)]
pub struct Selection {
    kind: SelectionKind,
    recv: TypeId,    // type of x
    obj: ObjectId,   // object denoted by x.f (Var for FieldVal, Func otherwise)
    index: Vec<i32>, // path from x to x.f
    indirect: bool,  // any pointer indirection on the path
}

impl Selection {
    /// Construct a Selection. Mirrors Go's struct-literal usage from
    /// `lookup.go` (which is unported as of chunk 10 — this constructor is
    /// the public seam tests can use).
    pub fn new(
        kind: SelectionKind,
        recv: TypeId,
        obj: ObjectId,
        index: Vec<i32>,
        indirect: bool,
    ) -> Self {
        Self {
            kind,
            recv,
            obj,
            index,
            indirect,
        }
    }

    /// The selection kind.
    pub fn kind(&self) -> SelectionKind {
        self.kind
    }

    /// Type of `x` in `x.f`.
    pub fn recv(&self) -> TypeId {
        self.recv
    }

    /// The object denoted by `x.f` — a `Var` for [`SelectionKind::FieldVal`],
    /// otherwise a `Func`.
    pub fn obj(&self) -> ObjectId {
        self.obj
    }

    /// Path from `x` to `f`. The last entry is the field/method index in the
    /// declaring type; earlier entries are indices of embedded fields
    /// traversed implicitly, starting at embedding depth 0.
    pub fn index(&self) -> &[i32] {
        &self.index
    }

    /// Reports whether any pointer indirection was required to get from `x`
    /// to `f` in `x.f`.
    ///
    /// Note: matches Go's `Indirect`, which spuriously returns `true` for
    /// some `MethodVal` selections (go issue #8353); we preserve that
    /// behaviour for fidelity.
    pub fn indirect(&self) -> bool {
        self.indirect
    }
}

/// Returns the type of `x.f`, which may differ from the type of `f`. See the
/// `Selection` doc comment for the rules.
///
/// - [`SelectionKind::FieldVal`] — returns the field's declared type.
/// - [`SelectionKind::MethodVal`] — returns a fresh Signature with the
///   receiver's type replaced by `s.recv`.
/// - [`SelectionKind::MethodExpr`] — returns a fresh Signature without a
///   receiver, with the original receiver type promoted to the new first
///   parameter (same name).
///
/// New Vars / Tuples / Signatures are allocated in the arenas provided —
/// matching Go's "build a fresh `*Signature` by copying" but without the
/// hidden global heap.
///
/// Equivalent to `Selection.Type`.
pub fn selection_type(
    type_arena: &mut TypeArena,
    object_arena: &mut ObjectArena,
    s: &Selection,
) -> TypeId {
    match s.kind {
        SelectionKind::FieldVal => s.obj.typ(object_arena).expect("Var field must have a type"),
        SelectionKind::MethodVal => {
            let (orig_sig_id, orig_recv_obj) = method_signature(object_arena, type_arena, s.obj);
            let (orig_params, orig_results, orig_variadic) = match type_arena.get(orig_sig_id) {
                TypeData::Signature(sig) => (sig.params(), sig.results(), sig.variadic()),
                _ => unreachable!(),
            };
            // Clone the receiver Var with `typ = s.recv`.
            let recv_name = orig_recv_obj.name(object_arena).to_string();
            let new_recv = new_param(object_arena, recv_name, s.recv);
            // Promote to Recv kind (matches Go's recv layout, even though
            // the receiver isn't currently in a Tuple).
            if let ObjectData::Var(v) = object_arena.get_mut(new_recv) {
                v.set_kind(VarKind::Recv);
            }
            new_signature_type(
                type_arena,
                Some(new_recv),
                &[],
                &[],
                orig_params,
                orig_results,
                orig_variadic,
            )
        }
        SelectionKind::MethodExpr => {
            let (orig_sig_id, orig_recv_obj) = method_signature(object_arena, type_arena, s.obj);
            let (orig_params, orig_results, orig_variadic) = match type_arena.get(orig_sig_id) {
                TypeData::Signature(sig) => (sig.params(), sig.results(), sig.variadic()),
                _ => unreachable!(),
            };
            // Snapshot original param object ids before we mutate the arena.
            let mut new_params: Vec<ObjectId> = Vec::new();
            // Promote receiver to first param with type s.recv.
            let recv_name = orig_recv_obj.name(object_arena).to_string();
            let arg0 = new_param(object_arena, recv_name, s.recv);
            new_params.push(arg0);
            if let Some(p_id) = orig_params {
                let n = match type_arena.get(p_id) {
                    TypeData::Tuple(t) => t.len(),
                    _ => unreachable!(),
                };
                for i in 0..n {
                    let var = match type_arena.get(p_id) {
                        TypeData::Tuple(t) => t.at(i),
                        _ => unreachable!(),
                    };
                    new_params.push(var);
                }
            }
            let new_params_tup = new_tuple(type_arena, &new_params);
            new_signature_type(
                type_arena,
                None,
                &[],
                &[],
                new_params_tup,
                orig_results,
                orig_variadic,
            )
        }
    }
}

/// Read the underlying Signature TypeId + receiver ObjectId for a method
/// object. Panics if `obj` isn't a `Func` with a Signature type and a
/// non-`None` receiver.
fn method_signature(
    object_arena: &ObjectArena,
    type_arena: &TypeArena,
    obj: ObjectId,
) -> (TypeId, ObjectId) {
    let sig_id = obj.typ(object_arena).expect("method Func must have a type");
    let recv = match type_arena.get(sig_id) {
        TypeData::Signature(sig) => sig
            .recv()
            .expect("MethodVal/Expr selection requires a receiver"),
        _ => panic!("method selection on non-Signature type"),
    };
    (sig_id, recv)
}

/// Stub renderer matching the prefix conventions of Go's `SelectionString`.
/// Renders a selection as `"<kind> (<recv-type>) <name>"`, using
/// [`crate::typestring::type_string`] for the receiver type (chunk 17 wired
/// this in, replacing the earlier `type#{id}` placeholder).
///
/// The trailing field-type / method-signature that Go's `SelectionString`
/// also appends is still omitted: it needs [`selection_type`], which
/// *synthesises* a fresh signature in the arena (requiring `&mut TypeArena`)
/// — a follow-up once the Checker owns the arena mutably.
///
/// Equivalent to `SelectionString` (header + receiver type only).
pub fn selection_string(
    type_arena: &crate::arena::TypeArena,
    oarena: &ObjectArena,
    parena: &crate::arena::PackageArena,
    s: &Selection,
) -> String {
    let prefix = match s.kind {
        SelectionKind::FieldVal => "field",
        SelectionKind::MethodVal => "method",
        SelectionKind::MethodExpr => "method expr",
    };
    format!(
        "{} ({}) {}",
        prefix,
        crate::typestring::type_string(type_arena, oarena, parena, s.recv, None),
        s.obj.name(oarena)
    )
}
