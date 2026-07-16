//! Port of [`github.com/ultraware/whitespace`](https://github.com/ultraware/whitespace)
//! (golangci-lint wrapper in `pkg/golinters/whitespace`).
//!
//! Defaults match golangci-lint: `multi-if=false`, `multi-func=false`
//! (only unnecessary leading/trailing newlines).
//!
//! DEFERRED: `multi-if` / `multi-func` enforcement when enabled; SuggestedFix;
//! full comment-first/last accuracy when package load lacks `PARSE_COMMENTS`.

use std::sync::OnceLock;

use guff::ast::{BlockStmt, CommentGroup, Decl, File, FuncDecl};
use guff::position::{FileSet, Pos};
use guff::walk::{self, NodeRef, Visitor};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::WhitespaceOptions;

struct WhitespaceVisitor<'a> {
    fset: &'a FileSet,
    comments: &'a [CommentGroup],
    pending: &'a mut Vec<(u32, String)>,
}

fn pos_line(fset: &FileSet, pos: Pos) -> i64 {
    fset.position(pos).line
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

    for c in comments {
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
    let (opening, first, last) = first_and_last(v.comments, v.fset, stmt);
    if let Some(first) = first {
        if let Some(msg) = check_start(v.fset, opening, first) {
            v.pending.push(msg);
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
        if let NodeRef::BlockStmt(stmt) = node {
            check_block(self, stmt);
        }
        true
    }
}

fn run_func(
    fset: &FileSet,
    comments: &[CommentGroup],
    decl: &FuncDecl,
    pending: &mut Vec<(u32, String)>,
) {
    if decl.body.is_none() {
        return;
    }
    let mut visitor = WhitespaceVisitor {
        fset,
        comments,
        pending,
    };
    walk::walk(&mut visitor, NodeRef::FuncDecl(decl));
}

fn run_file(file: &File, fset: &FileSet, pending: &mut Vec<(u32, String)>) {
    for decl in &file.decls {
        let Decl::FuncDecl(f) = decl else {
            continue;
        };
        run_func(fset, &file.comments, f, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "whitespace requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<WhitespaceOptions>("whitespace")
        .copied()
        .unwrap_or_default();
    // DEFERRED: multi-if / multi-func checks when options.multi_if / options.multi_func.
    let _ = options;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        let name = fset.position(file.pos()).filename;
        if !name.ends_with(".go") {
            continue;
        }
        run_file(file, &fset, &mut pending);
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
