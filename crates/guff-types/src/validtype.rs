//! Port of `cmd/compile/internal/types2/validtype.go`.
//!
//! [`valid_type`] verifies that a [`Named`](crate::Named) type does not
//! "expand" indefinitely — i.e. doesn't contain itself in its own memory
//! layout. Cycles via alias types (`type A = [10]A`) are caught earlier by
//! the object-declaration cycle detector; this one looks for self-containment
//! through `Struct`/`Array`/`Union`/`Interface` embedding.
//!
//! ## Decoupling from `Checker`
//!
//! Go's `validType0` is a `Checker` method that uses tracing, the package
//! pointer, and reports cycles via `check.cycleError`. None of those are
//! ported yet, so we:
//!
//! - Drop tracing entirely.
//! - Replace `check.pkg` assertions with `debug_assert!`s that fire only if
//!   you have access to the package id (so callers can pass `None` and skip).
//! - Return a [`ValidResult`] enum — `Cycle(path)` lets the caller render
//!   the error in whatever shape it needs once `errors.go` is ported.
//!
//! When the cycle is detected, the origin's `from_rhs` is replaced with
//! `Typ[Invalid]` (matching Go), so subsequent calls on the same type
//! short-circuit and downstream uses see the invalidity. The caller passes
//! the invalid `TypeId` (typically from
//! [`init_universe`](crate::init_universe)'s `Typ[]` table).

use crate::alias::unalias_readonly;
use crate::arena::{ObjectArena, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::named::named_origin;
use crate::predicates::identical;

/// Outcome of [`valid_type`].
#[derive(Debug, Clone)]
pub enum ValidResult {
    /// The type is valid.
    Valid,
    /// A cycle was detected. `path` is the list of `Named` ObjectId entries
    /// from the start of the cycle to its end, in the order Go would pass
    /// to `check.cycleError`. Use for error reporting.
    Cycle { path: Vec<ObjectId> },
}

impl ValidResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, ValidResult::Valid)
    }
}

/// Verify that `typ` (a `Named` type) does not "expand" indefinitely.
///
/// `invalid_typ` should be `Typ[Invalid]` from the predeclared table — used
/// to mark the type's origin invalid when a cycle is detected.
///
/// Equivalent to `Checker.validType`. Returns [`ValidResult::Cycle`] on
/// detection; the caller is responsible for rendering / reporting.
///
/// # Panics
/// Panics if `typ` isn't a `Named`.
pub fn valid_type(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
    invalid_typ: TypeId,
) -> ValidResult {
    assert!(
        matches!(arena.get(typ), TypeData::Named(_)),
        "valid_type entry must be a Named"
    );
    let mut nest: Vec<TypeId> = Vec::new();
    let mut path: Vec<TypeId> = Vec::new();
    match valid_type0(
        arena,
        oarena,
        parena,
        typ,
        &mut nest,
        &mut path,
        invalid_typ,
    ) {
        Step::Ok => ValidResult::Valid,
        Step::Cycle(cycle) => ValidResult::Cycle {
            path: cycle
                .into_iter()
                .map(|t| match arena.get(t) {
                    TypeData::Named(n) => n.obj(),
                    _ => unreachable!("nest holds only Named types"),
                })
                .collect(),
        },
    }
}

/// Internal recursion result. `Cycle(path)` carries the list of `Named`
/// `TypeId`s that form the cycle — we convert to ObjectId at the top level.
#[derive(Debug)]
enum Step {
    Ok,
    Cycle(Vec<TypeId>),
}

fn valid_type0(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    typ: TypeId,
    nest: &mut Vec<TypeId>,
    path: &mut Vec<TypeId>,
    invalid_typ: TypeId,
) -> Step {
    let typ = unalias_readonly(arena, typ);

    use crate::TypeKind as K;
    match typ.kind(arena) {
        K::Array => {
            let elem = match arena.get(typ) {
                TypeData::Array(a) => a.elem(),
                _ => unreachable!(),
            };
            valid_type0(arena, oarena, parena, elem, nest, path, invalid_typ)
        }
        K::Struct => {
            let field_typs: Vec<TypeId> = match arena.get(typ) {
                TypeData::Struct(s) => (0..s.num_fields())
                    .map(|i| {
                        s.field(i)
                            .typ(oarena)
                            .expect("struct field Var must have a type")
                    })
                    .collect(),
                _ => unreachable!(),
            };
            for ft in field_typs {
                if let Step::Cycle(c) =
                    valid_type0(arena, oarena, parena, ft, nest, path, invalid_typ)
                {
                    return Step::Cycle(c);
                }
            }
            Step::Ok
        }
        K::Union => {
            let term_typs: Vec<TypeId> = match arena.get(typ) {
                TypeData::Union(u) => (0..u.len()).map(|i| u.term(i).typ()).collect(),
                _ => unreachable!(),
            };
            for tt in term_typs {
                if let Step::Cycle(c) =
                    valid_type0(arena, oarena, parena, tt, nest, path, invalid_typ)
                {
                    return Step::Cycle(c);
                }
            }
            Step::Ok
        }
        K::Interface => {
            let embeds: Vec<TypeId> = match arena.get(typ) {
                TypeData::Interface(i) => i.embeddeds.clone(),
                _ => unreachable!(),
            };
            for e in embeds {
                if let Step::Cycle(c) =
                    valid_type0(arena, oarena, parena, e, nest, path, invalid_typ)
                {
                    return Step::Cycle(c);
                }
            }
            Step::Ok
        }
        K::Named => {
            // If `typ` is already in nest, we have a cycle.
            //
            // We can't borrow `nest` while calling `identical` (which takes
            // `&mut TypeArena`). Snapshot the candidates first.
            let candidates: Vec<TypeId> = nest.clone();
            let mut found_cycle = false;
            for e in &candidates {
                if identical(arena, oarena, parena, *e, typ) {
                    found_cycle = true;
                    break;
                }
            }
            if found_cycle {
                // Mark t.Origin().fromRHS = Typ[Invalid].
                let origin = named_origin(arena, typ);
                if let TypeData::Named(n) = arena.get_mut(origin) {
                    n.invalidate(invalid_typ);
                }

                // Find the start of the cycle inside `path` and return the
                // slice from there onward.
                let path_snapshot: Vec<TypeId> = path.clone();
                for (start, p) in path_snapshot.iter().enumerate() {
                    if identical(arena, oarena, parena, *p, typ) {
                        return Step::Cycle(path_snapshot[start..].to_vec());
                    }
                }
                panic!("cycle start not found in path");
            }

            // No cycle. Push onto nest+path and recurse into the origin's
            // RHS (the declared underlying, pre-cycle-invalidation).
            let origin = named_origin(arena, typ);
            let rhs = match arena.get(origin) {
                TypeData::Named(n) => n.from_rhs(),
                _ => unreachable!(),
            };
            let rhs = match rhs {
                Some(r) => r,
                None => return Step::Ok, // incomplete Named — treat as valid for now
            };
            nest.push(typ);
            path.push(typ);
            let res = valid_type0(arena, oarena, parena, rhs, nest, path, invalid_typ);
            path.pop();
            nest.pop();
            res
        }
        K::TypeParam => {
            // A type parameter stands for the type (argument) it was
            // instantiated with. If we're inside an instantiated Named, look
            // up the matching type argument and validate that.
            let d = match nest.len().checked_sub(1) {
                Some(d) => d,
                None => return Step::Ok,
            };
            let inst = nest[d];
            let (tparams, targs) = match arena.get(inst) {
                TypeData::Named(n) => {
                    let tp = n
                        .tparams
                        .as_ref()
                        .map(|l| l.list().to_vec())
                        .unwrap_or_default();
                    let ta = n
                        .inst
                        .as_ref()
                        .map(|i| i.targs.list().to_vec())
                        .unwrap_or_default();
                    (tp, ta)
                }
                _ => return Step::Ok,
            };
            for (i, tparam_id) in tparams.iter().enumerate() {
                // Match the type parameter directly via TypeId equality —
                // each TypeParam has a unique allocation.
                if typ == *tparam_id && i < targs.len() {
                    let targ = targs[i];
                    // Validate targ in nest[:d] (excluding the current
                    // instantiation) — restore nest[d] afterwards in case
                    // the recursive call mutated it (Go issue #66323).
                    let saved = nest.split_off(d);
                    let res = valid_type0(arena, oarena, parena, targ, nest, path, invalid_typ);
                    nest.extend(saved);
                    return res;
                }
            }
            Step::Ok
        }
        // Basic, Slice, Pointer, Map, Chan, Tuple, Signature, Alias — these
        // are not self-containing kinds (Pointer breaks the layout chain,
        // Slice/Map/Chan are reference types, etc.) so they're trivially
        // valid in this analysis.
        _ => Step::Ok,
    }
}

/// Helper: returns the list of `TypeName` ObjectId entries for the given
/// `Named` `TypeId`s, in order. Equivalent to `makeObjList` in Go.
pub fn make_obj_list(arena: &TypeArena, tlist: &[TypeId]) -> Vec<ObjectId> {
    tlist
        .iter()
        .map(|&t| match arena.get(t) {
            TypeData::Named(n) => n.obj(),
            _ => panic!("make_obj_list: expected Named"),
        })
        .collect()
}
