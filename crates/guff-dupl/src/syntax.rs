//! Serialized syntax stream and clone matching (port of `syntax/syntax.go`).

use sha1::{Digest, Sha1};

use crate::suffixtree::{SuffixMatch, Token};

/// To avoid stack overflow with gigantic composite literals.
const MAX_CHILDREN_SERIAL: usize = 10_000;

/// Uniform syntax tree node used for clone detection.
#[derive(Debug, Clone)]
pub struct SyntaxNode {
    pub node_type: i32,
    pub filename: String,
    pub pos: i32,
    pub end: i32,
    pub children: Vec<SyntaxNode>,
    /// Number of nodes owned by this subtree (excluding self), set by [`serialize`].
    pub owns: i32,
}

impl SyntaxNode {
    pub fn new(node_type: i32, filename: String, pos: i32, end: i32) -> Self {
        Self {
            node_type,
            filename,
            pos,
            end,
            children: Vec::new(),
            owns: 0,
        }
    }
}

impl Token for SyntaxNode {
    fn val(&self) -> i32 {
        self.node_type
    }
}

#[derive(Debug, Clone)]
pub struct SyntaxMatch {
    pub hash: Vec<u8>,
    pub frags: Vec<Vec<usize>>,
}

/// Flatten a syntax tree into a preorder stream and set `owns` counts.
pub fn serialize(root: &mut SyntaxNode) -> Vec<usize> {
    let mut stream = Vec::with_capacity(10);
    serial(root, &mut stream);
    stream
}

fn serial(node: &mut SyntaxNode, stream: &mut Vec<usize>) -> i32 {
    let idx = stream.len();
    stream.push(idx);
    let mut count = 0;
    for (i, child) in node.children.iter_mut().enumerate() {
        if i > MAX_CHILDREN_SERIAL {
            break;
        }
        count += serial(child, stream);
    }
    node.owns = count;
    count + 1
}

/// Find complete syntax units in a suffix-tree match group.
pub fn find_syntax_units(
    data: &[SyntaxNode],
    stream: &[usize],
    m: &SuffixMatch,
    threshold: i32,
) -> SyntaxMatch {
    if m.ps.is_empty() {
        return SyntaxMatch {
            hash: Vec::new(),
            frags: Vec::new(),
        };
    }
    let first_start = m.ps[0] as usize;
    let first_end = first_start + m.len as usize;
    if first_end > stream.len() {
        return SyntaxMatch {
            hash: Vec::new(),
            frags: Vec::new(),
        };
    }
    let first_seq: Vec<usize> = stream[first_start..first_end].to_vec();
    let mut indexes = get_units_indexes(&first_seq, data, threshold);

    let index_cnt = indexes.len();
    if index_cnt > 0 {
        let lasti = indexes[index_cnt - 1];
        let firstn_idx = first_seq[lasti];
        let firstn = &data[firstn_idx];
        for i in 1..m.ps.len() {
            let n_idx = stream[m.ps[i] as usize + lasti];
            let n = &data[n_idx];
            if firstn.owns != n.owns {
                indexes.truncate(index_cnt - 1);
                break;
            }
        }
    }
    if indexes.is_empty()
        || is_cyclic(&indexes, &first_seq, data)
        || spans_multiple_files(&indexes, &first_seq, data)
    {
        return SyntaxMatch {
            hash: Vec::new(),
            frags: Vec::new(),
        };
    }

    let mut frags = Vec::with_capacity(m.ps.len());
    for &pos in &m.ps {
        let mut frag = Vec::with_capacity(indexes.len());
        for &index in &indexes {
            frag.push(stream[pos as usize + index]);
        }
        frags.push(frag);
    }

    let last_index = indexes[indexes.len() - 1];
    let last_node_idx = first_seq[last_index];
    let last_node = &data[last_node_idx];
    let hash_end = last_index + 1 + last_node.owns as usize;
    let hash_nodes: Vec<usize> = first_seq[indexes[0]..hash_end].to_vec();
    let hash = hash_seq(&hash_nodes, data);

    SyntaxMatch { hash, frags }
}

fn get_units_indexes(node_seq: &[usize], data: &[SyntaxNode], threshold: i32) -> Vec<usize> {
    let mut indexes = Vec::new();
    let mut split = false;
    let mut i = 0;
    while i < node_seq.len() {
        let n = &data[node_seq[i]];
        if n.owns >= (node_seq.len() - i) as i32 - 1 {
            i += 1;
            split = true;
            continue;
        }
        if n.owns + 1 < threshold {
            split = true;
        } else {
            if split {
                indexes.clear();
                split = false;
            }
            indexes.push(i);
        }
        i += n.owns as usize + 1;
    }
    indexes
}

fn is_cyclic(indexes: &[usize], nodes: &[usize], data: &[SyntaxNode]) -> bool {
    let cnt = indexes.len();
    if cnt <= 1 {
        return false;
    }

    let mut alts: std::collections::HashSet<usize> = std::collections::HashSet::new();
    for i in 1..=cnt / 2 {
        if cnt % i == 0 {
            alts.insert(i);
        }
    }

    for i in 0..indexes[cnt / 2] {
        let nstart = &data[nodes[i + indexes[0]]];
        let mut to_remove = Vec::new();
        for &alt in &alts {
            let mut ok = true;
            let mut j = alt;
            while j < cnt {
                let index = i + indexes[j];
                if index < nodes.len() {
                    let nalt = &data[nodes[index]];
                    if nstart.owns == nalt.owns && nstart.node_type == nalt.node_type {
                        j += alt;
                        continue;
                    }
                } else if i >= indexes[alt] {
                    return true;
                }
                ok = false;
                break;
            }
            if !ok {
                to_remove.push(alt);
            }
        }
        for r in to_remove {
            alts.remove(&r);
        }
        if alts.is_empty() {
            return false;
        }
    }
    true
}

fn spans_multiple_files(indexes: &[usize], nodes: &[usize], data: &[SyntaxNode]) -> bool {
    if indexes.len() < 2 {
        return false;
    }
    let f = &data[nodes[indexes[0]]].filename;
    for i in 1..indexes.len() {
        if &data[nodes[indexes[i]]].filename != f {
            return true;
        }
    }
    false
}

fn hash_seq(nodes: &[usize], data: &[SyntaxNode]) -> Vec<u8> {
    let mut h = Sha1::new();
    let bytes: Vec<u8> = nodes.iter().map(|&idx| data[idx].node_type as u8).collect();
    h.update(bytes);
    h.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_type::{FILE, IDENT};

    #[test]
    fn serialize_sets_owns() {
        let mut root = SyntaxNode::new(FILE, "a.go".into(), 0, 100);
        root.children.push(SyntaxNode::new(IDENT, "a.go".into(), 1, 5));
        root.children.push(SyntaxNode::new(IDENT, "a.go".into(), 6, 10));
        let stream = serialize(&mut root);
        assert_eq!(stream.len(), 3);
        assert_eq!(root.owns, 2);
    }
}
