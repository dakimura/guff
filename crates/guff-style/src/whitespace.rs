//! Port of [`github.com/ultraware/whitespace`](https://github.com/ultraware/whitespace)
//! (golangci-lint wrapper in `pkg/golinters/whitespace`).
//!
//! Defaults match golangci-lint: `multi-if=false`, `multi-func=false`
//! (only unnecessary leading/trailing newlines).
//!
//! Comments inside blocks are required for leading-newline accuracy. Production
//! load uses `Mode::NONE` (no body comments on the typed AST), so we scan each
//! file once for COMMENT tokens and remap them onto the Pass [`FileSet`]. That
//! avoids a full `PARSE_COMMENTS` re-parse of every file.
//!
//! DEFERRED: SuggestedFix; `ignore-leading` / `ignore-trailing` settings.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{BlockStmt, Comment, CommentGroup, Decl, FuncDecl};
use guff::position::{File, FileSet, Pos};
use guff::scanner::{Scanner, SCAN_COMMENTS};
use guff::token::Token;
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::WhitespaceOptions;

struct WhitespaceVisitor<'a> {
    fset: &'a FileSet,
    comments: &'a [CommentGroup],
    pending: &'a mut Vec<(u32, String, TextEdit)>,
    want_newline: HashMap<i64, bool>,
    multi_if: bool,
    multi_func: bool,
}

fn pos_line(fset: &FileSet, pos: Pos) -> i64 {
    fset.position(pos).line
}

fn check_multi_line(v: &mut WhitespaceVisitor<'_>, body: &BlockStmt, start: Pos, end: Pos) {
    if pos_line(v.fset, end) > pos_line(v.fset, start) {
        v.want_newline.insert(body.lbrace.0, true);
    }
}

/// First/last content inside a block (statement or comment), plus opening pos.
fn first_and_last(
    comments: &[CommentGroup],
    fset: &FileSet,
    stmt: &BlockStmt,
) -> (Pos, Option<(Pos, Pos)>, Option<(Pos, Pos)>) {
    let mut opening_pos = Pos(stmt.lbrace.0 + 1);

    if stmt.list.is_empty() {
        return (opening_pos, None, None);
    }

    let mut first = (stmt.list[0].pos(), stmt.list[0].end());
    let mut last = {
        let s = stmt.list.last().unwrap();
        (s.pos(), s.end())
    };

    // Only comments whose start lies within the block's byte range
    // `[stmt.pos(), stmt.end())` can change `first`/`last`/`opening_pos`: every
    // branch below either requires the comment to be on the opening line
    // (`c.pos() > opening_pos`, i.e. past `{`) or strictly inside the block, and
    // the `<` / `>` guards drop everything else. `comments` arrives in source
    // order (ascending `pos`), so binary-search to the window start and stop at
    // the block end — turning the former O(blocks × file_comments) scan into
    // O(log C + comments_in_block). Comments after `stmt.end()` only ever reach
    // the opening-line branch for single-line blocks, which never yield a
    // leading/trailing-newline finding, so skipping them is finding-preserving.
    let lo = stmt.pos().0;
    let hi = stmt.end().0;
    let start = comments.partition_point(|c| c.pos().0 < lo);

    for c in &comments[start..] {
        if c.pos().0 >= hi {
            break;
        }
        // Comment on the `{` line after `{` → treat as opening content.
        if pos_line(fset, c.pos()) == pos_line(fset, opening_pos) && c.pos().0 > opening_pos.0 {
            if pos_line(fset, c.end()) != pos_line(fset, opening_pos) {
                first = (c.pos(), c.end());
            } else {
                opening_pos = c.end();
            }
        }

        if pos_line(fset, c.pos()) == pos_line(fset, stmt.pos())
            || pos_line(fset, c.end()) == pos_line(fset, stmt.end())
        {
            continue;
        }

        if c.pos().0 < stmt.pos().0 || c.end().0 > stmt.end().0 {
            continue;
        }

        if c.pos().0 < first.0.0 {
            first = (c.pos(), c.end());
        }
        if c.end().0 > last.1.0 {
            last = (c.pos(), c.end());
        }
    }

    (opening_pos, Some(first), Some(last))
}

/// A blank-line fix: everything between the brace and the code collapses to a
/// single newline.
///
/// The edit swallows the statement's indentation along with the blank lines —
/// upstream's spans start one past `{` and run to the statement itself — and
/// the formatter that runs after the fixes puts it back. Trying to preserve the
/// indent here would mean guessing at it; upstream does not.
fn newline_edit(pos: Pos, end: Pos) -> TextEdit {
    TextEdit {
        pos: pos.0 as u32,
        end: end.0 as u32,
        new_text: "\n".into(),
    }
}

fn check_start(fset: &FileSet, start: Pos, first: (Pos, Pos)) -> Option<(u32, String, TextEdit)> {
    if pos_line(fset, start) + 1 < pos_line(fset, first.0) {
        Some((
            start.0 as u32,
            "unnecessary leading newline".into(),
            newline_edit(start, first.0),
        ))
    } else {
        None
    }
}

fn check_end(fset: &FileSet, end: Pos, last: (Pos, Pos)) -> Option<(u32, String, TextEdit)> {
    if pos_line(fset, end) - 1 > pos_line(fset, last.1) {
        Some((
            end.0 as u32,
            "unnecessary trailing newline".into(),
            newline_edit(last.1, end),
        ))
    } else {
        None
    }
}

fn check_block(v: &mut WhitespaceVisitor<'_>, stmt: &BlockStmt) {
    let want_newline = v.want_newline.get(&stmt.lbrace.0).copied().unwrap_or(false);
    let comments = if want_newline {
        &[][..]
    } else {
        v.comments
    };
    let (opening, first, last) = first_and_last(comments, v.fset, stmt);

    if let Some(first) = first {
        let start_msg = check_start(v.fset, opening, first);
        if want_newline && start_msg.is_none() && !stmt.list.is_empty() {
            // The opposite direction: a multi-line header wants a blank line
            // after it, so this one *inserts* rather than collapsing.
            let at = stmt.list[0].pos();
            v.pending.push((
                opening.0 as u32,
                "multi-line statement should be followed by a newline".into(),
                TextEdit {
                    pos: at.0 as u32,
                    end: at.0 as u32,
                    new_text: "\n".into(),
                },
            ));
        } else if !want_newline {
            if let Some(msg) = start_msg {
                v.pending.push(msg);
            }
        }
    }

    if let Some(last) = last {
        if let Some(msg) = check_end(v.fset, stmt.rbrace, last) {
            v.pending.push(msg);
        }
    }
}

impl<'a> Visitor<'a> for WhitespaceVisitor<'a> {
    fn enter(&mut self, node: NodeRef<'a>) -> bool {
        match node {
            NodeRef::IfStmt(stmt) if self.multi_if => {
                check_multi_line(self, &stmt.body, stmt.cond.pos(), stmt.cond.end());
            }
            NodeRef::FuncLit(stmt) if self.multi_func => {
                check_multi_line(self, &stmt.body, stmt.ty.pos(), stmt.ty.end());
            }
            NodeRef::FuncDecl(stmt) if self.multi_func => {
                if let Some(body) = &stmt.body {
                    check_multi_line(self, body, stmt.ty.pos(), stmt.ty.end());
                }
            }
            NodeRef::BlockStmt(stmt) => {
                check_block(self, stmt);
            }
            _ => {}
        }
        true
    }
}

fn run_func(
    fset: &FileSet,
    comments: &[CommentGroup],
    decl: &FuncDecl,
    multi_if: bool,
    multi_func: bool,
    pending: &mut Vec<(u32, String, TextEdit)>,
) {
    if decl.body.is_none() {
        return;
    }
    let mut visitor = WhitespaceVisitor {
        fset,
        comments,
        pending,
        want_newline: HashMap::new(),
        multi_if,
        multi_func,
    };
    walk::walk(&mut visitor, NodeRef::FuncDecl(decl));
}

fn run_file_decls(
    decls: &[Decl],
    fset: &FileSet,
    comments: &[CommentGroup],
    multi_if: bool,
    multi_func: bool,
    pending: &mut Vec<(u32, String, TextEdit)>,
) {
    for decl in decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        run_func(fset, comments, f, multi_if, multi_func, pending);
    }
}

fn src_may_have_comments(src: &[u8]) -> bool {
    src.windows(2)
        .any(|w| w == b"//" || w == b"/*")
}

/// Scan COMMENT tokens and remap them onto `pass_file`'s byte base.
///
/// Cheaper than a full `PARSE_COMMENTS` re-parse: we only tokenize, then walk
/// the already-typed AST. Grouping matches `consume_comment_group(1)` —
/// adjacent comments with no empty line between them form one group; a real
/// (non-semicolon) token flushes the current group.
fn collect_comments(src: &[u8], pass_file: &File) -> Vec<CommentGroup> {
    if !src_may_have_comments(src) {
        return Vec::new();
    }

    let temp_fset = FileSet::new();
    let temp_file = temp_fset.add_file(pass_file.name(), -1, src.len() as i64);
    let temp_base = temp_file.base();
    let pass_base = pass_file.base();

    let mut sc = Scanner::new();
    sc.init(Arc::clone(&temp_file), src, None, SCAN_COMMENTS);

    let mut groups = Vec::new();
    let mut cur: Vec<Comment> = Vec::new();
    let mut endline: i64 = -1;

    loop {
        let (pos, tok, lit) = sc.scan();
        match tok {
            Token::EOF => break,
            Token::COMMENT => {
                let line = temp_fset.position(pos).line;
                if !cur.is_empty() && (endline < 0 || line > endline + 1) {
                    groups.push(CommentGroup {
                        list: std::mem::take(&mut cur),
                    });
                }
                let mut el = line;
                if lit.as_bytes().get(1) == Some(&b'*') {
                    el += lit.bytes().filter(|&b| b == b'\n').count() as i64;
                }
                endline = el;
                cur.push(Comment {
                    slash: Pos(pass_base + (pos.0 - temp_base)),
                    text: lit.into_owned(),
                });
            }
            Token::SEMICOLON => {
                // Inserted newline semis sit between adjacent line comments;
                // line-gap logic above already decides whether to join.
            }
            _ => {
                if !cur.is_empty() {
                    groups.push(CommentGroup {
                        list: std::mem::take(&mut cur),
                    });
                    endline = -1;
                }
            }
        }
    }
    if !cur.is_empty() {
        groups.push(CommentGroup { list: cur });
    }
    groups
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "whitespace requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<WhitespaceOptions>("whitespace")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let file = &pass.files()[i];
        let name = fset.position(file.pos()).filename;
        if !name.ends_with(".go") {
            continue;
        }

        let Some(pass_file) = fset.file(file.pos()) else {
            continue;
        };

        // Prefer a comment scan when the on-disk path is known. Fall back to
        // whatever comments the loaded AST already has (tests / overlays).
        let comments = if let Some(bytes) = pass.pkg().source_bytes(i) {
            if bytes.len() as i64 == pass_file.size() {
                collect_comments(bytes, pass_file.as_ref())
            } else {
                file.comments.clone()
            }
        } else if let Some(path) = paths.get(i) {
            match fs::read(path) {
                Ok(src) if src.len() as i64 == pass_file.size() => {
                    collect_comments(&src, pass_file.as_ref())
                }
                Ok(_) | Err(_) => file.comments.clone(),
            }
        } else {
            file.comments.clone()
        };

        run_file_decls(
            &file.decls,
            &fset,
            &comments,
            options.multi_if,
            options.multi_func,
            &mut pending,
        );
    }

    for (pos, message, edit) in pending {
        pass.report(Diagnostic {
            pos,
            message: message.clone(),
            suggested_fixes: vec![SuggestedFix {
                message,
                text_edits: vec![edit],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "whitespace",
        doc: "Whitespace is a linter that checks for unnecessary newlines at the start and end of functions, if, for, etc.",
        url: "https://github.com/ultraware/whitespace",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
