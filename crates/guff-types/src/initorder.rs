//! Port of `cmd/compile/internal/types2/initorder.go`.
//!
//! Computes [`Info::init_order`](crate::api::Info::init_order): the order in
//! which package-level variables with initialization expressions must be
//! evaluated, derived from the object dependency graph built by `add_decl_dep`
//! (see [`Checker::add_decl_dep`](crate::Checker::add_decl_dep)).
//!
//! ## Differences from Go
//!
//! - Go uses a `container/heap` priority queue with `heap.Fix` (decrease-key).
//!   Because the `Less` ordering is a strict total order (the final tie-breaker
//!   is the per-object source `order()`, which is unique), repeatedly selecting
//!   the minimum by linear scan yields the identical sequence — so we do that
//!   instead of porting the heap. Package-level variable counts are small.
//! - The object dependency graph is represented with `Vec<GraphNode>` and
//!   `usize` node indices rather than `*graphNode` pointers and `nodeSet`
//!   pointer maps.
//! - `reportCycle` emits a single concise [`Code::InvalidInitCycle`] error
//!   rather than Go's multi-line error with one `refers to` line per edge.

use crate::hash::{HashMap, HashSet};

use guff_types_errors::Code;

use crate::api::Initializer;
use crate::arena::{ObjectArena, ObjectData};
use crate::check::Checker;
use crate::resolver::DeclInfo;
use crate::ObjectId;

/// A node in the object dependency graph.
///
/// Each index `p` in `pred` is an edge `p -> self` (a consumer of this node);
/// each index `s` in `succ` is an edge `self -> s` (a dependency of this node),
/// with `a -> b` meaning "a depends on b".
struct GraphNode {
    /// The object represented by this node (a constant, variable, or function).
    obj: ObjectId,
    /// Indices of consumer nodes (predecessors).
    pred: HashSet<usize>,
    /// Indices of dependency nodes (successors).
    succ: HashSet<usize>,
    /// Number of outstanding dependencies before this object can be initialized.
    ndeps: i64,
}

impl GraphNode {
    /// The cost of removing this node: each predecessor is copied to each
    /// successor (and vice-versa). Equivalent to `graphNode.cost`.
    fn cost(&self) -> usize {
        self.pred.len() * self.succ.len()
    }
}

/// Reports whether `obj` may be an initialization dependency — Go's
/// `dependency` interface, satisfied only by constants, variables, and
/// functions. (Constants are included because constant-expression cycles are
/// reported during init-order computation.)
fn is_dependency(objects: &ObjectArena, obj: ObjectId) -> bool {
    matches!(
        objects.get(obj),
        ObjectData::Const(_) | ObjectData::Var(_) | ObjectData::Func(_)
    )
}

fn is_const(objects: &ObjectArena, obj: ObjectId) -> bool {
    matches!(objects.get(obj), ObjectData::Const(_))
}

fn is_func(objects: &ObjectArena, obj: ObjectId) -> bool {
    matches!(objects.get(obj), ObjectData::Func(_))
}

fn is_var(objects: &ObjectArena, obj: ObjectId) -> bool {
    matches!(objects.get(obj), ObjectData::Var(_))
}

impl Checker {
    /// Compute [`Info::init_order`](crate::api::Info::init_order) for the
    /// package being checked.
    ///
    /// Equivalent to `Checker.initOrder` (`initorder.go`).
    pub fn init_order(&mut self) {
        // An InitOrder may already have been computed if a package is built
        // from several calls; clear it.
        self.info.init_order.clear();

        // Build the dependency graph (function nodes eliminated), then collect
        // the non-function graph nodes into the priority working set.
        let mut nodes = self.dependency_graph();

        // Working set of remaining (non-function) node indices, paired with the
        // index into `nodes`. We pop the highest-priority node (fewest
        // dependencies) repeatedly until the set is empty.
        let mut remaining: Vec<usize> = (0..nodes.len()).collect();

        // Track which n:1 declarations (keyed by a representative variable) have
        // already emitted an Initializer.
        let mut emitted: HashSet<ObjectId> = HashSet::default();

        while !remaining.is_empty() {
            // Select the next node: minimum by `node_less`.
            let mut best = 0usize; // position within `remaining`
            for i in 1..remaining.len() {
                if self.node_less(&nodes, remaining[i], remaining[best]) {
                    best = i;
                }
            }
            let n_idx = remaining.swap_remove(best);

            // If n still depends on other nodes, we have a cycle.
            if nodes[n_idx].ndeps > 0 {
                let from = nodes[n_idx].obj;
                let cycle = find_path(
                    &self.obj_map,
                    &self.objects,
                    from,
                    from,
                    &mut HashSet::default(),
                );
                // If `from` is not part of the cycle, `cycle` is empty; the
                // cycle is reported when the algorithm reaches an object that
                // is in it. Once reached, the cycle is broken (dependency
                // counts reduced below), so remaining nodes don't re-trigger.
                if !cycle.is_empty() {
                    self.report_cycle(&cycle);
                }
            }

            // Reduce the dependency count of all dependent (predecessor) nodes.
            let preds: Vec<usize> = nodes[n_idx].pred.iter().copied().collect();
            for p in preds {
                nodes[p].ndeps -= 1;
            }

            // Record the init order for variables with initializers only.
            let v = nodes[n_idx].obj;
            if !is_var(&self.objects, v) {
                continue;
            }
            let info = match self.obj_map.get(&v) {
                Some(d) if d.has_initializer() => d,
                _ => continue,
            };

            // n:1 variable declarations (a, b = f()) introduce a node per lhs
            // variable but share one initializer — emit only once.
            let info_lhs: Vec<ObjectId> = if info.lhs.is_empty() {
                vec![v]
            } else {
                info.lhs.clone()
            };
            let representative = info_lhs[0];
            if !emitted.insert(representative) {
                continue; // initializer already emitted
            }
            let rhs = match info.init {
                Some(id) => id,
                None => continue, // defensive: hasInitializer implies init for vars
            };
            self.info
                .init_order
                .push(Initializer { lhs: info_lhs, rhs });
        }
    }

    /// The `nodeQueue.Less` ordering: constants before non-constants, then by
    /// ascending dependency count, then by ascending source order.
    fn node_less(&self, nodes: &[GraphNode], a: usize, b: usize) -> bool {
        let xa = nodes[a].obj;
        let xb = nodes[b].obj;

        // Prioritize all constants before non-constants (go.dev/issue/66575).
        let a_const = is_const(&self.objects, xa);
        let b_const = is_const(&self.objects, xb);
        if a_const != b_const {
            return a_const;
        }

        // Then by number of incoming dependencies, then by source order.
        let na = nodes[a].ndeps;
        let nb = nodes[b].ndeps;
        na < nb || (na == nb && xa.order(&self.objects) < xb.order(&self.objects))
    }

    /// Build the object dependency graph from `obj_map`, with function nodes
    /// removed. The result contains only constants and variables.
    ///
    /// Equivalent to `dependencyGraph`.
    fn dependency_graph(&self) -> Vec<GraphNode> {
        // M maps each dependency object to its node index.
        // Sort by source order so FxHash key iteration cannot change node indices
        // (PERF_TASKS_V2 §0-12 / A-1).
        let mut m: HashMap<ObjectId, usize> = HashMap::default();
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut objs: Vec<ObjectId> = self.obj_map.keys().copied().collect();
        objs.sort_by_key(|o| o.order(&self.objects));
        for obj in objs {
            if is_dependency(&self.objects, obj) {
                m.insert(obj, nodes.len());
                nodes.push(GraphNode {
                    obj,
                    pred: HashSet::default(),
                    succ: HashSet::default(),
                    ndeps: 0,
                });
            }
        }

        // Compute edges: for each dependency obj -> d, create n->s and s->n.
        // Sort for deterministic HashSet population order (not required for
        // correctness, but keeps cycle dumps stable across hashers).
        let mut edge_objs: Vec<ObjectId> = m.keys().copied().collect();
        edge_objs.sort_by_key(|o| o.order(&self.objects));
        for obj in edge_objs {
            let n_idx = m[&obj];
            if let Some(info) = self.obj_map.get(&obj) {
                let mut deps: Vec<ObjectId> = info.deps.keys().copied().collect();
                deps.sort_by_key(|o| o.order(&self.objects));
                for d in deps {
                    if let Some(&d_idx) = m.get(&d) {
                        nodes[n_idx].succ.insert(d_idx);
                        nodes[d_idx].pred.insert(n_idx);
                    }
                }
            }
        }

        // Separate function and non-function nodes.
        let mut func_g: Vec<usize> = Vec::new();
        let mut g: Vec<usize> = Vec::new();
        for i in 0..nodes.len() {
            if is_func(&self.objects, nodes[i].obj) {
                func_g.push(i);
            } else {
                g.push(i);
            }
        }

        // Remove function nodes, collecting remaining nodes in G. Mutually
        // recursive functions may form (permitted) cycles that would otherwise
        // inflate variable dependency counts. Remove high-cost functions last.
        func_g.sort_by_key(|&i| nodes[i].cost());
        for &n in &func_g {
            // Connect each predecessor p of n with each successor s, then drop
            // the edges to/from n.
            let preds: Vec<usize> = {
                let mut v: Vec<usize> = nodes[n].pred.iter().copied().collect();
                v.sort_unstable();
                v
            };
            let succs: Vec<usize> = {
                let mut v: Vec<usize> = nodes[n].succ.iter().copied().collect();
                v.sort_unstable();
                v
            };
            for &p in &preds {
                if p != n {
                    for &s in &succs {
                        if s != n {
                            nodes[p].succ.insert(s);
                            nodes[s].pred.insert(p);
                        }
                    }
                    nodes[p].succ.remove(&n);
                }
            }
            for &s in &succs {
                nodes[s].pred.remove(&n);
            }
        }

        // Build the result graph (non-function nodes only), renumbering so that
        // pred/succ reference positions within the new vector. We keep only G
        // nodes; all edges to/from function nodes have already been removed, so
        // every surviving edge points within G.
        let mut old_to_new: HashMap<usize, usize> = HashMap::default();
        for (new_idx, &old) in g.iter().enumerate() {
            old_to_new.insert(old, new_idx);
        }
        let mut result: Vec<GraphNode> = Vec::with_capacity(g.len());
        for &old in &g {
            let pred: HashSet<usize> = nodes[old]
                .pred
                .iter()
                .filter_map(|p| old_to_new.get(p).copied())
                .collect();
            let succ: HashSet<usize> = nodes[old]
                .succ
                .iter()
                .filter_map(|s| old_to_new.get(s).copied())
                .collect();
            let ndeps = succ.len() as i64;
            result.push(GraphNode {
                obj: nodes[old].obj,
                pred,
                succ,
                ndeps,
            });
        }
        result
    }

    /// Report an error for the given initialization cycle.
    ///
    /// Equivalent to `Checker.reportCycle`, but condensed into a single error
    /// message (D07: positions are bare `u32`s; we don't emit one `refers to`
    /// line per edge).
    fn report_cycle(&mut self, cycle: &[ObjectId]) {
        let obj = cycle[0];
        let pos = obj.pos(&self.objects);
        let name = obj.name(&self.objects).to_string();

        if cycle.len() == 1 {
            self.error(
                pos,
                Code::InvalidInitCycle,
                format!("initialization cycle: {name} refers to itself"),
            );
            return;
        }

        // "cycle[i] refers to cycle[j]" for (i,j) = (0,n-1), (n-1,n-2), ...,
        // (1,0). Build a single chain "a refers to b refers to ... refers to a".
        let mut chain = vec![name.clone()];
        for j in (0..cycle.len()).rev() {
            chain.push(cycle[j].name(&self.objects).to_string());
        }
        self.error(
            pos,
            Code::InvalidInitCycle,
            format!(
                "initialization cycle for {name}: {}",
                chain.join(" refers to ")
            ),
        );
    }
}

/// Return the (reversed) list of objects `[to, ..., from]` such that there is a
/// path of object dependencies from `from` to `to`, or an empty vector if there
/// is no such path.
///
/// Equivalent to `findPath`.
fn find_path(
    obj_map: &HashMap<ObjectId, DeclInfo>,
    objects: &ObjectArena,
    from: ObjectId,
    to: ObjectId,
    seen: &mut HashSet<ObjectId>,
) -> Vec<ObjectId> {
    if !seen.insert(from) {
        return Vec::new();
    }

    // Sort deps for a deterministic result.
    let mut deps: Vec<ObjectId> = match obj_map.get(&from) {
        Some(info) => info.deps.keys().copied().collect(),
        None => Vec::new(),
    };
    deps.sort_by_key(|d| d.order(objects));

    for d in deps {
        if d == to {
            return vec![d];
        }
        let mut p = find_path(obj_map, objects, d, to, seen);
        if !p.is_empty() {
            p.push(d);
            return p;
        }
    }

    Vec::new()
}
