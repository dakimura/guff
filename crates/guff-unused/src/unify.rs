//! Port of `honnef.co/go/tools/go/types/typeutil.Unify` (staticcheck v0.8.1,
//! itself copied from gopls' `implementation.go`).
//!
//! `unused` asks it whether a concrete method implements an interface method
//! whose signature mentions the interface's type parameters. v0.7.0 matched a
//! *bare* `T` only, so `list() ([]T, error)` never matched `list() ([]int,
//! error)` and every such method was reported (dapr's forty findings, see
//! `lenient_implements`); unification matches the structure around `T` too.
//!
//! Upstream panics on a type it does not expect (a `Union`, or recursion past
//! depth 100). guff answers "does not unify" instead: the caller then treats
//! the method as not implementing, which is upstream's answer for every input
//! that does not crash it.

use std::collections::HashMap;

use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectArena, ObjectData, TypeArena, TypeData, TypeId};
use guff_types::named::{named_origin, named_type_args};

use guff_types::tuple::{tuple_at, tuple_len};

/// `unifier`: type parameter → the type it is bound to. On entry the bound
/// entries constrain the unification; on success every parameter the call
/// touched is written back (an unbound one as `None`); on failure the map is
/// cleared, as upstream's `clear(unifier)`.
pub type Unifier = HashMap<TypeId, Option<TypeId>>;

struct State<'a> {
    types: &'a TypeArena,
    objects: &'a ObjectArena,
    /// type parameter → slot; parameters unified with each other share one.
    bindings: HashMap<TypeId, usize>,
    slots: Vec<Option<TypeId>>,
    depth: usize,
}

impl State<'_> {
    fn binding_for(&mut self, tp: TypeId) -> usize {
        if let Some(&s) = self.bindings.get(&tp) {
            return s;
        }
        self.slots.push(None);
        let s = self.slots.len() - 1;
        self.bindings.insert(tp, s);
        s
    }

    /// `bind`: set slot `b` to `t` unless `b` occurs in `t`.
    fn bind(&mut self, b: usize, t: TypeId) -> bool {
        let mut tps = Vec::new();
        type_params(self.types, self.objects, t, &mut tps);
        if tps.iter().any(|tp| self.bindings.get(tp) == Some(&b)) {
            return false; // failed "occurs" check
        }
        self.slots[b] = Some(t);
        true
    }

    fn uni(&mut self, x: TypeId, y: TypeId) -> bool {
        self.depth += 1;
        let ok = self.uni_inner(x, y);
        self.depth -= 1;
        ok
    }

    fn uni_inner(&mut self, x: TypeId, y: TypeId) -> bool {
        if self.depth > 100 {
            return false; // upstream: panic("unify: max depth exceeded")
        }
        let (mut x, mut y) = (unalias_readonly(self.types, x), unalias_readonly(self.types, y));
        let tpx = matches!(self.types.get(x), TypeData::TypeParam(_)).then_some(x);
        let tpy = matches!(self.types.get(y), TypeData::TypeParam(_)).then_some(y);
        if tpx.is_some() || tpy.is_some() {
            if tpx == tpy {
                return true;
            }
            let bx = tpx.map(|tp| self.binding_for(tp));
            let by = tpy.map(|tp| self.binding_for(tp));
            if let (Some(bx), Some(by)) = (bx, by) {
                if self.slots[bx].is_none() && self.slots[by].is_none() {
                    // Both unbound: x shares y's binding.
                    self.bindings.insert(tpx.unwrap(), by);
                    return true;
                }
            }
            if let Some(t) = bx.and_then(|b| self.slots[b]) {
                x = t;
            }
            if let Some(t) = by.and_then(|b| self.slots[b]) {
                y = t;
            }
            if let Some(b) = bx.filter(|&b| self.slots[b].is_none()) {
                return self.bind(b, y);
            }
            if let Some(b) = by.filter(|&b| self.slots[b].is_none()) {
                return self.bind(b, x);
            }
            return self.uni(x, y);
        }

        let (tx, ty) = (self.types.get(x), self.types.get(y));
        if std::mem::discriminant(tx) != std::mem::discriminant(ty) {
            return false; // mismatched types
        }
        match (tx, ty) {
            (TypeData::Array(a), TypeData::Array(b)) => {
                let (ae, be) = (a.elem(), b.elem());
                a.len() == b.len() && self.uni(ae, be)
            }
            (TypeData::Basic(a), TypeData::Basic(b)) => a.kind() == b.kind(),
            (TypeData::Chan(a), TypeData::Chan(b)) => {
                let (ae, be) = (a.elem(), b.elem());
                a.dir() == b.dir() && self.uni(ae, be)
            }
            // Upstream's own TODO: only the method counts are compared. The
            // type set is computed by the checker for every interface it
            // completes; an uncompleted one falls back to its explicit methods.
            (TypeData::Interface(a), TypeData::Interface(b)) => {
                let n = |i: &guff_types::interface::Interface| {
                    i.cached_typeset()
                        .map(|ts| ts.num_methods())
                        .unwrap_or_else(|| i.num_explicit_methods())
                };
                n(a) == n(b)
            }
            (TypeData::Map(a), TypeData::Map(b)) => {
                let (ak, ae, bk, be) = (a.key(), a.elem(), b.key(), b.elem());
                self.uni(ak, bk) && self.uni(ae, be)
            }
            (TypeData::Named(_), TypeData::Named(_)) => {
                if named_origin(self.types, x) != named_origin(self.types, y) {
                    return false; // different named types
                }
                let xa: Vec<TypeId> =
                    named_type_args(self.types, x).map(|l| l.list().to_vec()).unwrap_or_default();
                let ya: Vec<TypeId> =
                    named_type_args(self.types, y).map(|l| l.list().to_vec()).unwrap_or_default();
                if xa.len() != ya.len() {
                    return false; // arity error (ill-typed)
                }
                xa.into_iter().zip(ya).all(|(a, b)| self.uni(a, b))
            }
            (TypeData::Pointer(a), TypeData::Pointer(b)) => {
                let (ae, be) = (a.elem(), b.elem());
                self.uni(ae, be)
            }
            (TypeData::Signature(a), TypeData::Signature(b)) => {
                let (av, bv) = (a.variadic(), b.variadic());
                let (ap, bp, ar, br) = (a.params(), b.params(), a.results(), b.results());
                av == bv && self.uni_tuples(ap, bp) && self.uni_tuples(ar, br)
            }
            (TypeData::Slice(a), TypeData::Slice(b)) => {
                let (ae, be) = (a.elem(), b.elem());
                self.uni(ae, be)
            }
            (TypeData::Struct(a), TypeData::Struct(b)) => {
                if a.num_fields() != b.num_fields() {
                    return false;
                }
                let pairs: Vec<(TypeId, TypeId)> = {
                    let mut out = Vec::new();
                    for i in 0..a.num_fields() {
                        let (fa, fb) = (a.field(i), b.field(i));
                        let (ObjectData::Var(va), ObjectData::Var(vb)) =
                            (self.objects.get(fa), self.objects.get(fb))
                        else {
                            return false;
                        };
                        if va.embedded() != vb.embedded()
                            || va.name() != vb.name()
                            || a.tag(i) != b.tag(i)
                            || (!fa.exported(self.objects)
                                && fa.pkg(self.objects) != fb.pkg(self.objects))
                        {
                            return false;
                        }
                        out.push((va.typ(), vb.typ()));
                    }
                    out
                };
                pairs.into_iter().all(|(a, b)| self.uni(a, b))
            }
            (TypeData::Tuple(_), TypeData::Tuple(_)) => self.uni_tuples(Some(x), Some(y)),
            // Upstream: `panic(fmt.Sprintf("unexpected Type %#v", x))`.
            _ => false,
        }
    }

    fn uni_tuples(&mut self, x: Option<TypeId>, y: Option<TypeId>) -> bool {
        let n = tuple_len(self.types, x);
        if n != tuple_len(self.types, y) {
            return false;
        }
        let (Some(x), Some(y)) = (x, y) else {
            return n == 0;
        };
        for i in 0..n {
            let a = tuple_at(self.types, x, i).typ(self.objects);
            let b = tuple_at(self.types, y, i).typ(self.objects);
            let (Some(a), Some(b)) = (a, b) else {
                return false;
            };
            if !self.uni(a, b) {
                return false;
            }
        }
        true
    }
}

/// `typeParams`: the free type parameters in `t` that matter for the occurs
/// check. Interfaces are not looked into (upstream's own TODO).
fn type_params(types: &TypeArena, objects: &ObjectArena, t: TypeId, out: &mut Vec<TypeId>) {
    fn walk(
        types: &TypeArena,
        objects: &ObjectArena,
        t: TypeId,
        out: &mut Vec<TypeId>,
        depth: usize,
    ) {
        if depth > 100 {
            return;
        }
        let t = unalias_readonly(types, t);
        match types.get(t) {
            TypeData::TypeParam(_) => {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
            TypeData::Array(a) => walk(types, objects, a.elem(), out, depth + 1),
            TypeData::Chan(c) => walk(types, objects, c.elem(), out, depth + 1),
            TypeData::Map(m) => {
                walk(types, objects, m.key(), out, depth + 1);
                walk(types, objects, m.elem(), out, depth + 1);
            }
            TypeData::Named(n) => {
                if named_origin(types, t) == t {
                    if let Some(tps) = n.type_params() {
                        for &tp in tps.list() {
                            walk(types, objects, tp, out, depth + 1);
                        }
                    }
                } else if let Some(args) = named_type_args(types, t) {
                    for &a in args.list() {
                        walk(types, objects, a, out, depth + 1);
                    }
                }
            }
            TypeData::Pointer(p) => walk(types, objects, p.elem(), out, depth + 1),
            TypeData::Slice(s) => walk(types, objects, s.elem(), out, depth + 1),
            TypeData::Signature(s) => {
                for tup in [s.params(), s.results()].into_iter().flatten() {
                    walk(types, objects, tup, out, depth + 1);
                }
            }
            TypeData::Struct(s) => {
                for i in 0..s.num_fields() {
                    if let Some(ft) = s.field(i).typ(objects) {
                        walk(types, objects, ft, out, depth + 1);
                    }
                }
            }
            TypeData::Tuple(_) => {
                for i in 0..tuple_len(types, Some(t)) {
                    if let Some(vt) = tuple_at(types, t, i).typ(objects) {
                        walk(types, objects, vt, out, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }
    walk(types, objects, t, out, 0);
}

/// `Unify(x, y, unifier)`.
pub fn unify(
    types: &TypeArena,
    objects: &ObjectArena,
    x: TypeId,
    y: TypeId,
    unifier: &mut Unifier,
) -> bool {
    let mut st = State {
        types,
        objects,
        bindings: HashMap::new(),
        slots: Vec::new(),
        depth: 0,
    };
    for (&tp, &t) in unifier.iter() {
        st.slots.push(t);
        st.bindings.insert(tp, st.slots.len() - 1);
    }
    if !st.uni(x, y) {
        unifier.clear();
        return false;
    }
    for (tp, slot) in st.bindings {
        unifier.insert(tp, st.slots[slot]);
    }
    true
}
