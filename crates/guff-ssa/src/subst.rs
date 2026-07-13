//! Type substitution for generic instantiation.
//!
//! Port of go/ssa's `subst.go` (`subster`). A [`Subster`] maps a set of type
//! parameters to type-parameter-free replacement types and rewrites a type by
//! replacing each occurrence of a mapped type parameter, rebuilding only the
//! composite types whose contents actually changed.
//!
//! This is the SSA-specific substituter used when compiling a generic function
//! instantiation `F[targs]`: `((λm. E) N)`, i.e. `E[m := N]`.
//!
//! MILESTONE E scope: chunk E01 covered the substituter skeleton, the result
//! cache, and the leaf / simple composite cases (`TypeParam`, `Basic`, `Array`,
//! `Slice`, `Pointer`). Chunk E02 added the pure-`TypeId` composite kinds
//! (`Map`, `Chan`, `Union`). Chunk E03 adds the object-bearing kinds (`Tuple`,
//! `Struct`, `Signature`), which reach through `ObjectId` variables to their
//! types; substitution therefore threads a `&mut ObjectArena` alongside the
//! `&mut TypeArena` so it can read a variable's type and allocate fresh
//! substituted `Var`s. The instantiation-bearing kinds (`Named`, `Interface`,
//! `Alias`) are handled from chunk E04 for the common "declared outside the
//! origin function" case (a package-level generic type instance, e.g. `List[T]`
//! with `T := int` → `List[int]`): substitute the type arguments and
//! re-instantiate the origin. Chunk E05 adds `Interface` (substituting each
//! method's receiver-less signature and each embedded type, cycle-breaking on
//! the receiver). Chunk E06 completes `subst.rs` with the local-type case — a
//! `Named`/`Alias` declared *within* the generic origin function, which Go
//! treats as unique per instantiation: a fresh copy of the type (and of its own
//! type parameters and their constraints) is created per `F[targs]`, with the
//! partially-built copy cached before its underlying type is substituted so that
//! recursive local types (`type X struct{ next *X }`) terminate.

use std::collections::HashMap;
use guff_types::{
    ChanDir, Context, ObjectArena, ObjectData, ObjectId, PackageId, TypeArena, TypeData, TypeId,
};

/// A type substitution operation from a fixed set of type parameters to their
/// replacement types. (Go: `subster`)
///
/// An empty substitution (no replacements) acts as the identity function, so a
/// parameterized and a non-parameterized function can be compiled uniformly.
pub struct Subster {
    /// type parameter `TypeId` → replacement `TypeId`. Replacement types must
    /// themselves be free of the substituted type parameters. (Go:
    /// `subster.replacements`)
    replacements: HashMap<TypeId, TypeId>,
    /// memoized results of [`Subster::typ`], keyed by input type. Preserves
    /// sharing and terminates substitution over cyclic types. (Go:
    /// `subster.cache`)
    cache: HashMap<TypeId, TypeId>,
    /// The origin function's scope bounds `[pos, end)`, against which
    /// [`Subster::declared_within`] tests a candidate type's declaration
    /// position. `None` means no origin is in scope (the common package-level
    /// instantiation), so every `Named`/`Alias` is treated as declared outside
    /// the origin. (Go: `subster.origin`, a `*types.Func`, consulted through
    /// `fn.Scope().Contains(pos)`; the scope range is precomputed here.)
    origin_scope: Option<(u32, u32)>,
    /// Instantiation cache, so repeated `orig[targs...]` yield the same type.
    /// Go shares one `*types.Context` across substers; here each `Subster` owns
    /// its own (a functional cache, not observable in results). (Go:
    /// `subster.ctxt`.)
    ctxt: Context,
    /// Canonicalizes instantiations produced within this substitution so the
    /// same `(orig, targs)` maps to one `TypeId`. (Go: `subster.uniqueness`.)
    uniqueness: HashMap<TypeId, TypeId>,
}

impl Subster {
    /// Creates a substituter replacing `tparams[i]` with `targs[i]`. The slices
    /// must have equal length; `targs` must not contain any of `tparams`.
    /// (Go: `makeSubster`, restricted to a single (tparams, targs) pair.)
    ///
    /// The origin function is left unset (`None`); use [`Subster::in_origin`] to
    /// substitute within a generic function that declares local types.
    pub fn new(tparams: &[TypeId], targs: &[TypeId]) -> Self {
        Self::with_recv_and_type(&[], &[], tparams, targs)
    }

    /// Creates a substituter replacing `rtparams[i]` with `rtargs[i]` and
    /// `tparams[i]` with `targs[i]`. (Go: `makeSubster`.)
    pub fn with_recv_and_type(
        rtparams: &[TypeId],
        rtargs: &[TypeId],
        tparams: &[TypeId],
        targs: &[TypeId],
    ) -> Self {
        let got = rtargs.len() + targs.len();
        let want = rtparams.len() + tparams.len();
        assert_eq!(
            got, want,
            "makeSubster argument count must match: got {}; want {}",
            got, want
        );
        let mut replacements = HashMap::with_capacity(want);
        for (&tp, &ta) in rtparams.iter().zip(rtargs.iter()) {
            replacements.insert(tp, ta);
        }
        for (&tp, &ta) in tparams.iter().zip(targs.iter()) {
            replacements.insert(tp, ta);
        }
        Self {
            replacements,
            cache: HashMap::new(),
            origin_scope: None,
            ctxt: Context::new(),
            uniqueness: HashMap::new(),
        }
    }

    /// Sets the origin function whose local type declarations are unique per
    /// instantiation, given that function's scope bounds `[scope_pos, scope_end)`.
    /// A `Named`/`Alias` whose declaring object's position falls within this
    /// range is treated as a local type and copied fresh per instantiation.
    /// (Go: `makeSubster` sets `origin = fn.Origin()`, later consulted via
    /// `fn.Scope().Contains(pos)`.)
    pub fn in_origin(mut self, scope_pos: u32, scope_end: u32) -> Self {
        self.origin_scope = Some((scope_pos, scope_end));
        self
    }

    /// Returns `true` if this substitution has no replacements, in which case
    /// [`Subster::typ`] is the identity. (Go: a nil `*subster`.)
    pub fn is_identity(&self) -> bool {
        self.replacements.is_empty()
    }

    /// Substitutes within `t`, returning the rewritten type. When no substituted
    /// type parameter occurs in `t`, the same `TypeId` is returned unchanged
    /// (preserving type identity, on which composite reconstruction depends).
    /// (Go: `(*subster).typ`)
    pub fn typ(&mut self, arena: &mut TypeArena, oarena: &mut ObjectArena, t: TypeId) -> TypeId {
        if let Some(&r) = self.cache.get(&t) {
            return r;
        }
        let res = self.typ_uncached(arena, oarena, t);
        self.cache.insert(t, res);
        res
    }

    fn typ_uncached(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        t: TypeId,
    ) -> TypeId {
        // Read the kind and any child types up front so the immutable arena
        // borrow is released before we recurse / allocate new types.
        enum Kind {
            Param,
            Leaf,
            Array(TypeId, i64),
            Slice(TypeId),
            Pointer(TypeId),
            Map(TypeId, TypeId),
            Chan(ChanDir, TypeId),
            /// Each term's `(tilde, type)`, in order.
            Union(Vec<(bool, TypeId)>),
            /// The tuple's variables, in order.
            Tuple(Vec<ObjectId>),
            /// The struct's fields and their (parallel) tags.
            Struct(Vec<ObjectId>, Vec<String>),
            /// `recv`, `params` (a Tuple type or `None`), `results`, `variadic`,
            /// and whether the signature is generic (has type parameters).
            Signature {
                recv: Option<ObjectId>,
                params: Option<TypeId>,
                results: Option<TypeId>,
                variadic: bool,
                generic: bool,
            },
            Named,
            Alias,
            Interface,
        }
        let kind = match arena.get(t) {
            TypeData::TypeParam(_) => Kind::Param,
            TypeData::Basic(_) => Kind::Leaf,
            TypeData::Array(a) => Kind::Array(a.elem(), a.len()),
            TypeData::Slice(s) => Kind::Slice(s.elem()),
            TypeData::Pointer(p) => Kind::Pointer(p.elem()),
            TypeData::Map(m) => Kind::Map(m.key(), m.elem()),
            TypeData::Chan(c) => Kind::Chan(c.dir(), c.elem()),
            TypeData::Union(u) => Kind::Union(
                (0..u.len())
                    .map(|i| {
                        let term = u.term(i);
                        (term.tilde(), term.typ())
                    })
                    .collect(),
            ),
            TypeData::Tuple(tup) => {
                Kind::Tuple((0..tup.len()).map(|i| tup.at(i)).collect())
            }
            TypeData::Struct(s) => {
                let n = s.num_fields();
                Kind::Struct(
                    (0..n).map(|i| s.field(i)).collect(),
                    (0..n).map(|i| s.tag(i).to_string()).collect(),
                )
            }
            TypeData::Signature(s) => Kind::Signature {
                recv: s.recv(),
                params: s.params(),
                results: s.results(),
                variadic: s.variadic(),
                generic: s.type_params().is_some_and(|tp| !tp.is_empty()),
            },
            TypeData::Named(_) => Kind::Named,
            TypeData::Alias(_) => Kind::Alias,
            TypeData::Interface(_) => Kind::Interface,
        };

        match kind {
            // A substituted type parameter maps to its replacement; an
            // unmapped one is preserved.
            Kind::Param => self.replacements.get(&t).copied().unwrap_or(t),
            // Basic types are type-parameter-free.
            Kind::Leaf => t,
            Kind::Array(elem, len) => {
                let r = self.typ(arena, oarena, elem);
                if r != elem {
                    guff_types::array::new_array(arena, r, len)
                } else {
                    t
                }
            }
            Kind::Slice(elem) => {
                let r = self.typ(arena, oarena, elem);
                if r != elem {
                    guff_types::slice::new_slice(arena, r)
                } else {
                    t
                }
            }
            Kind::Pointer(elem) => {
                let r = self.typ(arena, oarena, elem);
                if r != elem {
                    guff_types::pointer::new_pointer(arena, r)
                } else {
                    t
                }
            }
            Kind::Map(key, elem) => {
                let rk = self.typ(arena, oarena, key);
                let re = self.typ(arena, oarena, elem);
                if rk != key || re != elem {
                    guff_types::map::new_map(arena, rk, re)
                } else {
                    t
                }
            }
            Kind::Chan(dir, elem) => {
                let r = self.typ(arena, oarena, elem);
                if r != elem {
                    guff_types::chan::new_chan(arena, dir, r)
                } else {
                    t
                }
            }
            Kind::Union(terms) => {
                // Substitute each term's type; rebuild the union only if some
                // term type actually changed, preserving each term's `~`.
                let mut changed = false;
                let mut out = Vec::with_capacity(terms.len());
                for (tilde, ty) in terms {
                    let r = self.typ(arena, oarena, ty);
                    changed |= r != ty;
                    out.push(guff_types::union::new_term(tilde, r));
                }
                if changed {
                    guff_types::union::new_union(arena, out)
                } else {
                    t
                }
            }
            Kind::Tuple(vars) => match self.varlist(arena, oarena, &vars) {
                // A non-empty tuple stays non-empty after substitution.
                Some(new_vars) => guff_types::tuple::new_tuple(arena, &new_vars)
                    .expect("substituted non-empty tuple is non-empty"),
                None => t,
            },
            Kind::Struct(fields, tags) => match self.varlist(arena, oarena, &fields) {
                Some(new_fields) => guff_types::r#struct::new_struct(arena, new_fields, tags),
                None => t,
            },
            Kind::Signature {
                recv,
                params,
                results,
                variadic,
                generic,
            } => {
                // Matches go/ssa: substituting generic signatures is unsupported
                // (would require instantiating the signature with its targs).
                assert!(
                    !generic,
                    "substituting generic function signatures is unsupported"
                );
                let new_recv = recv.map(|r| self.var_(arena, oarena, r));
                // Substituting a Tuple type reuses the Tuple case above.
                let new_params = params.map(|p| self.typ(arena, oarena, p));
                let new_results = results.map(|r| self.typ(arena, oarena, r));
                if new_recv != recv || new_params != params || new_results != results {
                    guff_types::signature::new_signature_type(
                        arena,
                        new_recv,
                        &[],
                        &[],
                        new_params,
                        new_results,
                        variadic,
                    )
                } else {
                    t
                }
            }
            Kind::Named => self.named(arena, oarena, t),
            Kind::Alias => self.alias(arena, oarena, t),
            Kind::Interface => self.interface_(arena, oarena, t),
        }
    }

    /// Substitutes within an `Interface` type. (Go: `(*subster).interface_`)
    ///
    /// Each explicit method's signature is stripped of its receiver before
    /// substituting — the receiver points back at the interface, so recursing
    /// through it would cycle. The interface is rebuilt only if a method
    /// signature or an embedded type actually changed.
    fn interface_(&mut self, arena: &mut TypeArena, oarena: &mut ObjectArena, t: TypeId) -> TypeId {
        use guff_types::{
            interface_embedded_type, interface_explicit_method, interface_num_embeddeds,
            interface_num_explicit_methods,
        };

        // Read the method (name, signature) pairs and embedded types up front,
        // releasing the arena borrows before we recurse / allocate.
        let n_methods = interface_num_explicit_methods(arena, t);
        let methods_meta: Vec<(String, TypeId)> = (0..n_methods)
            .map(|i| {
                let f = interface_explicit_method(arena, t, i);
                match oarena.get(f) {
                    ObjectData::Func(func) => (
                        func.name().to_string(),
                        func.typ().expect("interface method has a signature"),
                    ),
                    other => panic!(
                        "interface method must be a Func, got {:?}",
                        std::mem::discriminant(other)
                    ),
                }
            })
            .collect();
        let embeds_orig: Vec<TypeId> = (0..interface_num_embeddeds(arena, t))
            .map(|i| interface_embedded_type(arena, t, i))
            .collect();

        // Substitute each method's receiver-less signature.
        let mut methods_changed = false;
        let mut subst_sigs = Vec::with_capacity(methods_meta.len());
        for (_, sig) in &methods_meta {
            let norecv = Self::change_recv_nil(arena, *sig);
            let subst_sig = self.typ(arena, oarena, norecv);
            methods_changed |= subst_sig != norecv;
            subst_sigs.push(subst_sig);
        }

        // Substitute each embedded type.
        let mut embeds_changed = false;
        let mut new_embeds = Vec::with_capacity(embeds_orig.len());
        for &e in &embeds_orig {
            let r = self.typ(arena, oarena, e);
            embeds_changed |= r != e;
            new_embeds.push(r);
        }

        if !methods_changed && !embeds_changed {
            return t;
        }

        // Rebuild. Interface methods carry no receiver — NewInterfaceType fills
        // them in — so the fresh Funcs wrap the receiver-less substituted sigs.
        let new_methods = methods_meta
            .into_iter()
            .zip(subst_sigs)
            .map(|((name, _), sig)| guff_types::new_func(oarena, name, Some(sig)))
            .collect();
        guff_types::new_interface_type(arena, new_methods, new_embeds)
    }

    /// Returns a copy of signature `sig` with its receiver removed. Interface
    /// method signatures reference the interface through their receiver; dropping
    /// it breaks the cycle before substitution. (Go: `changeRecv(sig, nil)`)
    fn change_recv_nil(arena: &mut TypeArena, sig: TypeId) -> TypeId {
        let params = guff_types::signature_params(arena, sig);
        let results = guff_types::signature_results(arena, sig);
        let variadic = guff_types::signature_variadic(arena, sig);
        guff_types::new_signature_type(arena, None, &[], &[], params, results, variadic)
    }

    /// Substitutes within a `Named` type. (Go: `(*subster).named`)
    ///
    /// For a type declared outside the origin function — the common case — an
    /// un-instantiated named type is type-parameter-free and returned as-is,
    /// while an instance `orig[targs...]` has its type arguments substituted and
    /// is re-instantiated. A type declared *within* the origin is a local type
    /// unique per instantiation: without type arguments it is copied fresh (see
    /// [`Subster::fresh_local_named`]); with type arguments the copy is reduced
    /// to substituting the origin and re-instantiating with the substituted args.
    fn named(&mut self, arena: &mut TypeArena, oarena: &mut ObjectArena, t: TypeId) -> TypeId {
        // Read the shape up front, releasing the arena borrow before recursing.
        let obj = guff_types::named_obj(arena, t);
        let targs: Vec<TypeId> = guff_types::named_type_args(arena, t)
            .map(|l| l.list().to_vec())
            .unwrap_or_default();
        let origin = guff_types::named_origin(arena, t);

        if self.declared_within(oarena, obj) {
            // t is declared within the origin function.
            if targs.is_empty() {
                // A local type abstraction (`λx. U`, possibly with x empty): its
                // underlying may mention the origin's type parameters, so build a
                // fresh copy with the substitution applied.
                debug_assert_eq!(
                    origin, t,
                    "local parameterized type abstraction must be an origin type"
                );
                return self.fresh_local_named(arena, oarena, t, obj);
            }
            // A local generic type instance `A[targs]`: reduce to substituting
            // the origin abstraction and instantiating with the substituted args.
            let sub_origin = self.typ(arena, oarena, origin);
            let sub_targs = self.typelist(arena, oarena, &targs);
            return self.instantiate(arena, oarena, sub_origin, sub_targs);
        }

        // t is declared outside the origin function.
        if targs.is_empty() {
            // Not an instance: its underlying type cannot mention the
            // substituted parameters, so it is preserved.
            return t;
        }
        // An instance: substitute the type arguments (which may mention the
        // substituted parameters) and re-instantiate the origin.
        let new_targs = self.typelist(arena, oarena, &targs);
        self.instantiate(arena, oarena, origin, new_targs)
    }

    /// Builds a fresh copy of a `Named` type declared within the origin, applying
    /// the substitution to its underlying type and its own type parameters and
    /// constraints. (Go: `(*subster).named`, the `declaredWithin && targs==0`
    /// branch.)
    ///
    /// The partly-built copy is inserted into the cache (`cache[t] = fresh` and
    /// `cache[fresh] = fresh`) *before* its underlying is substituted, so a
    /// recursive local type (e.g. `type X struct{ next *X }`) resolves each
    /// self-reference to `fresh` instead of recursing forever. A distinct copy
    /// per instantiation matches Go treating each `F[N]` as having its own local
    /// types.
    fn fresh_local_named(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        t: TypeId,
        tname: ObjectId,
    ) -> TypeId {
        // Read the shape up front, releasing the immutable borrows before we
        // allocate / recurse.
        let tparams_orig: Vec<TypeId> = match arena.get(t) {
            TypeData::Named(n) => n
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => unreachable!("fresh_local_named called on non-Named"),
        };
        let underlying = guff_types::named_underlying(arena, t)
            .expect("a declared named type has an underlying type");
        let (name, pos, pkg) = obj_ident(oarena, tname);

        // Fresh TypeName + an incomplete Named (underlying set later), so the
        // cache short-circuit below can point recursive references at it.
        let fresh_obj = copy_type_name(oarena, &name, pos, pkg);
        let fresh = guff_types::new_named(arena, oarena, fresh_obj, None, vec![]);

        // Copy the type parameters, priming the cache so their occurrences within
        // the underlying map to the copies.
        let (new_tparams, bounds) = self.make_fresh_tparams(arena, oarena, &tparams_orig);
        if let Some(tpl) = guff_types::bind_tparams(arena, new_tparams.clone()) {
            guff_types::named_set_type_params(arena, fresh, tpl);
        }

        // Short-circuit: both t and the fresh copy resolve to fresh during
        // traversal of the underlying.
        self.cache.insert(t, fresh);
        self.cache.insert(fresh, fresh);

        let su = self.typ(arena, oarena, underlying);
        guff_types::set_underlying(arena, fresh, su);

        // Substitute the type parameters' constraints once the copies exist.
        self.set_fresh_constraints(arena, oarena, &new_tparams, &bounds);
        fresh
    }

    /// Substitutes within an `Alias` type. Follows the same strategy as
    /// [`Subster::named`]. (Go: `(*subster).alias`)
    fn alias(&mut self, arena: &mut TypeArena, oarena: &mut ObjectArena, t: TypeId) -> TypeId {
        let obj = guff_types::alias_obj(arena, t);
        let targs: Vec<TypeId> = match arena.get(t) {
            TypeData::Alias(a) => a
                .type_args()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => unreachable!("alias() called on non-Alias"),
        };
        let origin = guff_types::alias_origin(arena, t);

        if self.declared_within(oarena, obj) {
            if targs.is_empty() {
                return self.fresh_local_alias(arena, oarena, t, obj);
            }
            let sub_origin = self.typ(arena, oarena, origin);
            let sub_targs = self.typelist(arena, oarena, &targs);
            return self.instantiate(arena, oarena, sub_origin, sub_targs);
        }

        if targs.is_empty() {
            return t;
        }
        let new_targs = self.typelist(arena, oarena, &targs);
        self.instantiate(arena, oarena, origin, new_targs)
    }

    /// Builds a fresh copy of an `Alias` declared within the origin, substituting
    /// its right-hand side and its own type parameters and constraints. (Go:
    /// `(*subster).alias`, the `declaredWithin && targs==0` branch.)
    ///
    /// Unlike [`Subster::fresh_local_named`], no cache short-circuit is needed —
    /// aliases cannot be defined recursively — so the copies of the type
    /// parameters are primed in the cache, the right-hand side is substituted, and
    /// only then is the fresh alias created.
    fn fresh_local_alias(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        t: TypeId,
        tname: ObjectId,
    ) -> TypeId {
        let tparams_orig: Vec<TypeId> = match arena.get(t) {
            TypeData::Alias(a) => a
                .type_params()
                .map(|l| l.list().to_vec())
                .unwrap_or_default(),
            _ => unreachable!("fresh_local_alias called on non-Alias"),
        };
        let rhs = guff_types::alias_rhs(arena, t)
            .expect("a declared alias has a right-hand side");
        let (name, pos, pkg) = obj_ident(oarena, tname);

        // Copy the type parameters (priming the cache) before substituting rhs.
        let (new_tparams, bounds) = self.make_fresh_tparams(arena, oarena, &tparams_orig);
        let sub_rhs = self.typ(arena, oarena, rhs);

        let fresh_obj = copy_type_name(oarena, &name, pos, pkg);
        let fresh = guff_types::new_alias(arena, oarena, fresh_obj, Some(sub_rhs));
        if let Some(tpl) = guff_types::bind_tparams(arena, new_tparams.clone()) {
            guff_types::alias_set_type_params(arena, fresh, tpl);
        }

        self.set_fresh_constraints(arena, oarena, &new_tparams, &bounds);
        fresh
    }

    /// Creates a fresh, uninitialized-constraint copy of each type parameter in
    /// `orig`, priming `cache[orig[i]] = copy[i]` so occurrences within the type
    /// being copied map to the copies. Returns the copies and each original's
    /// (unsubstituted) constraint, for [`Subster::set_fresh_constraints`] to
    /// substitute once the copies exist. (Go: the `for cur := range
    /// tparams.TypeParams()` loop shared by `named` and `alias`.)
    fn make_fresh_tparams(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        orig: &[TypeId],
    ) -> (Vec<TypeId>, Vec<Option<TypeId>>) {
        // Read each original's (name, pos, pkg, constraint) up front.
        let metas: Vec<(String, u32, Option<PackageId>, Option<TypeId>)> = orig
            .iter()
            .map(|&cur| {
                let cobj = guff_types::type_param_obj(arena, cur);
                let (name, pos, pkg) = obj_ident(oarena, cobj);
                let bound = guff_types::type_param_constraint(arena, cur);
                (name, pos, pkg, bound)
            })
            .collect();

        let mut new_tparams = Vec::with_capacity(orig.len());
        let mut bounds = Vec::with_capacity(orig.len());
        for (&cur, (name, pos, pkg, bound)) in orig.iter().zip(metas) {
            let cobj = copy_type_name(oarena, &name, pos, pkg);
            let ntp = guff_types::new_type_param(arena, cobj, None);
            guff_types::type_name_set_typ(oarena, cobj, ntp);
            self.cache.insert(cur, ntp);
            new_tparams.push(ntp);
            bounds.push(bound);
        }
        (new_tparams, bounds)
    }

    /// Substitutes and installs each fresh type parameter's constraint, run after
    /// the copies exist so a constraint referring to a sibling copy resolves to
    /// it. (Go: the trailing `ntp.SetConstraint(subst.typ(bound))` loop.)
    fn set_fresh_constraints(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        new_tparams: &[TypeId],
        bounds: &[Option<TypeId>],
    ) {
        for (&ntp, &bound) in new_tparams.iter().zip(bounds) {
            if let Some(b) = bound {
                let sb = self.typ(arena, oarena, b);
                guff_types::set_constraint(arena, ntp, sb);
            }
        }
    }

    /// Substitutes each type in a type argument list. (Go: `(*subster).typelist`)
    fn typelist(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        l: &[TypeId],
    ) -> Vec<TypeId> {
        l.iter().map(|&x| self.typ(arena, oarena, x)).collect()
    }

    /// Instantiates `orig[targs...]`, canonicalizing the result within this
    /// substitution. (Go: `(*subster).instantiate`)
    fn instantiate(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        orig: TypeId,
        targs: Vec<TypeId>,
    ) -> TypeId {
        let i = guff_types::instantiate(arena, oarena, &mut self.ctxt, orig, targs);
        *self.uniqueness.entry(i).or_insert(i)
    }

    /// Reports whether `obj` is declared within the origin function's scope.
    /// (Go: `declaredWithin`)
    ///
    /// With no origin set (the common package-level substitution), nothing is
    /// "within", so this is always `false`. Otherwise the object's declaration
    /// position is tested against the origin function's scope bounds — Go
    /// "trusts the positions if they exist". Position-less objects (Go's parent-
    /// scope-walk fallback) do not occur for local type declarations, which always
    /// carry a source position, so that path is treated as "not within" here.
    fn declared_within(&self, oarena: &ObjectArena, obj: ObjectId) -> bool {
        match self.origin_scope {
            None => false,
            Some((scope_pos, scope_end)) => {
                let pos = obj.pos(oarena);
                // pos == 0 is `token.NoPos`; a real declaration has pos != 0.
                pos != 0 && scope_pos <= pos && pos < scope_end
            }
        }
    }

    /// Substitutes the type of a single variable. If the type is unchanged the
    /// same `ObjectId` is returned; otherwise a fresh `Var` (a field or a
    /// parameter, matching the original) with the substituted type is allocated.
    /// (Go: `(*subster).var_`)
    fn var_(&mut self, arena: &mut TypeArena, oarena: &mut ObjectArena, v: ObjectId) -> ObjectId {
        // Read the variable's shape up front, releasing the object-arena borrow
        // before recursing into `typ`.
        let (vtyp, is_field, embedded, name) = match oarena.get(v) {
            ObjectData::Var(var) => (
                var.typ(),
                var.is_field(),
                var.embedded(),
                var.name().to_string(),
            ),
            other => panic!(
                "subst.var_ expected Var, got {:?}",
                std::mem::discriminant(other)
            ),
        };
        let r = self.typ(arena, oarena, vtyp);
        if r == vtyp {
            return v;
        }
        if is_field {
            guff_types::object::var::new_field(oarena, name, r, embedded)
        } else {
            guff_types::object::var::new_param(oarena, name, r)
        }
    }

    /// Substitutes each variable's type. Returns `Some(new_vars)` if any
    /// variable changed (with every element re-resolved via [`Subster::var_`]),
    /// or `None` when the whole list is unchanged. (Go: `(*subster).varlist`)
    fn varlist(
        &mut self,
        arena: &mut TypeArena,
        oarena: &mut ObjectArena,
        vars: &[ObjectId],
    ) -> Option<Vec<ObjectId>> {
        let mut changed = false;
        let mut out = Vec::with_capacity(vars.len());
        for &v in vars {
            let w = self.var_(arena, oarena, v);
            changed |= w != v;
            out.push(w);
        }
        if changed {
            Some(out)
        } else {
            None
        }
    }
}

/// Reads an object's `(name, pos, pkg)` triple. Used to copy a `TypeName`'s
/// identity onto a fresh local-type copy. (Go: `obj.Name()`/`.Pos()`/`.Pkg()`.)
fn obj_ident(oarena: &ObjectArena, obj: ObjectId) -> (String, u32, Option<PackageId>) {
    (obj.name(oarena).to_string(), obj.pos(oarena), obj.pkg(oarena))
}

/// Allocates a fresh `TypeName` carrying the given identity (its bound type is
/// filled in by the caller). (Go: `types.NewTypeName(pos, pkg, name, nil)`.)
fn copy_type_name(
    oarena: &mut ObjectArena,
    name: &str,
    pos: u32,
    pkg: Option<PackageId>,
) -> ObjectId {
    let id = guff_types::new_type_name(oarena, name.to_string(), None);
    id.set_pos(oarena, pos);
    if let Some(p) = pkg {
        id.set_pkg(oarena, p);
    }
    id
}
