// Port of Go's go/ast/import.go to Rust.
//
// Original: Copyright 2011 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// `sort_imports` sorts runs of consecutive import lines within
// `import (...)` blocks of a [`File`] and removes adjacent duplicates
// (when safe — i.e. no comment on the survivor would be lost).
//
// Differences from Go:
//
// * Pass the FileSet as `&Arc<FileSet>` since our `position::FileSet`
//   is shared via `Arc`.
// * Re-use [`crate::directive::unquote`] to unquote import paths.

use std::cmp::Ordering;
use std::sync::Arc;

use crate::ast::{BasicLit, Decl, File, GenDecl, Spec};
use crate::directive::unquote;
use crate::position::{FileSet, Pos};
use crate::token::Token;

/// Sort runs of consecutive import lines in `f`'s import blocks and
/// remove safe duplicates. Also rebuilds `f.imports` so its order
/// matches the post-sort declarations.
pub fn sort_imports(fset: &Arc<FileSet>, f: &mut File) {
    for d in f.decls.iter_mut() {
        let decl = match d {
            Decl::GenDecl(g) if g.tok == Some(Token::IMPORT) => g,
            _ => break, // imports are always first; stop at non-import.
        };

        if !decl.lparen.is_valid() {
            // Not a block: trivially sorted.
            continue;
        }

        // Identify and sort runs of specs on successive lines.
        let mut new_specs: Vec<Spec> = Vec::with_capacity(decl.specs.len());
        let mut i = 0usize;
        for j in 0..decl.specs.len() {
            if j > i
                && line_at(fset, decl.specs[j].pos()) > 1 + line_at(fset, decl.specs[j - 1].end())
            {
                new_specs.extend(sort_specs(
                    fset,
                    f.comments.as_mut_slice(),
                    decl,
                    &decl.specs[i..j],
                ));
                i = j;
            }
        }
        new_specs.extend(sort_specs(
            fset,
            f.comments.as_mut_slice(),
            decl,
            &decl.specs[i..],
        ));
        decl.specs = new_specs;

        // Tidy any blank line between the last spec and the `)`.
        if let Some(last_spec) = decl.specs.last() {
            let last_line = line_at(fset, last_spec.pos());
            let mut r_paren_line = line_at(fset, decl.rparen);
            while r_paren_line > last_line + 1 {
                r_paren_line -= 1;
                if let Some(file) = fset.file(decl.rparen) {
                    file.merge_line(r_paren_line as usize);
                }
            }
        }
    }

    // Rebuild File.imports to match the sorted decls.
    f.imports.clear();
    for decl in &f.decls {
        if let Decl::GenDecl(g) = decl {
            if g.tok == Some(Token::IMPORT) {
                for s in &g.specs {
                    if let Spec::ImportSpec(is) = s {
                        f.imports.push(is.clone());
                    }
                }
            }
        }
    }
}

fn line_at(fset: &Arc<FileSet>, pos: Pos) -> i64 {
    fset.line_for(pos, false)
}

fn import_path(s: &Spec) -> String {
    match s {
        Spec::ImportSpec(is) => unquote(&is.path.value).unwrap_or_default(),
        _ => String::new(),
    }
}

fn import_name(s: &Spec) -> String {
    match s {
        Spec::ImportSpec(is) => is.name.as_ref().map(|n| n.name.clone()).unwrap_or_default(),
        _ => String::new(),
    }
}

fn import_comment(s: &Spec) -> String {
    match s {
        Spec::ImportSpec(is) => is.comment.as_ref().map(|cg| cg.text()).unwrap_or_default(),
        _ => String::new(),
    }
}

/// True iff `prev` may be removed in favor of `next` without data loss.
fn collapse(prev: &Spec, next: &Spec) -> bool {
    if import_path(next) != import_path(prev) || import_name(next) != import_name(prev) {
        return false;
    }
    matches!(prev, Spec::ImportSpec(is) if is.comment.is_none())
}

#[derive(Clone, Copy, Debug)]
struct PosSpan {
    start: Pos,
    end: Pos,
}

#[derive(Clone)]
struct CgPos {
    /// True iff the comment is *left of* the spec (block comment on
    /// the line above), as opposed to a trailing comment.
    left: bool,
    cg_index: usize,
}

fn sort_specs(
    fset: &Arc<FileSet>,
    comments: &mut [crate::ast::CommentGroup],
    decl: &GenDecl,
    specs: &[Spec],
) -> Vec<Spec> {
    if specs.len() <= 1 {
        return specs.to_vec();
    }

    // Snapshot the original positions before we move things around.
    let pos: Vec<PosSpan> = specs
        .iter()
        .map(|s| PosSpan {
            start: s.pos(),
            end: s.end(),
        })
        .collect();

    // Locate comments within the run's line range.
    let beg_specs = pos[0].start;
    let end_specs = pos.last().unwrap().end;
    let beg_file = fset.file(beg_specs).expect("file for begSpecs");
    let beg = beg_file.line_start(line_at(fset, beg_specs) as usize);
    let end_line = line_at(fset, end_specs);
    let end_file = fset.file(end_specs).expect("file for endSpecs");
    let end = if end_line as usize == end_file.line_count() {
        end_specs
    } else {
        end_file.line_start(end_line as usize + 1)
    };

    let mut first = comments.len();
    let mut last: isize = -1;
    for (i, g) in comments.iter().enumerate() {
        if g.end() >= end {
            break;
        }
        if beg <= g.pos() {
            if i < first {
                first = i;
            }
            if i as isize > last {
                last = i as isize;
            }
        }
    }

    let comment_range = if last >= 0 {
        Some((first, last as usize))
    } else {
        None
    };

    // Assign comments → specs by line.
    let mut import_comments: Vec<Vec<CgPos>> = vec![Vec::new(); specs.len()];
    if let Some((lo, hi)) = comment_range {
        let mut spec_index = 0usize;
        for ci in lo..=hi {
            let g = &comments[ci];
            while spec_index + 1 < specs.len() && pos[spec_index + 1].start <= g.pos() {
                spec_index += 1;
            }
            let mut left = false;
            if spec_index == 0 && pos[spec_index].start > g.pos() {
                left = true;
            } else if spec_index + 1 < specs.len()
                && line_at(fset, pos[spec_index].start) + 1 == line_at(fset, g.pos())
            {
                spec_index += 1;
                left = true;
            }
            import_comments[spec_index].push(CgPos { left, cg_index: ci });
        }
    }

    // Sort by (import_path, import_name, import_comment).
    let mut indices: Vec<usize> = (0..specs.len()).collect();
    indices.sort_by(|&a, &b| {
        let r = import_path(&specs[a]).cmp(&import_path(&specs[b]));
        if r != Ordering::Equal {
            return r;
        }
        let r = import_name(&specs[a]).cmp(&import_name(&specs[b]));
        if r != Ordering::Equal {
            return r;
        }
        import_comment(&specs[a]).cmp(&import_comment(&specs[b]))
    });
    let mut sorted: Vec<Spec> = indices.iter().map(|&i| specs[i].clone()).collect();
    let mut sorted_comments: Vec<Vec<CgPos>> = indices
        .iter()
        .map(|&i| import_comments[i].clone())
        .collect();

    // Dedup adjacent equal specs (when safe).
    let mut deduped: Vec<Spec> = Vec::with_capacity(sorted.len());
    let mut deduped_comments: Vec<Vec<CgPos>> = Vec::with_capacity(sorted.len());
    for i in 0..sorted.len() {
        if i == sorted.len() - 1 || !collapse(&sorted[i], &sorted[i + 1]) {
            deduped.push(sorted[i].clone());
            deduped_comments.push(std::mem::take(&mut sorted_comments[i]));
        } else {
            let p = sorted[i].pos();
            let l = line_at(fset, p);
            if l != line_at(fset, decl.rparen) {
                if let Some(file) = fset.file(p) {
                    file.merge_line(l as usize);
                }
            }
        }
    }
    sorted = deduped;

    // Repositioning: bring each spec (and its comments) back to the
    // original slot index it now occupies.
    for (i, s) in sorted.iter_mut().enumerate() {
        if let Spec::ImportSpec(is) = s {
            if let Some(name) = is.name.as_mut() {
                name.name_pos = pos[i].start;
            }
            update_basic_lit_pos(&mut is.path, pos[i].start);
            is.end_pos = pos[i].end;
            for gp in &deduped_comments[i] {
                let cg = &mut comments[gp.cg_index];
                for c in &mut cg.list {
                    if gp.left {
                        c.slash = Pos(pos[i].start.0 - 1);
                    } else {
                        c.slash = pos[i].end;
                    }
                }
            }
        }
    }

    // Re-sort affected comments by their (now updated) positions.
    if let Some((lo, hi)) = comment_range {
        let slice = &mut comments[lo..=hi];
        slice.sort_by(|a, b| a.pos().cmp(&b.pos()));
    }

    sorted
}

/// Update `lit.value_pos` and (if set) `lit.value_end` so that the
/// literal's end is displaced by the same amount as its start.
fn update_basic_lit_pos(lit: &mut BasicLit, pos: Pos) {
    let len = lit.end().0 - lit.pos().0;
    lit.value_pos = pos;
    if lit.value_end.is_valid() {
        lit.value_end = Pos(pos.0 + len);
    }
}

// ====================================================================
// Tests — hand-built ASTs since we have no parser yet.
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BasicLit, CommentGroup, GenDecl, Ident, ImportSpec, Spec};
    use crate::position::FileSet;
    use crate::token::Token;

    fn import_spec(path_pos: Pos, path: &str) -> ImportSpec {
        ImportSpec {
            doc: None,
            name: None,
            path: BasicLit {
                id: 0,
                value_pos: path_pos,
                value_end: Pos(0),
                kind: Some(Token::STRING),
                value: format!("\"{}\"", path),
            },
            comment: None,
            end_pos: Pos(0),
            id: 0,
        }
    }

    fn import_spec_named(name_pos: Pos, name: &str, path_pos: Pos, path: &str) -> ImportSpec {
        ImportSpec {
            doc: None,
            name: Some(Ident {
                name_pos,
                name: name.to_string(),
                ..Default::default()
            }),
            path: BasicLit {
                id: 0,
                value_pos: path_pos,
                value_end: Pos(0),
                kind: Some(Token::STRING),
                value: format!("\"{}\"", path),
            },
            comment: None,
            end_pos: Pos(0),
            id: 0,
        }
    }

    /// Build a FileSet + File pre-populated with line offsets so that
    /// each import spec ends up on its own line. Returns the file's
    /// base so the caller can phrase positions relative to it.
    fn setup_block(fset: &Arc<FileSet>, decl: &mut GenDecl, lines: &[i64]) -> i64 {
        let last = *lines.last().unwrap();
        let f = fset.add_file("test.go", fset.base(), last + 10);
        for &offset in lines {
            f.add_line(offset);
        }
        decl.lparen = Pos(f.base() + 1);
        decl.rparen = Pos(f.base() + last + 5);
        f.base()
    }

    #[test]
    fn single_spec_block_is_returned_unchanged() {
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        let base = setup_block(&fset, &mut decl, &[10]);
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 10), "foo")));

        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);
        if let Decl::GenDecl(g) = &f.decls[0] {
            assert_eq!(g.specs.len(), 1);
        }
        assert_eq!(f.imports.len(), 1);
    }

    #[test]
    fn duplicate_specs_are_collapsed_when_safe() {
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        // Two specs on consecutive lines; both `"test"`.
        let base = setup_block(&fset, &mut decl, &[10, 20]);
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 10), "test")));
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 20), "test")));

        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);

        if let Decl::GenDecl(g) = &f.decls[0] {
            assert_eq!(g.specs.len(), 1, "duplicate should have been collapsed");
        }
        assert_eq!(f.imports.len(), 1);
    }

    #[test]
    fn distinct_specs_get_sorted_alphabetically() {
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        let base = setup_block(&fset, &mut decl, &[10, 20, 30]);
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 10), "zeta")));
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 20), "alpha")));
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 30), "mu")));

        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);

        let paths: Vec<String> = match &f.decls[0] {
            Decl::GenDecl(g) => g.specs.iter().map(import_path).collect(),
            _ => unreachable!(),
        };
        assert_eq!(paths, vec!["alpha", "mu", "zeta"]);
    }

    #[test]
    fn name_aliases_are_part_of_the_sort_key() {
        // Two specs with the same path but different alias names: must
        // NOT collapse, but should sort by name.
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        let base = setup_block(&fset, &mut decl, &[10, 20]);
        decl.specs.push(Spec::ImportSpec(import_spec_named(
            Pos(base + 10),
            "z",
            Pos(base + 12),
            "x",
        )));
        decl.specs.push(Spec::ImportSpec(import_spec_named(
            Pos(base + 20),
            "a",
            Pos(base + 22),
            "x",
        )));

        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);

        let names: Vec<String> = match &f.decls[0] {
            Decl::GenDecl(g) => g.specs.iter().map(import_name).collect(),
            _ => unreachable!(),
        };
        assert_eq!(names, vec!["a", "z"]);
    }

    #[test]
    fn collapse_preserves_comment_carrying_spec() {
        // Two duplicate "x" imports — one carries a comment, the other
        // doesn't. `collapse` is asymmetric: a spec with a comment may
        // not be dropped, but a comment-less spec adjacent to one with
        // a comment can be. So the surviving spec is the commented one.
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        let base = setup_block(&fset, &mut decl, &[10, 20]);
        let mut commented = import_spec(Pos(base + 10), "x");
        commented.comment = Some(CommentGroup {
            list: vec![crate::ast::Comment {
                slash: Pos(base + 15),
                text: "// k".to_string(),
            }],
        });
        decl.specs.push(Spec::ImportSpec(commented));
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(base + 20), "x")));

        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);
        let g = match &f.decls[0] {
            Decl::GenDecl(g) => g,
            _ => unreachable!(),
        };
        assert_eq!(g.specs.len(), 1, "duplicate collapses");
        if let Spec::ImportSpec(is) = &g.specs[0] {
            assert!(is.comment.is_some(), "surviving spec must keep its comment");
        }
    }

    #[test]
    fn non_block_imports_are_left_alone() {
        // `import "foo"` (no parens) — must skip.
        let fset = FileSet::new();
        let mut decl = GenDecl {
            tok: Some(Token::IMPORT),
            tok_pos: Pos(1),
            ..Default::default()
        };
        // No lparen → not a block.
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(1), "foo")));
        decl.specs
            .push(Spec::ImportSpec(import_spec(Pos(2), "foo")));
        let mut f = File {
            decls: vec![Decl::GenDecl(decl)],
            ..Default::default()
        };
        sort_imports(&fset, &mut f);
        let n = match &f.decls[0] {
            Decl::GenDecl(g) => g.specs.len(),
            _ => unreachable!(),
        };
        assert_eq!(n, 2, "non-block imports unchanged");
        // f.imports is still rebuilt.
        assert_eq!(f.imports.len(), 2);
    }
}
