//! Port of `cmd/compile/internal/types2/signature.go`.
//!
//! Chunk 2 ports the data structure and the simple accessors. The variadic
//! last-parameter validation (which requires `typeset` iteration and the
//! `Slice` / `isString` predicates) is **deferred** — we only check that
//! variadic signatures have at least one parameter, matching the cheaper
//! invariant from Go.
//!
//! Receiver and result type-parameter lists (`rparams` / `tparams`) are also
//! deferred — `new_signature_type` panics if you pass any, until
//! `TypeParamList` lands in a later chunk.

use serde::{Deserialize, Serialize};

use crate::arena::{ObjectId, TypeArena, TypeData, TypeId};
use crate::typelists::TypeParamList;

/// A (non-builtin) function or method type.
///
/// Equivalent to `types2.Signature`. The receiver is ignored when comparing
/// signatures for identity. `params` and `results` follow Go's `*Tuple`
/// convention where `None` means the empty tuple (matches `nil *Tuple`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signature {
    recv: Option<ObjectId>,
    params: Option<TypeId>,
    results: Option<TypeId>,
    variadic: bool,
    pub(crate) rparams: Option<TypeParamList>, // receiver type parameters
    pub(crate) tparams: Option<TypeParamList>, // function/method type parameters
}

impl Signature {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        self.recv = r.obj_opt(self.recv);
        self.params = r.ty_opt(self.params);
        self.results = r.ty_opt(self.results);
        if let Some(l) = self.rparams.as_mut() {
            l.remap_ids(r);
        }
        if let Some(l) = self.tparams.as_mut() {
            l.remap_ids(r);
        }
    }
}

impl Signature {
    /// Returns the receiver of this signature (if a method), or `None` if a
    /// function. Ignored when comparing signatures for identity.
    pub fn recv(&self) -> Option<ObjectId> {
        self.recv
    }

    /// Parameters as a Tuple `TypeId`, or `None` for an empty list.
    pub fn params(&self) -> Option<TypeId> {
        self.params
    }

    /// Results as a Tuple `TypeId`, or `None` for an empty list.
    pub fn results(&self) -> Option<TypeId> {
        self.results
    }

    /// Reports whether the signature is variadic.
    pub fn variadic(&self) -> bool {
        self.variadic
    }

    /// Type parameters of this signature, or `None` if non-generic.
    pub fn type_params(&self) -> Option<&TypeParamList> {
        self.tparams.as_ref()
    }

    /// Receiver type parameters (for generic methods), or `None`.
    pub fn recv_type_params(&self) -> Option<&TypeParamList> {
        self.rparams.as_ref()
    }
}

/// Construct a new function type.
///
/// Equivalent to `types2.NewSignatureType` minus the chunk-2-deferred parts:
/// - **No type parameters yet.** Pass empty slices for `recv_type_params` and
///   `type_params`; passing non-empty panics.
/// - **No variadic last-parameter type check.** We only enforce that variadic
///   signatures have at least one parameter; the "must be slice or string"
///   check arrives with the typeset/Identical machinery in a later chunk.
///
/// # Panics
/// - If `variadic` is true and `params` is `None`.
///
/// Chunk-9 note: `recv_type_params` and `type_params` are still ignored
/// — the legacy `&[()]` placeholder slots remain so the call sites in
/// chunks 1-8 still compile. To attach type params, use
/// [`signature_set_type_params`] / [`signature_set_recv_type_params`]
/// after construction.
pub fn new_signature_type(
    arena: &mut TypeArena,
    recv: Option<ObjectId>,
    _recv_type_params: &[()],
    _type_params: &[()],
    params: Option<TypeId>,
    results: Option<TypeId>,
    variadic: bool,
) -> TypeId {
    if variadic {
        // Cheap structural check — the full "last param must be a slice or
        // a typeset-compatible type" validation is deferred.
        let n = crate::tuple::tuple_len(arena, params);
        if n == 0 {
            panic!("variadic function must have at least one parameter");
        }
    }
    arena.alloc(TypeData::Signature(Signature {
        recv,
        params,
        results,
        variadic,
        rparams: None,
        tparams: None,
    }))
}

/// Set the function-level type parameters on a Signature.
pub fn signature_set_type_params(arena: &mut TypeArena, id: TypeId, params: TypeParamList) {
    // `remutate` and not `get_mut`: the type parameters are part of the
    // hash-cons key, so setting them here has to move the entry.
    arena.remutate(id, |data| match data {
        TypeData::Signature(s) => s.tparams = Some(params),
        other => panic!(
            "expected Signature, got {:?}",
            std::mem::discriminant(other)
        ),
    });
}

/// Set the receiver type parameters on a Signature (for generic methods).
pub fn signature_set_recv_type_params(arena: &mut TypeArena, id: TypeId, params: TypeParamList) {
    arena.remutate(id, |data| match data {
        TypeData::Signature(s) => s.rparams = Some(params),
        other => panic!(
            "expected Signature, got {:?}",
            std::mem::discriminant(other)
        ),
    });
}

// Free-function accessors over a TypeId. Match the Go method shape:
// `(*Signature).Recv() / Params() / Results() / Variadic()`.

pub fn signature_recv(arena: &TypeArena, id: TypeId) -> Option<ObjectId> {
    as_signature_opt(arena, id)?.recv
}

pub fn signature_params(arena: &TypeArena, id: TypeId) -> Option<TypeId> {
    as_signature_opt(arena, id)?.params
}

pub fn signature_results(arena: &TypeArena, id: TypeId) -> Option<TypeId> {
    as_signature_opt(arena, id)?.results
}

pub fn signature_variadic(arena: &TypeArena, id: TypeId) -> bool {
    as_signature_opt(arena, id)
        .map(|s| s.variadic)
        .unwrap_or(false)
}

/// The function-level type parameters of the signature, or `None` if it is
/// non-generic (or `id` is not a signature).
pub fn signature_type_params(arena: &TypeArena, id: TypeId) -> Option<&TypeParamList> {
    as_signature_opt(arena, id)?.tparams.as_ref()
}

/// The receiver type parameters of the signature (for a generic method like
/// `func (r T[P]) M()`), or `None` if there are none (or `id` is not a signature).
pub fn signature_recv_type_params(arena: &TypeArena, id: TypeId) -> Option<&TypeParamList> {
    as_signature_opt(arena, id)?.rparams.as_ref()
}

fn as_signature_opt(arena: &TypeArena, id: TypeId) -> Option<&Signature> {
    // Named / Alias function types (e.g. `type Handler func()`) store a
    // Signature as their underlying type; callers often pass the Named id.
    let id = id.underlying(arena);
    match arena.get(id) {
        TypeData::Signature(s) => Some(s),
        _ => None,
    }
}
