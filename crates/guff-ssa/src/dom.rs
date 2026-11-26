//! Dominance algorithms.

use crate::ids::BlockId;
use crate::function::Function;
use crate::arena::ArenaId;

/// DomInfo contains a BasicBlock's dominance information.
/// (Go: `domInfo`)
#[derive(Default, Debug, Clone)]
pub struct DomInfo {
    /// immediate dominator (parent in domtree)
    pub idom: Option<BlockId>,
    /// nodes immediately dominated by this one
    pub children: Vec<BlockId>,
    /// pre-order numbering within domtree
    pub pre: i32,
    /// post-order numbering within domtree
    pub post: i32,
}

impl DomInfo {
    pub fn new() -> Self {
        Self::default()
    }
}

struct LtState {
    // Each slice is indexed by b.index().
    sdom: Vec<Option<BlockId>>,
    parent: Vec<Option<BlockId>>,
    ancestor: Vec<Option<BlockId>>,
}

impl LtState {
    fn dfs(&mut self, f: &mut Function, v_id: BlockId, i: i32, preorder: &mut Vec<Option<BlockId>>) -> i32 {
        preorder[i as usize] = Some(v_id);
        f.blocks.get_mut(v_id).dom.pre = i; // For now: DFS preorder
        let mut i = i + 1;
        self.sdom[v_id.index()] = Some(v_id);
        self.link(None, v_id);

        let succs = f.blocks.get(v_id).succs.clone();
        for w_id in succs {
            if self.sdom[w_id.index()].is_none() {
                self.parent[w_id.index()] = Some(v_id);
                i = self.dfs(f, w_id, i, preorder);
            }
        }
        i
    }

    fn eval(&self, f: &Function, mut v_id: BlockId) -> BlockId {
        let mut u_id = v_id;
        while let Some(ancestor_id) = self.ancestor[v_id.index()] {
            let v_sdom_pre = f.blocks.get(self.sdom[v_id.index()].unwrap()).dom.pre;
            let u_sdom_pre = f.blocks.get(self.sdom[u_id.index()].unwrap()).dom.pre;
            if v_sdom_pre < u_sdom_pre {
                u_id = v_id;
            }
            v_id = ancestor_id;
        }
        u_id
    }

    fn link(&mut self, v_id: Option<BlockId>, w_id: BlockId) {
        self.ancestor[w_id.index()] = v_id;
    }
}

/// build_dom_tree computes the dominator tree of f using the LT algorithm.
/// Precondition: all blocks are reachable (e.g. optimize_blocks has been run).
/// (Go: `buildDomTree`)
pub fn build_dom_tree(f: &mut Function) {
    if f.blocks.is_empty() {
        return;
    }

    // Clear any previous domInfo.
    for b in f.blocks.values_mut() {
        b.dom = DomInfo::new();
    }

    let n = f.blocks.len();
    // Arena might have gaps, but ssa blocks are usually dense.
    // Using max index to be safe for vector indexing.
    let max_idx = f.blocks.iter().map(|(id, _)| id.index()).max().unwrap_or(0) + 1;

    let mut lt = LtState {
        sdom: vec![None; max_idx],
        parent: vec![None; max_idx],
        ancestor: vec![None; max_idx],
    };

    let mut preorder = vec![None; n];
    let root_id = f.blocks.iter().next().map(|(id, _)| id).unwrap();

    let prenum = lt.dfs(f, root_id, 0, &mut preorder);
    // TODO: f.Recover support if added later.

    let mut buckets = preorder.clone();

    // In reverse preorder...
    for i in (1..prenum).rev() {
        let w_id = preorder[i as usize].unwrap();

        // Step 3. Implicitly define the immediate dominator of each node.
        let mut v_id = buckets[i as usize].unwrap();
        while v_id != w_id {
            let next_v_id = buckets[f.blocks.get(v_id).dom.pre as usize].unwrap();
            let u_id = lt.eval(f, v_id);
            let u_sdom_pre = f.blocks.get(lt.sdom[u_id.index()].unwrap()).dom.pre;
            if u_sdom_pre < i {
                f.blocks.get_mut(v_id).dom.idom = Some(u_id);
            } else {
                f.blocks.get_mut(v_id).dom.idom = Some(w_id);
            }
            v_id = next_v_id;
        }

        // Step 2. Compute the semidominators of all nodes.
        let parent_id = lt.parent[w_id.index()].unwrap();
        lt.sdom[w_id.index()] = Some(parent_id);
        
        let preds = f.blocks.get(w_id).preds.clone();
        for v_id in preds {
            // Pred might be unreachable, in which case it wasn't visited by DFS
            // and sdom[v_id.index()] is None.
            if lt.sdom[v_id.index()].is_some() {
                let u_id = lt.eval(f, v_id);
                let u_sdom_pre = f.blocks.get(lt.sdom[u_id.index()].unwrap()).dom.pre;
                let w_sdom_pre = f.blocks.get(lt.sdom[w_id.index()].unwrap()).dom.pre;
                if u_sdom_pre < w_sdom_pre {
                    lt.sdom[w_id.index()] = lt.sdom[u_id.index()];
                }
            }
        }

        lt.link(Some(parent_id), w_id);

        if Some(parent_id) == lt.sdom[w_id.index()] {
            f.blocks.get_mut(w_id).dom.idom = Some(parent_id);
        } else {
            let sdom_pre = f.blocks.get(lt.sdom[w_id.index()].unwrap()).dom.pre as usize;
            buckets[i as usize] = buckets[sdom_pre];
            buckets[sdom_pre] = Some(w_id);
        }
    }

    // The final 'Step 3' is now outside the loop.
    let mut v_id = buckets[0].unwrap();
    while v_id != root_id {
        let next_v_id = buckets[f.blocks.get(v_id).dom.pre as usize].unwrap();
        f.blocks.get_mut(v_id).dom.idom = Some(root_id);
        v_id = next_v_id;
    }

    // Step 4. Explicitly define the immediate dominator of each node, in preorder.
    for i in 1..prenum {
        let w_id = preorder[i as usize].unwrap();
        let sdom_id = lt.sdom[w_id.index()].unwrap();
        if f.blocks.get(w_id).dom.idom.unwrap() != sdom_id {
            let idom_id = f.blocks.get(w_id).dom.idom.unwrap();
            let new_idom_id = f.blocks.get(idom_id).dom.idom.unwrap();
            f.blocks.get_mut(w_id).dom.idom = Some(new_idom_id);
        }
        
        let idom_id = f.blocks.get(w_id).dom.idom.unwrap();
        f.blocks.get_mut(idom_id).dom.children.push(w_id);
    }

    number_dom_tree(f, root_id, 0, 0);
}

fn number_dom_tree(f: &mut Function, v_id: BlockId, pre: i32, post: i32) -> (i32, i32) {
    let mut pre = pre;
    let mut post = post;
    f.blocks.get_mut(v_id).dom.pre = pre;
    pre += 1;
    
    let children = f.blocks.get(v_id).dom.children.clone();
    for child_id in children {
        let (new_pre, new_post) = number_dom_tree(f, child_id, pre, post);
        pre = new_pre;
        post = new_post;
    }
    f.blocks.get_mut(v_id).dom.post = post;
    post += 1;
    (pre, post)
}

/// DomFrontier maps each block to the set of blocks in its dominance frontier.
/// (Go: `domFrontier`)
pub struct DomFrontier {
    // Indexed by BlockId.index()
    pub frontier: Vec<Vec<BlockId>>,
}

impl DomFrontier {
    pub fn build(f: &Function) -> Self {
        let max_idx = f.blocks.iter().map(|(id, _)| id.index()).max().unwrap_or(0) + 1;
        let mut df = DomFrontier {
            frontier: vec![vec![]; max_idx],
        };
        if !f.blocks.is_empty() {
            let root_id = f.blocks.iter().next().map(|(id, _)| id).unwrap();
            df.build_recursive(f, root_id);
            // TODO: f.Recover
        }
        df
    }

    fn build_recursive(&mut self, f: &Function, u_id: BlockId) {
        let u = f.blocks.get(u_id);
        for &child_id in &u.dom.children {
            self.build_recursive(f, child_id);
        }
        for &vb_id in &u.succs {
            let v = f.blocks.get(vb_id);
            if v.dom.idom != Some(u_id) {
                self.add(u_id, vb_id);
            }
        }
        let children = u.dom.children.clone(); // avoid borrow conflict
        for w_id in children {
            for vb_id in self.frontier[w_id.index()].clone() {
                let v = f.blocks.get(vb_id);
                if v.dom.idom != Some(u_id) {
                    self.add(u_id, vb_id);
                }
            }
        }
    }

    fn add(&mut self, u_id: BlockId, v_id: BlockId) {
        self.frontier[u_id.index()].push(v_id);
    }
}
