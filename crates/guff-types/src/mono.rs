//! Port of `cmd/compile/internal/types2/mono.go` (chunk 58).
//!
//! Validates that a package does not have *unbounded recursive
//! instantiation*, which is incompatible with compilers that use static
//! instantiation (monomorphization).
//!
//! It implements a "type flow" analysis: a directed, weighted graph whose
//! vertices represent type parameters (and some locally-defined types) and
//! whose edges record how a type argument flows into a type parameter. An
//! edge has weight 0 when the type argument *is* the referenced type itself,
//! and weight 1 when it is a type derived from it (e.g. `*T`, `map[A]B`). A
//! package cannot be statically instantiated if the graph has any
//! positive-weight cycle (zero-weight cycles reach a fixed point).
//!
//! For example:
//! ```text
//! func f[A, B any]() {
//!     type T int
//!     f[T, map[A]B]()
//! }
//! ```
//! produces vertices A, B, T; edges T<-A and T<-B (weight 1, from the local
//! `type T` declaration), A<-T (weight 0), B<-A and B<-B (weight 1). The
//! positive-weight cycle B<-B flags the package as non-monomorphizable.
//!
//! **Decoupling from Go**: the graph methods take the arenas explicitly so
//! the `MonoGraph` can live as a plain field of `Checker` (Go keeps it as
//! `check.mono`). The driver (`monomorph`) and the error report
//! (`report_instance_loop`) are `Checker` methods because they emit
//! diagnostics.
//!
//! **Deferred (D07-limited)**: object source positions are mostly `0` in this
//! port, so `local_named_vertex`'s "declared before" position gate
//! (`elem.pos < obj.pos`) finds no ambient type parameters — the
//! locally-defined-type edge class therefore degrades to a no-op (it can miss
//! some cycles, but never reports a false one). The direct type-parameter
//! flow class — the common case, e.g. `func f[T any]() { f[*T]() }` — works.
//! Also deferred: `record_canon` wiring at the generic-method receiver site
//! (the canon map is built but never populated, so method type parameters get
//! their own vertices = conservative), and the multi-line secondary-error
//! detail (collapsed to a single message, matching this port's error policy).

use crate::hash::HashMap;

use guff::ast::Expr;

use crate::alias::unalias_readonly;
use crate::arena::{
    ObjectArena, ObjectData, ObjectId, PackageArena, PackageId, ScopeArena, TypeArena, TypeData,
    TypeId,
};
use crate::array::array_elem;
use crate::chan::chan_elem;
use crate::check::Checker;
use crate::map::{map_elem, map_key};
use crate::named::{named_obj, named_origin, named_type_args};
use crate::pointer::pointer_elem;
use crate::r#struct::{struct_field, struct_num_fields};
use crate::signature::{signature_params, signature_results};
use crate::slice::slice_elem;
use crate::tuple::{tuple_at, tuple_len};
use crate::typeparam::type_param_obj;

use guff_types_errors::Code;

/// A vertex in the monomorphization flow graph, representing either a type
/// parameter or a locally-defined type (via its `TypeName` object).
#[derive(Debug, Clone)]
pub struct MonoVertex {
    /// Weight of the heaviest known path to this vertex.
    weight: i64,
    /// Previous edge (index into `edges`) on that path, or `-1` if none.
    pre: i64,
    /// Length of that path.
    len: i64,
    /// The defined type or type parameter this vertex represents.
    obj: ObjectId,
}

/// A directed, weighted edge: `typ` flows into `dst` from `src` at `pos`.
#[derive(Debug, Clone)]
pub struct MonoEdge {
    dst: usize,
    src: usize,
    weight: i64,
    /// Source position of the instantiation. Retained for faithful porting:
    /// Go positions each secondary cycle error at this `pos`. This port
    /// collapses the cycle into a single primary message (see
    /// `report_instance_loop`), and object positions are mostly `0` (D07), so
    /// it is not yet read.
    #[allow(dead_code)]
    pos: u32,
    typ: TypeId,
}

/// The monomorphization flow graph (Go's `monoGraph`).
#[derive(Debug, Default)]
pub struct MonoGraph {
    vertices: Vec<MonoVertex>,
    edges: Vec<MonoEdge>,

    /// Maps method-receiver type parameters to their receiver type's type
    /// parameters (keyed/valued by `TypeParam` `TypeId`). Populated by
    /// `record_canon` (currently unwired — see module docs).
    canon: HashMap<TypeId, TypeId>,

    /// Maps a defined type or (canonical) type parameter `TypeName` to its
    /// vertex index. May hold `-1` for a local type with no ambient type
    /// parameters (cached negative result from `local_named_vertex`).
    name_idx: HashMap<ObjectId, i64>,
}

impl MonoGraph {
    /// Records that `tpar` is the canonical type parameter corresponding to
    /// method type parameter `mpar` (Go's `recordCanon`).
    pub fn record_canon(&mut self, mpar: TypeId, tpar: TypeId) {
        self.canon.insert(mpar, tpar);
    }

    /// Records that the given type parameters were instantiated with the
    /// corresponding type arguments (Go's `recordInstance`). `xlist` is the
    /// explicit type-argument expression list (empty for inferred calls); it
    /// is used only to pick a better error position.
    #[allow(clippy::too_many_arguments)]
    pub fn record_instance(
        &mut self,
        types: &TypeArena,
        objects: &ObjectArena,
        scopes: &ScopeArena,
        packages: &PackageArena,
        pkg: PackageId,
        pos: u32,
        tparams: &[TypeId],
        targs: &[TypeId],
        xlist: &[Expr],
    ) {
        let positions: Vec<u32> = xlist.iter().map(|e| e.pos().0 as u32).collect();
        self.record_instance_at(
            types, objects, scopes, packages, pkg, pos, tparams, targs, &positions,
        );
    }

    /// Same, for callers that no longer hold the argument expressions — the
    /// deferred constraint check in `typexpr.rs` runs after the AST borrow is
    /// gone, so it carries the positions instead.
    #[allow(clippy::too_many_arguments)]
    pub fn record_instance_at(
        &mut self,
        types: &TypeArena,
        objects: &ObjectArena,
        scopes: &ScopeArena,
        packages: &PackageArena,
        pkg: PackageId,
        pos: u32,
        tparams: &[TypeId],
        targs: &[TypeId],
        positions: &[u32],
    ) {
        for (i, &tpar) in tparams.iter().enumerate() {
            if i >= targs.len() {
                break;
            }
            let pos = positions.get(i).copied().unwrap_or(pos);
            self.assign(types, objects, scopes, packages, pkg, pos, tpar, targs[i]);
        }
    }

    /// Records that `tpar` was instantiated as `targ` at `pos` (Go's
    /// `assign`).
    #[allow(clippy::too_many_arguments)]
    fn assign(
        &mut self,
        types: &TypeArena,
        objects: &ObjectArena,
        scopes: &ScopeArena,
        packages: &PackageArena,
        pkg: PackageId,
        pos: u32,
        tpar: TypeId,
        targ: TypeId,
    ) {
        // Instantiation cycles must occur within a single package, so we can
        // ignore instantiations of imported type parameters.
        if type_param_obj(types, tpar).pkg(objects) != Some(pkg) {
            return;
        }

        // The destination vertex (the type parameter being instantiated) is
        // fixed for the whole walk of `targ`.
        let dst = self.type_param_vertex(types, tpar);

        // Recursively walk the type argument to find any defined types or
        // type parameters that flow into `tpar`.
        self.do_walk(types, objects, scopes, packages, pkg, pos, dst, targ, targ);
    }

    /// Walk helper for `assign`'s `do` closure. `dst` is the vertex of the
    /// type parameter being instantiated; `targ` is the original (top-level)
    /// type argument used for the weight-0 self-flow comparison.
    #[allow(clippy::too_many_arguments)]
    fn do_walk(
        &mut self,
        types: &TypeArena,
        objects: &ObjectArena,
        scopes: &ScopeArena,
        packages: &PackageArena,
        pkg: PackageId,
        pos: u32,
        dst: usize,
        typ: TypeId,
        targ: TypeId,
    ) {
        let typ = unalias_readonly(types, typ);
        match types.get(typ) {
            TypeData::TypeParam(_) => {
                let src = self.type_param_vertex(types, typ);
                self.flow(dst, src, typ, targ, pos);
            }
            TypeData::Named(_) => {
                let origin = named_origin(types, typ);
                let src = self.local_named_vertex(types, objects, scopes, packages, pkg, origin);
                if src >= 0 {
                    self.flow(dst, src as usize, typ, targ, pos);
                }
                // Walk the type arguments.
                let targs: Vec<TypeId> = named_type_args(types, typ)
                    .map(|tl| tl.list().to_vec())
                    .unwrap_or_default();
                for t in targs {
                    self.do_walk(types, objects, scopes, packages, pkg, pos, dst, t, targ);
                }
            }
            TypeData::Array(_) => {
                let e = array_elem(types, typ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, e, targ);
            }
            TypeData::Basic(_) => { /* ok — no flow */ }
            TypeData::Chan(_) => {
                let e = chan_elem(types, typ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, e, targ);
            }
            TypeData::Map(_) => {
                let k = map_key(types, typ);
                let v = map_elem(types, typ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, k, targ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, v, targ);
            }
            TypeData::Pointer(_) => {
                let e = pointer_elem(types, typ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, e, targ);
            }
            TypeData::Slice(_) => {
                let e = slice_elem(types, typ);
                self.do_walk(types, objects, scopes, packages, pkg, pos, dst, e, targ);
            }
            TypeData::Interface(_) => {
                for m in interface_methods_ro(types, typ) {
                    if let Some(mt) = m.typ(objects) {
                        self.do_walk(types, objects, scopes, packages, pkg, pos, dst, mt, targ);
                    }
                }
            }
            TypeData::Signature(_) => {
                for tup in [signature_params(types, typ), signature_results(types, typ)] {
                    let n = tuple_len(types, tup);
                    if let Some(tup) = tup {
                        for i in 0..n {
                            let v = tuple_at(types, tup, i);
                            if let Some(vt) = v.typ(objects) {
                                self.do_walk(
                                    types, objects, scopes, packages, pkg, pos, dst, vt, targ,
                                );
                            }
                        }
                    }
                }
            }
            TypeData::Struct(_) => {
                let n = struct_num_fields(types, typ);
                for i in 0..n {
                    let f = struct_field(types, typ, i);
                    if let Some(ft) = f.typ(objects) {
                        self.do_walk(types, objects, scopes, packages, pkg, pos, dst, ft, targ);
                    }
                }
            }
            // Tuple/Union/Alias do not appear as type-argument shapes here
            // (Alias is stripped by `unalias_readonly` above). Treat as ok.
            _ => {}
        }
    }

    /// Adds an edge from `src` into `dst`, with weight 0 if `typ` *is* the
    /// original type argument `targ`, else weight 1 (Go's `flow` closure).
    fn flow(&mut self, dst: usize, src: usize, typ: TypeId, targ: TypeId, pos: u32) {
        let weight = if typ == targ { 0 } else { 1 };
        self.add_edge(dst, src, weight, pos, targ);
    }

    /// Returns the index of the vertex representing `named`, or `-1` if it
    /// doesn't need representation (imported, or declared at package scope so
    /// it has no ambient type parameters). Go's `localNamedVertex`.
    fn local_named_vertex(
        &mut self,
        types: &TypeArena,
        objects: &ObjectArena,
        scopes: &ScopeArena,
        packages: &PackageArena,
        pkg: PackageId,
        named: TypeId,
    ) -> i64 {
        let obj = named_obj(types, named);
        if obj.pkg(objects) != Some(pkg) {
            return -1; // imported type
        }
        let root = packages.get(pkg).scope();
        if obj.parent(objects) == Some(root) {
            return -1; // package scope, no ambient type parameters
        }
        if let Some(&idx) = self.name_idx.get(&obj) {
            return idx;
        }

        let mut idx: i64 = -1;
        let obj_pos = obj.pos(objects);

        // Walk the type definition's enclosing scopes to find ambient type
        // parameters it is implicitly parameterized by.
        let mut scope_opt = obj.parent(objects);
        while let Some(scope) = scope_opt {
            if scope == root {
                break;
            }
            let s = scopes.get(scope);
            for name in s.names() {
                let Some(elem) = s.lookup_local(&name) else {
                    continue;
                };
                // TypeName, declared before obj, of TypeParam type. (Go also
                // filters `!IsAlias()`, but the TypeParam-typ check below
                // already excludes aliases, whose typ is never a TypeParam.)
                if !matches!(objects.get(elem), ObjectData::TypeName(_)) {
                    continue;
                }
                if elem.pos(objects) >= obj_pos {
                    continue;
                }
                let Some(et) = elem.typ(objects) else {
                    continue;
                };
                if !matches!(types.get(et), TypeData::TypeParam(_)) {
                    continue;
                }
                if idx < 0 {
                    idx = self.vertices.len() as i64;
                    self.vertices.push(MonoVertex {
                        weight: 0,
                        pre: -1,
                        len: 0,
                        obj,
                    });
                }
                let tpv = self.type_param_vertex(types, et);
                self.add_edge(idx as usize, tpv, 1, elem.pos(objects), et);
            }
            scope_opt = s.parent();
        }

        self.name_idx.insert(obj, idx);
        idx
    }

    /// Returns the index of the vertex representing `tpar`, creating it if
    /// needed (Go's `typeParamVertex`).
    fn type_param_vertex(&mut self, types: &TypeArena, tpar: TypeId) -> usize {
        let tpar = *self.canon.get(&tpar).unwrap_or(&tpar);
        let obj = type_param_obj(types, tpar);
        if let Some(&idx) = self.name_idx.get(&obj) {
            return idx as usize;
        }
        let idx = self.vertices.len();
        self.vertices.push(MonoVertex {
            weight: 0,
            pre: -1,
            len: 0,
            obj,
        });
        self.name_idx.insert(obj, idx as i64);
        idx
    }

    fn add_edge(&mut self, dst: usize, src: usize, weight: i64, pos: u32, typ: TypeId) {
        self.edges.push(MonoEdge {
            dst,
            src,
            weight,
            pos,
            typ,
        });
    }
}

/// Reads the interface's full method set from its cached `tset`. The public
/// `interface_method` accessor needs `&mut TypeArena` to compute the type set
/// lazily; by the time instances are recorded every interface has been
/// checked, so the cache is present. Returns an empty list if it is not.
fn interface_methods_ro(types: &TypeArena, id: TypeId) -> Vec<ObjectId> {
    match types.get(id) {
        TypeData::Interface(i) => match i.tset.as_ref() {
            Some(t) => (0..t.num_methods()).map(|j| t.method(j)).collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

impl Checker {
    /// Detects unbounded instantiation cycles using a variant of
    /// Bellman-Ford (Go's `monomorph`). Instead of always running `|V|`
    /// iterations, it runs until a fixed point or until it finds a path of
    /// length `|V|` (a cycle). N.B. we look for *greatest*-weight paths.
    pub fn monomorph(&mut self) {
        let mut again = true;
        while again {
            again = false;

            for i in 0..self.mono.edges.len() {
                let edge = self.mono.edges[i].clone();
                let sw = self.mono.vertices[edge.src].weight;
                let dw = self.mono.vertices[edge.dst].weight;

                let w = sw + edge.weight;
                if w <= dw {
                    continue;
                }

                let src_len = self.mono.vertices[edge.src].len;
                {
                    let dst = &mut self.mono.vertices[edge.dst];
                    dst.pre = i as i64;
                    dst.len = src_len + 1;
                }
                if self.mono.vertices[edge.dst].len == self.mono.vertices.len() as i64 {
                    self.report_instance_loop(edge.dst);
                    return;
                }

                self.mono.vertices[edge.dst].weight = w;
                again = true;
            }
        }
    }

    /// Reports an instantiation cycle ending at vertex `v` (Go's
    /// `reportInstanceLoop`). The multi-line secondary detail is collapsed
    /// into a single message, matching this port's simplified error policy.
    fn report_instance_loop(&mut self, mut v: usize) {
        let n = self.mono.vertices.len();
        let mut stack: Vec<usize> = Vec::new();
        let mut seen = vec![false; n];

        // Walk backwards along the path until we revisit a vertex (the cycle).
        while !seen[v] {
            stack.push(v);
            seen[v] = true;
            let pre = self.mono.vertices[v].pre;
            v = self.mono.edges[pre as usize].src;
        }

        // Trim any vertices visited before first reaching v: they are not on
        // the cycle.
        while stack.first().copied() != Some(v) {
            stack.remove(0);
        }

        // Build the (single-line) message: the cycle head plus each step.
        let obj0 = self.mono.vertices[v].obj;
        let obj0_name = obj0.name(&self.objects).to_string();
        let mut parts: Vec<String> = Vec::new();
        let pos0 = obj0.pos(&self.objects);

        for &sv in &stack {
            let pre = self.mono.vertices[sv].pre;
            let edge = self.mono.edges[pre as usize].clone();
            let obj = self.mono.vertices[edge.dst].obj;
            let obj_name = obj.name(&self.objects).to_string();
            let typ_str = self.type_str(edge.typ);
            let kind = match self.objects.get(obj).typ_kind(&self.types) {
                TypeKind::Named => "implicitly parameterized by",
                _ => "instantiated as",
            };
            parts.push(format!("{} {} {}", obj_name, kind, typ_str));
        }

        let msg = if parts.is_empty() {
            format!("instantiation cycle: {}", obj0_name)
        } else {
            format!("instantiation cycle: {} ({})", obj0_name, parts.join("; "))
        };
        self.error(pos0, Code::InvalidInstanceCycle, msg);
    }
}

/// Minimal kind classification used only for the cycle-report wording.
enum TypeKind {
    Named,
    Other,
}

impl ObjectData {
    fn typ_kind(&self, types: &TypeArena) -> TypeKind {
        let typ = match self {
            ObjectData::TypeName(tn) => tn.typ(),
            _ => None,
        };
        match typ {
            Some(t) if matches!(types.get(t), TypeData::Named(_)) => TypeKind::Named,
            _ => TypeKind::Other,
        }
    }
}
