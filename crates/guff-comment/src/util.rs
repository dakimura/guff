//! Shared helpers for comment-oriented analyzers.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use guff::ast::{CommentGroup, Decl, File};
use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::{FileSet, Pos};

/// Re-parse `path` with comments so free-floating comments are available.
///
/// Production package load uses `Mode::NONE`, which drops body comments
/// from `file.comments`. Comment linters re-parse from disk like `nolint`.
/// Keep the returned [`FileSet`] so positions on `File` stay valid while mapping
/// line numbers back onto the Pass [`FileSet`].
///
/// `cached` is the package's already-read source bytes
/// (`pass.pkg().source_bytes(i)`). Pass them: type-checking read the file
/// moments ago, and re-opening it makes the kernel do the work twice. On
/// prometheus `./...` this path was a visible part of `__open` (0.97s) and
/// `read` (0.59s) in the profile. `None` falls back to reading, for callers
/// that have no package handle.
///
/// (Sharing one reparse across *all* comment linters was tried and reverted —
/// see PERF_TASKS_V3 §V1-4 NO-GO: retaining a second AST per file for the whole
/// analyze phase cost +0.94 GiB RSS for +1.1% wall.)
///
/// Parsed with [`COMMENTS_ONLY`]: godot, dupword, godox and godoclint read
/// comments and doc strings, never `Ident.obj`.
pub fn reparse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<(Arc<FileSet>, File)> {
    let owned;
    let src: &[u8] = match cached {
        Some(b) => b,
        None => {
            owned = fs::read(path).ok()?;
            &owned
        }
    };
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, src, COMMENTS_ONLY).ok()?;
    Some((fset, file))
}

/// Map a 1-based line in the AST file to a Pass-reportable byte offset.
pub fn line_pos(fset: &FileSet, file_pos: Pos, line: i64) -> Option<u32> {
    let ft = fset.file(file_pos)?;
    if line < 1 || line as usize > ft.line_count() {
        return None;
    }
    Some(ft.line_start(line as usize).0 as u32)
}

/// Map a position from the comment-preserving **re-parse** back into the
/// analysis `FileSet`, keeping the column.
///
/// The two FileSets number positions independently, so the only shared
/// coordinate is (line, column). Recovering just the line and taking
/// `line_pos` — the line's *start* — reports column 1 for every doc comment,
/// which is right only for declarations at the left margin. A doc comment
/// inside a `const (` group is indented, and upstream reports where it actually
/// begins: syncthing's `deviceid.go:22` is `:2` there and was `:1` here.
///
/// Nothing could see it. godoclint's isolate fixture is a single top-level
/// func, where both answers are 1, and the OSS/hunt comparison key has no
/// column field at all (§1).
///
/// `Position::column` is a 1-based byte column and `line_start` a byte offset,
/// so the arithmetic is exact rather than an approximation for ASCII.
pub fn reparsed_pos(
    fset: &guff::FileSet,
    file_pos: guff::Pos,
    re_fset: &guff::FileSet,
    pos: guff::Pos,
) -> Option<u32> {
    let p = re_fset.position(pos);
    let line_start = line_pos(fset, file_pos, p.line)?;
    Some(line_start + u32::try_from(p.column.max(1) - 1).unwrap_or(0))
}

/// Collect declaration doc comments (godot default `declarations` scope).
///
/// Matches upstream `getDeclarationComments`: top-level `GenDecl` / `FuncDecl`
/// docs only. Package file docs (`file.doc`) are intentionally omitted — godot's
/// `DeclScope` does not check them (unlike `all` / `toplevel`).
///
/// DEFERRED: `getBlockComments` for docs inside `var (` / `const (` groups.
pub fn declaration_docs(file: &File) -> Vec<&CommentGroup> {
    let mut out = Vec::new();
    for decl in &file.decls {
        match decl {
            Decl::GenDecl(g) => {
                if let Some(doc) = &g.doc {
                    out.push(doc);
                }
            }
            Decl::FuncDecl(f) => {
                if let Some(doc) = &f.doc {
                    out.push(doc);
                }
            }
            Decl::BadDecl(_) => {}
        }
    }
    out
}

/// Comments inside a top-level `var (` / `const (` block.
///
/// Port of godot's `getBlockComments`. Its `declarations` scope is
/// `getBlockComments() ++ getDeclarationComments()`, and guff had only the
/// second half, so a documented spec inside a group was never checked at all.
///
/// Three details are upstream's and are load-bearing:
///
/// - only a `GenDecl` with a real `Lparen` counts — `const A = 1` on one line
///   has no block to be inside of;
/// - the walk is over **`file.Comments`**, not the specs' `Doc` fields, so a
///   free-floating comment in the block is included and a spec's doc is found
///   by position rather than by ownership;
/// - the column must be **exactly 2**. Upstream says why: the block itself is
///   top level, so its immediate contents sit one level in. A comment indented
///   twice is deliberately skipped, and so is one at the margin.
pub fn block_comments<'a>(fset: &FileSet, file: &'a File) -> Vec<&'a CommentGroup> {
    let mut out = Vec::new();
    for decl in &file.decls {
        let Decl::GenDecl(d) = decl else {
            continue;
        };
        if d.lparen.0 == 0 {
            continue;
        }
        for c in &file.comments {
            if c.list.is_empty() {
                continue;
            }
            let pos = c.pos();
            if d.lparen > pos || pos > d.rparen {
                continue;
            }
            if fset.position(pos).column != 2 {
                continue;
            }
            out.push(c);
        }
    }
    out
}

/// Plain multiline text of a comment group with markers stripped.
pub fn comment_group_raw_text(cg: &CommentGroup) -> String {
    let mut parts = Vec::new();
    for c in &cg.list {
        parts.push(strip_comment_markers(&c.text));
    }
    parts.join("\n")
}

fn strip_comment_markers(raw: &str) -> String {
    let bytes = raw.as_bytes();
    if bytes.len() < 2 {
        return raw.to_string();
    }
    match bytes[1] {
        b'/' => {
            let body = &raw[2..];
            body.strip_prefix(' ').unwrap_or(body).to_string()
        }
        b'*' => {
            let mut s = raw[2..].to_string();
            if s.ends_with("*/") {
                s.truncate(s.len() - 2);
            }
            s
        }
        _ => raw.to_string(),
    }
}
