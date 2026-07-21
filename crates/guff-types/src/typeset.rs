//! Port of `cmd/compile/internal/types2/typeset.go`.
//!
//! [`TypeSet`] = methods × terms × comparable bit. The intersection of the
//! methods' implied set with the terms-and-comparable set gives the actual
//! "types satisfying this interface" set.
//!
//! [`compute_interface_type_set`] is the workhorse — it walks an
//! `Interface`'s explicit methods + embedded elements (which may themselves
//! be Interfaces, Unions, or arbitrary types) and folds them into a single
//! [`TypeSet`], caching the result on the Interface.
//!
//! Notes:
//! - Term identity uses [`predicates::identical`](crate::predicates::identical)
//!   (D01). `intersect_term_lists` filters non-comparable terms when the
//!   comparable bit is set (D02).
//! - **Version / import-constraint checks**, error reporting, and
//!   `embedPos`-based duplicate-method errors are Checker-side concerns.

use std::collections::{HashMap, HashSet};

use crate::arena::{ObjectArena, ObjectData, ObjectId, PackageArena, TypeArena, TypeData, TypeId};
use crate::predicates::{comparable_type, identical};
use crate::termlist::{self, all_termlist, TermList};
use crate::typeterm::Term;

/// The type set of an interface — the intersection of:
/// 1. the methods (an interface every concrete type must implement), and
/// 2. the `terms ∧ comparable` set (the allowable structural shapes).
///
/// Equivalent to `types2._TypeSet`.
#[derive(Debug, Clone)]
pub struct TypeSet {
    methods: Vec<ObjectId>,
    pub(crate) terms: TermList,
    pub(crate) comparable: bool, // invariant: !comparable || terms.is_all()
}

impl TypeSet {
    /// Relocate ids when merging into a shared seed base (R25).
    pub(crate) fn remap_ids(&mut self, r: &crate::merge::Remapper) {
        for m in &mut self.methods {
            *m = r.obj(*m);
        }
        crate::termlist::remap_ids(&mut self.terms, r);
    }
}

impl TypeSet {
    /// Reports whether this is the empty type set.
    pub fn is_empty(&self) -> bool {
        termlist::is_empty(&self.terms)
    }

    /// Reports whether this is the set of all types (i.e. the empty
    /// interface).
    pub fn is_all(&self) -> bool {
        self.is_method_set() && self.methods.is_empty()
    }

    /// Reports whether this interface is fully described by its method set
    /// (no term-list restrictions and not flagged comparable).
    pub fn is_method_set(&self) -> bool {
        !self.comparable && termlist::is_all(&self.terms)
    }

    pub fn num_methods(&self) -> usize {
        self.methods.len()
    }

    pub fn method(&self, i: usize) -> ObjectId {
        self.methods[i]
    }

    pub fn methods(&self) -> &[ObjectId] {
        &self.methods
    }

    /// Reports whether `f(tilde, typ)` is true for each specific term of
    /// this type set. `typ` is the term's `TypeId`. If the set has no
    /// specific terms, calls `f(false, None)` once.
    ///
    /// Always calls `f` at least once. Stops on first `false`.
    ///
    /// Equivalent to `_TypeSet.is(func(*term) bool)` — but with the
    /// `(tilde, typ)` decomposed because `term` is crate-private.
    pub fn is(&self, mut f: impl FnMut(bool, Option<crate::arena::TypeId>) -> bool) -> bool {
        let has_terms = self.has_terms();
        if !has_terms {
            return f(false, None);
        }
        for slot in self.terms.iter() {
            if let Some(t) = slot.as_ref() {
                if !f(t.tilde, t.typ) {
                    return false;
                }
            }
        }
        true
    }

    /// Find a method on this type set with matching package and name.
    /// Uses the same "different identifier" rule as
    /// [`crate::named::named_lookup_method`] — unexported names from a
    /// different package don't match unless `fold_case` is true.
    ///
    /// Returns `(index, ObjectId)` or `None`.
    ///
    /// Equivalent to `_TypeSet.LookupMethod`.
    pub fn lookup_method(
        &self,
        object_arena: &ObjectArena,
        package_arena: &PackageArena,
        pkg: Option<crate::arena::PackageId>,
        name: &str,
        fold_case: bool,
    ) -> Option<(usize, ObjectId)> {
        if name == "_" {
            return None;
        }
        for (i, m) in self.methods.iter().enumerate() {
            if m.same_id(object_arena, package_arena, pkg, name, fold_case) {
                return Some((i, *m));
            }
        }
        None
    }

    pub fn comparable(&self) -> bool {
        self.comparable
    }

    /// Mutating setter for the `comparable` bit. Used by
    /// [`crate::interface::interface_set_comparable`] for the predeclared
    /// `comparable` interface.
    pub(crate) fn set_comparable(&mut self, c: bool) {
        self.comparable = c;
    }

    /// Number of terms in the type set's term list. Useful for tests that
    /// need to assert structural properties without exposing the internal
    /// `Vec<Option<Term>>` shape.
    pub fn num_terms(&self) -> usize {
        self.terms.len()
    }

    #[allow(dead_code)] // used by TypeSet's Display impl when ported.
    pub(crate) fn has_terms(&self) -> bool {
        !termlist::is_empty(&self.terms) && !termlist::is_all(&self.terms)
    }
}

/// The "all types, no methods" type set. Used as the placeholder for the
/// empty interface and as the safety value for incomplete interfaces.
pub(crate) fn top_typeset() -> TypeSet {
    TypeSet {
        methods: Vec::new(),
        terms: all_termlist(),
        comparable: false,
    }
}

/// Sentinel for unions whose term count overflows. Distinguished by being
/// fully empty — has `is_empty()` true.
fn invalid_typeset() -> TypeSet {
    TypeSet {
        methods: Vec::new(),
        terms: Vec::new(),
        comparable: false,
    }
}

/// Maximum number of terms a Union may yield before we give up.
const MAX_TERM_COUNT: usize = 100;

/// Compute the type set for an Interface and cache it on the interface.
///
/// Equivalent to `computeInterfaceTypeSet`. Calls itself recursively for
/// embedded interfaces; calls [`compute_union_type_set`] for embedded
/// unions.
///
/// If the interface is incomplete (`!complete`), assigns the top type set
/// and returns — same as Go's behaviour to allow follow-on errors to play
/// out without producing a partial cached set.
///
/// # Panics
/// Panics if `id` does not refer to an `Interface`.
pub fn compute_interface_type_set(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    id: TypeId,
) {
    // Fast paths.
    let (methods, embeddeds, complete, already) = match arena.get(id) {
        TypeData::Interface(i) => (
            i.methods.clone(),
            i.embeddeds.clone(),
            i.complete,
            i.tset.is_some(),
        ),
        other => panic!(
            "compute_interface_type_set: expected Interface, got {:?}",
            std::mem::discriminant(other)
        ),
    };
    if already {
        return;
    }
    if !complete {
        // Defer to top type set, but do not memoise — we want to recompute
        // when complete=true is set. Actually Go DOES set tset = topTypeSet
        // in this branch — but then also returns. We mirror exactly.
        if let TypeData::Interface(i) = arena.get_mut(id) {
            // Don't actually cache for incomplete — Go returns &topTypeSet
            // without writing it. Following that for safety.
            let _ = i;
        }
        return;
    }

    // Seed tset to "top" to break recursion if an embedded interface
    // (eventually) references back to this one.
    if let TypeData::Interface(i) = arena.get_mut(id) {
        i.tset = Some(top_typeset());
    }

    // Method dedup uses `Object.Id()` — the package-qualified name for
    // unexported identifiers (so unexported methods from different
    // packages don't collide).
    let mut seen: HashMap<String, ObjectId> = HashMap::new();
    let mut all_methods: Vec<ObjectId> = Vec::new();

    for m in &methods {
        let key = m.id(object_arena, package_arena);
        seen.entry(key).or_insert_with(|| {
            all_methods.push(*m);
            *m
        });
    }

    let mut all_terms: TermList = all_termlist();
    let mut all_comparable = false;
    let mut union_sets: HashMap<TypeId, TypeSet> = HashMap::new();

    for &typ in &embeddeds {
        let u = typ.underlying(arena);
        let (terms, comparable) = match arena.get(u) {
            TypeData::Interface(_) => {
                // Recurse — this populates u's tset cache.
                compute_interface_type_set(arena, object_arena, package_arena, u);
                let ts = match arena.get(u) {
                    TypeData::Interface(i) => i.tset.clone(),
                    _ => unreachable!(),
                };
                match ts {
                    Some(ts) => {
                        // Merge methods from the embedded interface, deduped
                        // by `Object.Id`.
                        for m in &ts.methods {
                            let key = m.id(object_arena, package_arena);
                            seen.entry(key).or_insert_with(|| {
                                all_methods.push(*m);
                                *m
                            });
                        }
                        (ts.terms.clone(), ts.comparable)
                    }
                    None => continue,
                }
            }
            TypeData::Union(_) => {
                let ts =
                    compute_union_type_set(arena, object_arena, package_arena, &mut union_sets, u);
                if ts.is_empty() && ts.terms.is_empty() {
                    // invalid (term overflow); skip
                    continue;
                }
                (ts.terms.clone(), false)
            }
            _ => {
                // Non-interface, non-union embed: a single non-tilde term.
                if !is_valid_type(arena, u) {
                    continue;
                }
                (vec![Some(Term::single(typ))], false)
            }
        };
        let (next_terms, next_comp) = intersect_term_lists(
            arena,
            object_arena,
            package_arena,
            &all_terms,
            all_comparable,
            &terms,
            comparable,
        );
        all_terms = next_terms;
        all_comparable = next_comp;
    }

    sort_methods(&mut all_methods, object_arena, package_arena);

    if let TypeData::Interface(i) = arena.get_mut(id) {
        let ts = i.tset.as_mut().expect("seeded above");
        ts.methods = all_methods;
        ts.terms = all_terms;
        ts.comparable = all_comparable;
    }
}

/// Compute the type set of a Union type, memoising into `union_sets` to
/// avoid recomputing when the same Union appears multiple times.
///
/// Returns an [`invalid_typeset`] if the term count exceeds [`MAX_TERM_COUNT`].
///
/// Equivalent to `computeUnionTypeSet`.
pub fn compute_union_type_set(
    arena: &mut TypeArena,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
    union_sets: &mut HashMap<TypeId, TypeSet>,
    utyp: TypeId,
) -> TypeSet {
    if let Some(cached) = union_sets.get(&utyp) {
        return cached.clone();
    }
    // Seed with empty to break recursion (mirrors Go's `unionSets[utyp] = new(_TypeSet)`).
    union_sets.insert(
        utyp,
        TypeSet {
            methods: Vec::new(),
            terms: Vec::new(),
            comparable: false,
        },
    );

    let terms = match arena.get(utyp) {
        TypeData::Union(u) => u.clone_terms(),
        other => panic!(
            "compute_union_type_set: expected Union, got {:?}",
            std::mem::discriminant(other)
        ),
    };

    let mut all_terms: TermList = Vec::new();
    for ut in &terms {
        let t_typ = ut.typ();
        let u_typ = t_typ.underlying(arena);

        let term_terms: TermList = match arena.get(u_typ) {
            TypeData::Interface(_) => {
                // Treat embedded interface inside a union as its type set's terms.
                compute_interface_type_set(arena, object_arena, package_arena, u_typ);
                match arena.get(u_typ) {
                    TypeData::Interface(i) => match &i.tset {
                        Some(ts) => ts.terms.clone(),
                        None => continue,
                    },
                    _ => unreachable!(),
                }
            }
            _ => {
                if !is_valid_type(arena, u_typ) {
                    continue;
                }
                let slot = if ut.tilde()
                    && !identical(arena, object_arena, package_arena, t_typ, u_typ)
                {
                    None // ∅ — no type has this underlying
                } else if ut.tilde() {
                    Some(Term::tilde(t_typ))
                } else {
                    Some(Term::single(t_typ))
                };
                vec![slot]
            }
        };
        all_terms = termlist::union(arena, object_arena, package_arena, &all_terms, &term_terms);
        if all_terms.len() > MAX_TERM_COUNT {
            let inv = invalid_typeset();
            union_sets.insert(utyp, inv.clone());
            return inv;
        }
    }
    let result = TypeSet {
        methods: Vec::new(),
        terms: all_terms,
        comparable: false,
    };
    union_sets.insert(utyp, result.clone());
    result
}

/// Intersect two term-lists with their respective comparable bits.
///
/// `xcomp` / `ycomp` are only meaningful when their respective lists are
/// `is_all()`; the function preserves that invariant on its result.
///
/// When the result is marked comparable but is not the universe set, only
/// comparable terms are kept (Go's `comparableType` filter — D02).
///
/// Equivalent to `intersectTermLists`.
pub(crate) fn intersect_term_lists(
    arena: &mut TypeArena,
    oarena: &ObjectArena,
    parena: &PackageArena,
    xterms: &TermList,
    xcomp: bool,
    yterms: &TermList,
    ycomp: bool,
) -> (TermList, bool) {
    let mut terms = termlist::intersect(arena, oarena, parena, xterms, yterms);
    let mut comp = xcomp || ycomp;
    if comp && !termlist::is_all(&terms) {
        // Keep only comparable terms (Go: comparableType(t.typ, false, nil)).
        let mut filtered: TermList = Vec::with_capacity(terms.len());
        for t in terms.drain(..) {
            let Some(term) = t else {
                continue;
            };
            let Some(typ) = term.typ else {
                // Universe term shouldn't appear when !is_all; keep defensively.
                filtered.push(Some(term));
                continue;
            };
            let mut seen = HashSet::new();
            if comparable_type(arena, oarena, parena, typ, false, &mut seen).is_ok() {
                filtered.push(Some(term));
            }
        }
        terms = filtered;
        if !termlist::is_all(&terms) {
            comp = false;
        }
    }
    debug_assert!(!comp || termlist::is_all(&terms));
    (terms, comp)
}

// ----------------------------------------------------------------------------
// Helpers used by the type-set machinery.

fn is_valid_type(arena: &TypeArena, id: TypeId) -> bool {
    match arena.get(id) {
        TypeData::Basic(b) => b.kind() != crate::basic::BasicKind::Invalid,
        _ => true,
    }
}

/// Sort methods using Go's canonical `Object.cmp` ordering: exported
/// before unexported, then by name, then (for unexported) by owning
/// package path. Equivalent to `types2.sortMethods`.
fn sort_methods(
    methods: &mut Vec<ObjectId>,
    object_arena: &ObjectArena,
    package_arena: &PackageArena,
) {
    methods.sort_by(|a, b| crate::object::cmp(object_arena, package_arena, *a, *b));
}

// Helper on Union (kept here to avoid bloating union.rs with internal
// algebra concerns).
impl crate::union::Union {
    /// Internal: clone the terms list.
    pub(crate) fn clone_terms(&self) -> Vec<crate::union::Term> {
        (0..self.len()).map(|i| self.term(i).clone()).collect()
    }
}

// `ObjectData` is no longer referenced by sort_methods, but keep the
// import alive for the `is_valid_type` matchers above. (Silences the
// unused-import warning that would otherwise appear.)
#[allow(dead_code)]
fn _objectdata_used(_: &ObjectData) {}
