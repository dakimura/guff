//! Port of [`github.com/ultraware/whitespace`](https://github.com/ultraware/whitespace)
//! (golangci-lint wrapper in `pkg/golinters/whitespace`).
//!
//! Defaults match golangci-lint: `multi-if=false`, `multi-func=false`
//! (only unnecessary leading/trailing newlines).
//!
//! Comments inside blocks are required for leading-newline accuracy. Production
//! load uses `Mode::NONE` (no `file.comments`), so each file is re-parsed with
//! [`PARSE_COMMENTS`] and findings are mapped back onto the Pass [`FileSet`] by
//! line number.
//!
//! DEFERRED: SuggestedFix; `ignore-leading` / `ignore-trailing` settings.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{BlockStmt, CommentGroup, Decl, File, FuncDecl};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::WhitespaceOptions;

struct WhitespaceVisitor<'a> {
    fset: &'a FileSet,
    comments: &'a [CommentGroup],
    pending: &'a mut Vec<(u32, String)>,
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

fn check_start(fset: &FileSet, start: Pos, first: (Pos, Pos)) -> Option<(u32, String)> {
    if pos_line(fset, start) + 1 < pos_line(fset, first.0) {
        Some((start.0 as u32, "unnecessary leading newline".into()))
    } else {
        None
    }
}

fn check_end(fset: &FileSet, end: Pos, last: (Pos, Pos)) -> Option<(u32, String)> {
    if pos_line(fset, end) - 1 > pos_line(fset, last.1) {
        Some((end.0 as u32, "unnecessary trailing newline".into()))
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
            v.pending.push((
                opening.0 as u32,
                "multi-line statement should be followed by a newline".into(),
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
    pending: &mut Vec<(u32, String)>,
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

fn run_file(
    file: &File,
    fset: &FileSet,
    multi_if: bool,
    multi_func: bool,
    pending: &mut Vec<(u32, String)>,
) {
    for decl in &file.decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        run_func(fset, &file.comments, f, multi_if, multi_func, pending);
    }
}

fn reparse_with_comments(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn line_pos(fset: &FileSet, file_pos: Pos, line: i64) -> Option<u32> {
    let ft = fset.file(file_pos)?;
    if line < 1 || line as usize > ft.line_count() {
        return None;
    }
    Some(ft.line_start(line as usize).0 as u32)
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

        // Prefer a comment-bearing reparse when the on-disk path is known.
        // Fall back to the loaded AST (tests / overlays without comments).
        let (check_fset, check_file, map_lines) = if let Some(path) = paths.get(i) {
            if let Some((re_fset, parsed)) = reparse_with_comments(path) {
                (re_fset, parsed, true)
            } else {
                (fset.clone(), (*file).clone(), false)
            }
        } else if !file.comments.is_empty() {
            (fset.clone(), (*file).clone(), false)
        } else {
            (fset.clone(), (*file).clone(), false)
        };

        let mut local = Vec::new();
        run_file(
            &check_file,
            &check_fset,
            options.multi_if,
            options.multi_func,
            &mut local,
        );

        if map_lines {
            for (pos, message) in local {
                let line = check_fset.position(Pos(pos as i64)).line;
                if let Some(mapped) = line_pos(&fset, file.pos(), line) {
                    pending.push((mapped, message));
                }
            }
        } else {
            pending.extend(local);
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
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
