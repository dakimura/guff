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
