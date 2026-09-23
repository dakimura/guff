//! Comments for an analysis file.
//!
//! The analysis AST is parsed without comments, so an analyzer that needs
//! `ast.NewCommentMap` — the association between a comment group and the node
//! it documents — has to reparse the file with `PARSE_COMMENTS` and map the
//! reparse's positions back into the analysis `FileSet`.
//!
//! `S1008` grew its own copy of this before it was shared; `gosec` builds an
//! equivalent reparse inline because it also needs the reparsed AST itself.

use guff::ast::{Comment, CommentGroup, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;

use crate::Pass;

/// The comment groups of `file`, positioned in the analysis `FileSet`.
///
/// Returns an empty vector when the file cannot be located or reparsed; every
/// caller then behaves as if the file carried no comments.
pub fn file_comments(pass: &Pass<'_>, file: &File) -> Vec<CommentGroup> {
    // The analysis FileSet may name a file by full path or by basename
    // depending on how it was loaded, so compare on the basename of both.
    let Some(fname) = pass.fset().file(file.pos()).map(|f| f.name().to_string()) else {
        return Vec::new();
    };
    let base = std::path::Path::new(&fname)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fname.as_str())
        .to_string();
    let Some((index, path)) = pass
        .pkg()
        .compiled_go_files
        .iter()
        .enumerate()
        .find(|(_, p)| p.file_name().and_then(|s| s.to_str()) == Some(base.as_str()))
    else {
        return Vec::new();
    };
    // The type-checker already read this file; re-opening it makes the kernel
    // do the work twice (PERF_TASKS_V3 V1-4).
    let owned;
    let src: &[u8] = match pass.pkg().source_bytes(index) {
        Some(b) => b,
        None => match std::fs::read(path) {
            Ok(b) => {
                owned = b;
                &owned
            }
            Err(_) => return Vec::new(),
        },
    };
    let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let rfset = FileSet::new();
    let Ok(rfile) = parse_file(&rfset, name, src, PARSE_COMMENTS) else {
        return Vec::new();
    };
    let Some(to) = pass.fset().file(file.pos()) else {
        return Vec::new();
    };
    // Every comment came out of the same reparse, so it lives in the one file
    // `rfset` holds — look it up once instead of per comment.
    let Some(from) = rfset.file(rfile.package) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(rfile.comments.len());
    for group in &rfile.comments {
        let mut list = Vec::with_capacity(group.list.len());
        for c in &group.list {
            let offset = from.offset(c.slash);
            if offset < 0 || offset > to.size() {
                continue;
            }
            list.push(Comment {
                slash: to.pos(offset),
                text: c.text.clone(),
            });
        }
        if !list.is_empty() {
            out.push(CommentGroup { list });
        }
    }
    out
}
