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
use guff::position::{FileSet, Pos};

use crate::Pass;

/// Whether the source text of `file` contains `needle`. A cheap guard for
/// analyzers that only need the reparse when a marker is present at all
/// (`exhaustive` and its `//exhaustive:` directives). Unknown files answer
/// `true`, so the caller falls back to the full reparse.
pub fn file_source_contains(pass: &Pass<'_>, file: &File, needle: &[u8]) -> bool {
    let Some(fname) = pass.fset().file(file.pos()).map(|f| f.name().to_string()) else {
        return true;
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
        return true;
    };
    let owned;
    let src: &[u8] = match pass.pkg().source_bytes(index) {
        Some(b) => b,
        None => match std::fs::read(path) {
            Ok(b) => {
                owned = b;
                &owned
            }
            Err(_) => return true,
        },
    };
    src.windows(needle.len()).any(|w| w == needle)
}

/// `file` reparsed with comments, and the means to move a position of the
/// reparse into the analysis `FileSet`.
///
/// For an analyzer that reads the comments `go/parser` hangs on nodes —
/// `GenDecl.Doc`, `Field.Comment` — rather than the file's comment list:
/// the parser is deterministic, so a node of the reparse starts at the same
/// offset as its twin in the analysis AST.
pub struct Reparsed {
    pub fset: std::sync::Arc<FileSet>,
    pub file: File,
    from: std::sync::Arc<guff::position::File>,
    to: std::sync::Arc<guff::position::File>,
}

impl Reparsed {
    /// The analysis position of `p`, a position in the reparse.
    pub fn rebase(&self, p: Pos) -> Pos {
        if !p.is_valid() {
            return p;
        }
        self.to.pos(self.from.offset(p))
    }

    /// `group` with every comment moved into the analysis `FileSet`.
    pub fn rebase_group(&self, group: &CommentGroup) -> CommentGroup {
        CommentGroup {
            list: group
                .list
                .iter()
                .map(|c| Comment {
                    slash: self.rebase(c.slash),
                    text: c.text.clone(),
                })
                .collect(),
        }
    }
}

/// Reparse `file` with `PARSE_COMMENTS`. `None` when the file cannot be
/// located or reparsed.
pub fn reparse_with_comments(pass: &Pass<'_>, file: &File) -> Option<Reparsed> {
    // The analysis FileSet may name a file by full path or by basename
    // depending on how it was loaded, so compare on the basename of both.
    let fname = pass.fset().file(file.pos()).map(|f| f.name().to_string())?;
    let base = std::path::Path::new(&fname)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(fname.as_str())
        .to_string();
    let (index, path) = pass
        .pkg()
        .compiled_go_files
        .iter()
        .enumerate()
        .find(|(_, p)| p.file_name().and_then(|s| s.to_str()) == Some(base.as_str()))?;
    // The type-checker already read this file; re-opening it makes the kernel
    // do the work twice (PERF_TASKS_V3 V1-4).
    let owned;
    let src: &[u8] = match pass.pkg().source_bytes(index) {
        Some(b) => b,
        None => {
            owned = std::fs::read(path).ok()?;
            &owned
        }
    };
    let name = path.file_name().and_then(|s| s.to_str())?;
    let fset = FileSet::new();
    let rfile = parse_file(&fset, name, src, PARSE_COMMENTS).ok()?;
    let to = pass.fset().file(file.pos())?;
    // Everything came out of the same reparse, so it lives in the one file
    // `fset` holds — look it up once instead of per position.
    let from = fset.file(rfile.package)?;
    Some(Reparsed {
        fset,
        file: rfile,
        from,
        to,
    })
}

/// The comment groups of `file`, positioned in the analysis `FileSet`.
///
/// Returns an empty vector when the file cannot be located or reparsed; every
/// caller then behaves as if the file carried no comments.
pub fn file_comments(pass: &Pass<'_>, file: &File) -> Vec<CommentGroup> {
    let Some(r) = reparse_with_comments(pass, file) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(r.file.comments.len());
    for group in &r.file.comments {
        let mut list = Vec::with_capacity(group.list.len());
        for c in &group.list {
            let offset = r.from.offset(c.slash);
            if offset < 0 || offset > r.to.size() {
                continue;
            }
            list.push(Comment {
                slash: r.to.pos(offset),
                text: c.text.clone(),
            });
        }
        if !list.is_empty() {
            out.push(CommentGroup { list });
        }
    }
    out
}
