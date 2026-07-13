// Port of Go's go/token/tree.go to Rust.
//
// Original: Copyright 2025 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Self-balancing AVL tree keyed by Pos ranges. All entries must
// cover disjoint ranges. The implementation mirrors Go's reference
// (which in turn was simplified from rsc/omap), but uses an index
// arena (Vec<Option<Node>> + free list) instead of parent pointers,
// to keep the borrow checker happy without unsafe.

use std::sync::Arc;

use crate::position::File;

/// A key represents the Pos range of a File: [start, end].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub start: i64,
    pub end: i64,
}

impl Key {
    /// Construct a zero-width key that searches for a single Pos value.
    pub(crate) fn point(p: i64) -> Self {
        Key { start: p, end: p }
    }
}

/// `compare_key` reports whether x is before y (-1), after y (+1),
/// or overlapping y (0). Total order so long as the keys are disjoint.
pub(crate) fn compare_key(x: Key, y: Key) -> i32 {
    if x.end < y.start {
        -1
    } else if y.end < x.start {
        1
    } else {
        0
    }
}

/// Opaque index into the tree's node arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NodeIdx(pub usize);

pub(crate) struct Node {
    parent: Option<NodeIdx>,
    left: Option<NodeIdx>,
    right: Option<NodeIdx>,
    file: Arc<File>,
    key: Key,
    balance: i32,
    height: i32,
}

pub(crate) struct Tree {
    nodes: Vec<Option<Node>>,
    free: Vec<usize>,
    root: Option<NodeIdx>,
}

impl Tree {
    pub(crate) fn new() -> Self {
        Tree {
            nodes: Vec::new(),
            free: Vec::new(),
            root: None,
        }
    }

    fn alloc(&mut self, node: Node) -> NodeIdx {
        if let Some(idx) = self.free.pop() {
            self.nodes[idx] = Some(node);
            NodeIdx(idx)
        } else {
            self.nodes.push(Some(node));
            NodeIdx(self.nodes.len() - 1)
        }
    }

    fn free_slot(&mut self, idx: NodeIdx) {
        self.nodes[idx.0] = None;
        self.free.push(idx.0);
    }

    fn node(&self, idx: NodeIdx) -> &Node {
        self.nodes[idx.0]
            .as_ref()
            .expect("node index points to free slot")
    }

    fn node_mut(&mut self, idx: NodeIdx) -> &mut Node {
        self.nodes[idx.0]
            .as_mut()
            .expect("node index points to free slot")
    }

    pub(crate) fn file_at(&self, idx: NodeIdx) -> &Arc<File> {
        &self.node(idx).file
    }

    /// Locate the node identified by `k`. Returns `(found, parent)`:
    /// `found` is `Some(idx)` if a node with that key exists; otherwise
    /// `parent` is the node beneath which a fresh node should be linked.
    pub(crate) fn locate(&self, k: Key) -> (Option<NodeIdx>, Option<NodeIdx>) {
        let mut x = self.root;
        let mut parent: Option<NodeIdx> = None;
        let mut last_sign = 0;
        while let Some(idx) = x {
            let n = self.node(idx);
            let sign = compare_key(k, n.key);
            if sign < 0 {
                parent = Some(idx);
                last_sign = sign;
                x = n.left;
            } else if sign > 0 {
                parent = Some(idx);
                last_sign = sign;
                x = n.right;
            } else {
                return (Some(idx), parent);
            }
        }
        let _ = last_sign;
        (None, parent)
    }

    /// Insert `file` into the tree. Panics on a key overlap with a
    /// *different* File. A second insert of the same `Arc<File>` is a
    /// no-op (identity is checked via `Arc::ptr_eq`).
    pub(crate) fn add(&mut self, file: Arc<File>) {
        let k = file_key(&file);
        let (found, parent) = self.locate(k);
        if let Some(idx) = found {
            let existing = &self.node(idx).file;
            if Arc::ptr_eq(existing, &file) {
                return;
            }
            panic!(
                "file {} ({}-{}) overlaps with file {} ({}-{})",
                existing.name(),
                existing.base(),
                existing.end_pos(),
                file.name(),
                file.base(),
                file.end_pos(),
            );
        }
        self.insert(file, k, parent);
    }

    fn insert(&mut self, file: Arc<File>, k: Key, parent: Option<NodeIdx>) {
        let new_idx = self.alloc(Node {
            parent,
            left: None,
            right: None,
            file,
            key: k,
            balance: 0,
            // height -1 mirrors Go's deliberate "stale" value that
            // rebalance_up will normalize.
            height: -1,
        });
        match parent {
            None => self.root = Some(new_idx),
            Some(p) => {
                let pkey = self.node(p).key;
                if compare_key(k, pkey) < 0 {
                    self.node_mut(p).left = Some(new_idx);
                } else {
                    self.node_mut(p).right = Some(new_idx);
                }
            }
        }
        self.rebalance_up(Some(new_idx));
    }

    /// Delete the node at `idx`. Panics if `idx` is not currently in the
    /// tree.
    pub(crate) fn delete(&mut self, idx: NodeIdx) {
        let left = self.node(idx).left;
        let right = self.node(idx).right;
        let parent = self.node(idx).parent;

        match (left, right) {
            (None, _) => {
                // Replace idx with its right child.
                self.replace_in_parent(parent, idx, right);
                if let Some(r) = right {
                    self.node_mut(r).parent = parent;
                }
                self.free_slot(idx);
                self.rebalance_up(parent);
            }
            (Some(l), None) => {
                self.replace_in_parent(parent, idx, Some(l));
                self.node_mut(l).parent = parent;
                self.free_slot(idx);
                self.rebalance_up(parent);
            }
            (Some(_l), Some(_r)) => {
                self.delete_swap(idx);
            }
        }
    }

    fn delete_swap(&mut self, x_idx: NodeIdx) {
        // Find in-order successor (leftmost of right subtree) and detach it.
        let right = self
            .node(x_idx)
            .right
            .expect("delete_swap needs right child");
        let z_idx = self.delete_min(right);

        // After detaching z, z.parent points to where z used to live;
        // that's the lowest potentially unbalanced node.
        let unbalanced = if self.node(z_idx).parent == Some(x_idx) {
            // (x a (z nil b)) -> (z a b)
            z_idx
        } else {
            self.node(z_idx).parent.expect("z must have a parent")
        };

        // Copy x's links/heights into z.
        let x_parent = self.node(x_idx).parent;
        let x_height = self.node(x_idx).height;
        let x_balance = self.node(x_idx).balance;
        let x_left = self.node(x_idx).left;
        let x_right = self.node(x_idx).right;

        // Replace x with z in x's parent (or root).
        self.replace_in_parent(x_parent, x_idx, Some(z_idx));

        {
            let z = self.node_mut(z_idx);
            z.parent = x_parent;
            z.height = x_height;
            z.balance = x_balance;
            z.left = x_left;
            z.right = x_right;
        }
        if let Some(l) = x_left {
            self.node_mut(l).parent = Some(z_idx);
        }
        if let Some(r) = x_right {
            // x_right may equal z_idx if z was x's immediate right child;
            // in that case we've already overwritten z's right above.
            if r != z_idx {
                self.node_mut(r).parent = Some(z_idx);
            }
        }

        self.free_slot(x_idx);
        self.rebalance_up(Some(unbalanced));
    }

    /// Detach and return the leftmost descendant of `root_idx`. The
    /// returned node is unlinked from its parent; the rest of the
    /// subtree is rewired to skip over it.
    fn delete_min(&mut self, root_idx: NodeIdx) -> NodeIdx {
        let mut z_idx = root_idx;
        while let Some(l) = self.node(z_idx).left {
            z_idx = l;
        }
        let z_parent = self.node(z_idx).parent;
        let z_right = self.node(z_idx).right;

        // Replace z with z.right in z's parent.
        if let Some(p) = z_parent {
            if self.node(p).left == Some(z_idx) {
                self.node_mut(p).left = z_right;
            } else if self.node(p).right == Some(z_idx) {
                self.node_mut(p).right = z_right;
            }
        }
        if let Some(r) = z_right {
            self.node_mut(r).parent = z_parent;
        }
        z_idx
    }

    fn replace_in_parent(&mut self, parent: Option<NodeIdx>, old: NodeIdx, new: Option<NodeIdx>) {
        match parent {
            None => {
                assert_eq!(self.root, Some(old), "corrupt tree: missing root");
                self.root = new;
            }
            Some(p) => {
                let pnode = self.node_mut(p);
                if pnode.left == Some(old) {
                    pnode.left = new;
                } else if pnode.right == Some(old) {
                    pnode.right = new;
                } else {
                    panic!("corrupt tree: node not child of given parent");
                }
            }
        }
    }

    fn safe_height(&self, idx: Option<NodeIdx>) -> i32 {
        match idx {
            None => -1,
            Some(i) => self.node(i).height,
        }
    }

    fn update(&mut self, idx: NodeIdx) {
        let lh = self.safe_height(self.node(idx).left);
        let rh = self.safe_height(self.node(idx).right);
        let n = self.node_mut(idx);
        n.height = lh.max(rh) + 1;
        n.balance = rh - lh;
    }

    fn rotate_right(&mut self, y_idx: NodeIdx) -> NodeIdx {
        // p -> (y (x a b) c) becomes p -> (x a (y b c))
        let p = self.node(y_idx).parent;
        let x_idx = self
            .node(y_idx)
            .left
            .expect("rotate_right needs left child");
        let b = self.node(x_idx).right;

        // x.right = y
        self.node_mut(x_idx).right = Some(y_idx);
        self.node_mut(y_idx).parent = Some(x_idx);
        // y.left = b
        self.node_mut(y_idx).left = b;
        if let Some(b_idx) = b {
            self.node_mut(b_idx).parent = Some(y_idx);
        }
        // replace y with x under p
        self.replace_in_parent(p, y_idx, Some(x_idx));
        self.node_mut(x_idx).parent = p;

        self.update(y_idx);
        self.update(x_idx);
        x_idx
    }

    fn rotate_left(&mut self, x_idx: NodeIdx) -> NodeIdx {
        let p = self.node(x_idx).parent;
        let y_idx = self
            .node(x_idx)
            .right
            .expect("rotate_left needs right child");
        let b = self.node(y_idx).left;

        self.node_mut(y_idx).left = Some(x_idx);
        self.node_mut(x_idx).parent = Some(y_idx);
        self.node_mut(x_idx).right = b;
        if let Some(b_idx) = b {
            self.node_mut(b_idx).parent = Some(x_idx);
        }
        self.replace_in_parent(p, x_idx, Some(y_idx));
        self.node_mut(y_idx).parent = p;

        self.update(x_idx);
        self.update(y_idx);
        y_idx
    }

    fn rebalance_up(&mut self, mut x: Option<NodeIdx>) {
        while let Some(idx) = x {
            let h_before = self.node(idx).height;
            self.update(idx);
            let mut cur = idx;
            match self.node(cur).balance {
                -2 => {
                    let left = self.node(cur).left.expect("balance -2 needs left");
                    if self.node(left).balance == 1 {
                        self.rotate_left(left);
                    }
                    cur = self.rotate_right(cur);
                }
                2 => {
                    let right = self.node(cur).right.expect("balance +2 needs right");
                    if self.node(right).balance == -1 {
                        self.rotate_right(right);
                    }
                    cur = self.rotate_left(cur);
                }
                _ => {}
            }
            if self.node(cur).height == h_before {
                return;
            }
            x = self.node(cur).parent;
        }
    }

    /// In-order successor of `x`.
    fn next_of(&self, x: NodeIdx) -> Option<NodeIdx> {
        if let Some(mut r) = self.node(x).right {
            while let Some(l) = self.node(r).left {
                r = l;
            }
            return Some(r);
        }
        let mut cur = x;
        loop {
            let parent = self.node(cur).parent?;
            if self.node(parent).right == Some(cur) {
                cur = parent;
                continue;
            }
            return Some(parent);
        }
    }

    fn leftmost(&self) -> Option<NodeIdx> {
        let mut x = self.root?;
        while let Some(l) = self.node(x).left {
            x = l;
        }
        Some(x)
    }

    /// Snapshot of all files in ascending key order.
    pub(crate) fn all(&self) -> Vec<Arc<File>> {
        let mut out = Vec::new();
        let mut cur = self.leftmost();
        while let Some(idx) = cur {
            out.push(self.node(idx).file.clone());
            cur = self.next_of(idx);
        }
        out
    }
}

/// Compute the key of `file` (its Pos range).
pub(crate) fn file_key(file: &File) -> Key {
    Key {
        start: file.base(),
        end: file.base() + file.size(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::File;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::Arc;

    // Minimal reproducible PRNG so tests don't depend on `rand`.
    // Splitmix64 produces decent-quality 64-bit values for shuffling.
    struct SplitMix64 {
        state: u64,
    }

    impl SplitMix64 {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
            z ^ (z >> 31)
        }
        fn next_usize(&mut self, n: usize) -> usize {
            (self.next_u64() % (n as u64)) as usize
        }
        fn shuffle<T>(&mut self, slice: &mut [T]) {
            // Fisher-Yates
            for i in (1..slice.len()).rev() {
                let j = self.next_usize(i + 1);
                slice.swap(i, j);
            }
        }
    }

    fn random_seed() -> u64 {
        // Seed from time + a hashed identity to avoid identical runs.
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xDEAD_BEEF);
        let mut h = DefaultHasher::new();
        now.hash(&mut h);
        h.finish()
    }

    /// Mirrors Go's TestTree: AVL tree end-to-end exercise.
    #[test]
    fn test_tree() {
        let seed = random_seed();
        println!("random seed: {}", seed);
        let mut rng = SplitMix64::new(seed);

        // Create files with sequential, non-overlapping ranges.
        let mut files: Vec<Option<Arc<File>>> = (0..500)
            .scan(0i64, |base, _i| {
                *base += 1;
                let size = 1000i64;
                let f = File::new_for_test("".to_string(), *base, size);
                *base += size;
                Some(Some(f))
            })
            .collect();

        // Add them all in random order.
        let mut tree = Tree::new();
        {
            let mut shuffled: Vec<Arc<File>> = files.iter().filter_map(|f| f.clone()).collect();
            rng.shuffle(&mut shuffled);
            for f in shuffled {
                tree.add(f);
            }
        }

        // Randomly delete 100 entries.
        for _ in 0..100 {
            let i = rng.next_usize(files.len());
            let Some(file) = files[i].clone() else {
                continue;
            };
            files[i] = None;

            let (found, _) = tree.locate(file_key(&file));
            let idx = found.expect("locate must find existing file");
            assert!(
                Arc::ptr_eq(tree.file_at(idx), &file),
                "locate returned wrong file"
            );
            tree.delete(idx);
        }

        // Check point lookups within each surviving file.
        for slot in files.iter() {
            let Some(file) = slot.as_ref() else { continue };
            for &pos in &[
                file.base(),
                file.base() + file.size() / 2,
                file.base() + file.size(),
            ] {
                let (found, _) = tree.locate(Key::point(pos));
                let idx = found.expect("point lookup must find file");
                assert!(
                    Arc::ptr_eq(tree.file_at(idx), file),
                    "lookup {}@{} returned wrong file",
                    file.name(),
                    pos
                );
            }
        }

        // Check the in-order sequence matches.
        let alive: Vec<Arc<File>> = files.into_iter().flatten().collect();
        let collected = tree.all();
        assert_eq!(collected.len(), alive.len(), "tree.all length mismatch");
        for (a, b) in collected.iter().zip(alive.iter()) {
            assert!(Arc::ptr_eq(a, b), "tree.all order mismatch");
        }
    }
}
