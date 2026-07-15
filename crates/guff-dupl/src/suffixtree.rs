//! Suffix tree for clone detection (port of `suffixtree/suffixtree.go` + `dupl.go`).

use std::collections::BTreeMap;

const INFINITY: i32 = i32::MAX;

/// Position in the token data slice.
pub type Pos = i32;

pub trait Token {
    fn val(&self) -> i32;
}

#[derive(Debug, Clone)]
pub struct SuffixMatch {
    pub ps: Vec<Pos>,
    pub len: Pos,
}

struct PosList {
    positions: Vec<Pos>,
}

impl PosList {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
        }
    }

    fn append(&mut self, other: &PosList) {
        self.positions.extend_from_slice(&other.positions);
    }

    fn add(&mut self, pos: Pos) {
        self.positions.push(pos);
    }
}

struct ContextList {
    lists: BTreeMap<i32, PosList>,
}

impl ContextList {
    fn new() -> Self {
        Self {
            lists: BTreeMap::new(),
        }
    }

    fn get_all(&self) -> Vec<Pos> {
        let mut ps = Vec::new();
        for pl in self.lists.values() {
            ps.extend_from_slice(&pl.positions);
        }
        ps
    }

    fn append(&mut self, other: &ContextList) {
        for (lc, pl) in &other.lists {
            self.lists.entry(*lc).or_default().append(pl);
        }
    }
}

impl Default for PosList {
    fn default() -> Self {
        Self::new()
    }
}

/// Suffix tree over a stream of tokens.
pub struct STree<T: Token> {
    data: Vec<T>,
    root: StateId,
    aux_state: StateId,
    s: StateId,
    start: Pos,
    end: Pos,
    states: Vec<State<T>>,
}

struct State<T: Token> {
    trans: Vec<Tran<T>>,
    link_state: Option<StateId>,
}

type StateId = usize;

struct Tran<T: Token> {
    start: Pos,
    end: Pos,
    state: StateId,
    _marker: std::marker::PhantomData<T>,
}

impl<T: Token + Clone> STree<T> {
    pub fn new() -> Self {
        let mut tree = Self {
            data: Vec::new(),
            root: 0,
            aux_state: 0,
            s: 0,
            start: 0,
            end: 0,
            states: Vec::new(),
        };
        tree.root = tree.new_state();
        tree.aux_state = tree.new_state();
        tree.states[tree.root].link_state = Some(tree.aux_state);
        tree.s = tree.root;
        tree
    }

    fn new_state(&mut self) -> StateId {
        let id = self.states.len();
        self.states.push(State {
            trans: Vec::new(),
            link_state: None,
        });
        id
    }

    /// Append tokens and extend the suffix tree.
    pub fn update(&mut self, tokens: impl IntoIterator<Item = T>) {
        for token in tokens {
            self.data.push(token);
            self.update_one();
            let (s, start) = self.canonize(self.s, self.start, self.end);
            self.s = s;
            self.start = start;
            self.end += 1;
        }
    }

    fn update_one(&mut self) {
        let mut oldr = self.root;
        let mut s = self.s;
        let mut start = self.start;
        let end = self.end;
        let mut r = self.root;

        loop {
            let (nr, end_point) = self.test_and_split(s, start, end - 1);
            r = nr;
            if end_point {
                break;
            }
            self.fork(r, end);
            if oldr != self.root {
                self.states[oldr].link_state = Some(r);
            }
            oldr = r;
            let link = self.states[s].link_state.unwrap_or(self.root);
            let (ns, nstart) = self.canonize(link, start, end - 1);
            s = ns;
            start = nstart;
        }
        if oldr != self.root {
            self.states[oldr].link_state = Some(r);
        }
        self.s = s;
        self.start = start;
    }

    fn test_and_split(&mut self, s: StateId, start: Pos, end: Pos) -> (StateId, bool) {
        let c_val = self.data[self.end as usize].val();
        if start <= end {
            let tr_idx = self.find_tran_idx(s, self.data[start as usize].val());
            let tr = &self.states[s].trans[tr_idx];
            let split_point = tr.start + end - start + 1;
            if self.data[split_point as usize].val() == c_val {
                return (s, true);
            }
            let new_st = self.new_state();
            let (tr_start, tr_end, tr_state) = {
                let tr = &mut self.states[s].trans[tr_idx];
                let ts = tr.start;
                let te = tr.end;
                let st = tr.state;
                tr.end = split_point - 1;
                tr.state = new_st;
                (ts, te, st)
            };
            self.states[new_st].trans.push(Tran {
                start: split_point,
                end: tr_end,
                state: tr_state,
                _marker: std::marker::PhantomData,
            });
            return (new_st, false);
        }
        if s == self.aux_state || self.find_tran(s, c_val).is_some() {
            return (s, true);
        }
        (s, false)
    }

    fn canonize(&self, mut s: StateId, mut start: Pos, end: Pos) -> (StateId, Pos) {
        if s == self.aux_state {
            s = self.root;
            start += 1;
        }
        if start > end || start as usize >= self.data.len() {
            return (s, start);
        }
        loop {
            if start as usize >= self.data.len() {
                break;
            }
            let tr = self
                .find_tran(s, self.data[start as usize].val())
                .expect("transition should exist");
            if tr.end - tr.start > end - start {
                break;
            }
            start += tr.end - tr.start + 1;
            s = tr.state;
            if start > end {
                break;
            }
        }
        (s, start)
    }

    fn fork(&mut self, s: StateId, i: Pos) {
        let r = self.new_state();
        self.states[s].trans.push(Tran {
            start: i,
            end: INFINITY,
            state: r,
            _marker: std::marker::PhantomData,
        });
    }

    fn find_tran_idx(&self, s: StateId, val: i32) -> usize {
        self.states[s]
            .trans
            .iter()
            .position(|t| self.data[t.start as usize].val() == val)
            .expect("transition should exist")
    }

    fn find_tran(&self, s: StateId, val: i32) -> Option<&Tran<T>> {
        self.states[s]
            .trans
            .iter()
            .find(|t| self.data[t.start as usize].val() == val)
    }

    fn tran_len(&self, t: &Tran<T>) -> i64 {
        let act_end = self.tran_act_end(t) as i64;
        act_end - t.start as i64 + 1
    }

    fn tran_act_end(&self, t: &Tran<T>) -> Pos {
        if t.end == INFINITY {
            Pos::from(self.data.len() as i32 - 1)
        } else {
            t.end
        }
    }

    /// Find duplicate sequences at least `threshold` tokens long.
    pub fn find_dupl_over(&self, threshold: i32) -> Vec<SuffixMatch> {
        let mut out = Vec::new();
        let aux = Tran {
            start: 0,
            end: 0,
            state: self.root,
            _marker: std::marker::PhantomData,
        };
        walk_trans(self, &aux, 0, threshold, &mut out);
        out
    }
}

fn walk_trans<T: Token + Clone>(
    tree: &STree<T>,
    parent: &Tran<T>,
    length: i64,
    threshold: i32,
    out: &mut Vec<SuffixMatch>,
) -> ContextList {
    let s = parent.state;
    let mut cl = ContextList::new();
    let threshold = threshold as i64;

    if tree.states[s].trans.is_empty() {
        let mut pl = PosList::new();
        let act_end = if parent.end == INFINITY {
            tree.data.len() as i32 - 1
        } else {
            parent.end
        };
        let start = act_end + 1 - length as i32;
        pl.add(start);
        let ch = if start > 0 {
            tree.data[(start - 1) as usize].val()
        } else {
            0
        };
        cl.lists.insert(ch, pl);
        return cl;
    }

    for tr in &tree.states[s].trans {
        let ln = length + tree.tran_len(tr);
        let cl2 = walk_trans(tree, tr, ln, threshold as i32, out);
        if ln >= threshold {
            cl.append(&cl2);
        }
    }

    if length >= threshold && cl.lists.len() > 1 {
        let mut ps = cl.get_all();
        ps.sort_unstable();
        out.push(SuffixMatch {
            ps,
            len: length as i32,
        });
    }
    cl
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct CharTok(u8);

    impl Token for CharTok {
        fn val(&self) -> i32 {
            self.0 as i32
        }
    }

    #[test]
    fn construction_cacao() {
        let mut t = STree::new();
        for &b in b"cacao" {
            t.update(std::iter::once(CharTok(b)));
        }
    }

    #[test]
    fn finds_abab_duplicate() {
        let mut t = STree::new();
        for &b in b"abab$" {
            t.update(std::iter::once(CharTok(b)));
        }
        let matches = t.find_dupl_over(2);
        assert!(
            matches.iter().any(|m| m.len == 2 && m.ps.len() >= 2),
            "{matches:?}"
        );
    }
}
