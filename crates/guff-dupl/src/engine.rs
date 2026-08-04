//! Clone detection engine (port of `lib/lib.go`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;

use guff::ast::File;
use guff::position::FileSet;

use crate::golang::transform_file;
use crate::suffixtree::STree;
use crate::syntax::{self, SyntaxNode};

/// golangci-lint default (`linters.settings.dupl.threshold`).
pub const DEFAULT_THRESHOLD: i32 = 150;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneLoc {
    pub filename: String,
    pub line_start: i32,
    pub line_end: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplIssue {
    pub from: CloneLoc,
    pub to: CloneLoc,
}

/// Run dupl on the given source files.
pub fn run(paths: &[&Path], threshold: i32) -> Result<Vec<DuplIssue>, std::io::Error> {
    let mut data: Vec<SyntaxNode> = Vec::new();
    let mut stream: Vec<usize> = Vec::new();
    let mut tree = STree::new();

    for path in paths {
        let src = fs::read_to_string(path)?;
        let fset = FileSet::new();
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("input.go");
        let file = guff::parser::parse_file(&fset, name, src.as_bytes(), guff::parser::Mode::NONE)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        ingest_file(&mut data, &mut stream, &mut tree, &fset, path, &file);
    }

    let sentinel = SyntaxNode::new(-1, String::new(), 0, 0);
    let sentinel_idx = data.len();
    data.push(sentinel);
    stream.push(sentinel_idx);
    tree.update(std::iter::once(data[sentinel_idx].clone()));

    let mut groups: BTreeMap<Vec<u8>, Vec<Vec<usize>>> = BTreeMap::new();
    for m in tree.find_dupl_over(threshold) {
        let sm = syntax::find_syntax_units(&data, &stream, &m, threshold);
        if sm.frags.is_empty() {
            continue;
        }
        let hash = sm.hash;
        let entry = groups.entry(hash).or_default();
        for frag in sm.frags {
            entry.push(frag);
        }
    }

    let mut issues = Vec::new();
    for frags in groups.into_values() {
        let uniq = unique_frags(&data, frags);
        if uniq.len() <= 1 {
            continue;
        }
        let clones = prepare_clones(&data, &uniq)?;
        for i in 0..clones.len() {
            let from = clones[i].clone();
            let to = clones[(i + 1) % clones.len()].clone();
            issues.push(DuplIssue { from, to });
        }
    }
    Ok(issues)
}

fn ingest_file(
    data: &mut Vec<SyntaxNode>,
    stream: &mut Vec<usize>,
    tree: &mut STree<SyntaxNode>,
    fset: &FileSet,
    path: &Path,
    file: &File,
) {
    let filename = path.to_string_lossy().into_owned();
    let mut root = transform_file(fset, &filename, file);
    let base = data.len();
    let local_stream = syntax::serialize(&mut root);
    append_tree(&root, data);
    for local_idx in local_stream {
        let global = base + local_idx;
        stream.push(global);
        tree.update(std::iter::once(data[global].clone()));
    }
}

fn append_tree(node: &SyntaxNode, out: &mut Vec<SyntaxNode>) -> usize {
    let idx = out.len();
    out.push(SyntaxNode {
        node_type: node.node_type,
        filename: node.filename.clone(),
        pos: node.pos,
        end: node.end,
        children: Vec::new(),
        owns: node.owns,
    });
    let child_clones: Vec<SyntaxNode> = node
        .children
        .iter()
        .map(|child| {
            let child_idx = append_tree(child, out);
            out[child_idx].clone()
        })
        .collect();
    out[idx].children = child_clones;
    idx
}

fn unique_frags(data: &[SyntaxNode], frags: Vec<Vec<usize>>) -> Vec<Vec<usize>> {
    let mut file_map: HashMap<String, HashSet<i32>> = HashMap::new();
    let mut out = Vec::new();
    for seq in frags {
        let node = &data[seq[0]];
        let file = file_map.entry(node.filename.clone()).or_default();
        if file.insert(node.pos) {
            out.push(seq);
        }
    }
    out
}

fn prepare_clones(data: &[SyntaxNode], frags: &[Vec<usize>]) -> Result<Vec<CloneLoc>, std::io::Error> {
    let mut clones = Vec::with_capacity(frags.len());
    for dup in frags {
        let nstart = &data[dup[0]];
        let nend = &data[dup[dup.len() - 1]];
        let file = fs::read_to_string(&nstart.filename)?;
        let (line_start, line_end) = block_lines(&file, nstart.pos, nend.end);
        clones.push(CloneLoc {
            filename: nstart.filename.clone(),
            line_start,
            line_end,
        });
    }
    clones.sort_by(|a, b| {
        a.filename
            .cmp(&b.filename)
            .then_with(|| a.line_start.cmp(&b.line_start))
    });
    Ok(clones)
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn run_on_ok_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/dupl/ok.go");
        let issues = run(&[path.as_path()], 30).expect("run");
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn run_on_bad_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/testdata/dupl/bad.go");
        let issues = run(&[path.as_path()], 30).expect("run");
        assert!(!issues.is_empty(), "{issues:?}");
        // Match mibk/dupl / golangci: whole FuncDecl spans, not inner if/for blocks.
        assert!(
            issues.iter().any(|i| {
                i.from.line_start == 3
                    && i.from.line_end == 34
                    && i.to.line_start == 36
                    && i.to.line_end == 67
            }),
            "expected FuncDecl ranges 3-34 / 36-67, got {issues:?}"
        );
    }
}

fn block_lines(file: &str, from: i32, to: i32) -> (i32, i32) {
    let mut line = 1i32;
    let mut line_start = 0i32;
    let mut line_end = 0i32;
    for (offset, b) in file.bytes().enumerate() {
        if b == b'\n' {
            line += 1;
        }
        if offset == from as usize {
            line_start = line;
        }
        if offset == (to - 1) as usize {
            line_end = line;
            break;
        }
    }
    (line_start, line_end)
}
