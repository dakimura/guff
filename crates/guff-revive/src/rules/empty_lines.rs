//! `empty-lines` — warn on leading/trailing blank lines inside blocks.

use std::collections::HashSet;

use guff::ast::{BlockStmt, File};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{line_of, reparse_with_comments};

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    comment_lines: HashSet<usize>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            comment_lines: HashSet::new(),
            failures: Vec::new(),
        }
    }

    pub fn on_file(&mut self, file: &File) {
        // Upstream reads `file.CommentMap()`, which covers every comment in the
        // file. The analysis AST is parsed without comments, so a comment inside
        // a block is absent there and the block looks like it opens on a blank
        // line — a false "extra empty line at the start of a block".
        let fi = self
            .pass
            .files()
            .iter()
            .position(|f| std::ptr::eq(f, file))
            .unwrap_or(0);
        let reparsed = self
            .pass
            .pkg()
            .compiled_go_files
            .get(fi)
            .and_then(|path| reparse_with_comments(path, self.pass.pkg().source_bytes(fi)));
        self.comment_lines = match &reparsed {
            Some(rp) => comment_lines(&rp.file, |pos| {
                rp.fset.position(guff::position::Pos(pos)).line.max(0) as usize
            }),
            None => comment_lines(file, |pos| line_of(self.pass, pos)),
        };
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::BlockStmt(block) = n else {
            return;
        };
        check_block(self.pass, block, &self.comment_lines, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

/// Lines covered by a comment. `line` resolves a position in `file`'s own
/// `FileSet`, which is the reparse's, not the pass's.
fn comment_lines(file: &File, line: impl Fn(i64) -> usize) -> HashSet<usize> {
    let mut lines = HashSet::new();
    for group in &file.comments {
        for comment in &group.list {
            let start = line(comment.slash.0);
            let end = line(comment.end().0);
            for l in start..=end {
                lines.insert(l);
            }
        }
    }
    lines
}

fn check_block(
    pass: &Pass<'_>,
    block: &BlockStmt,
    comment_lines: &HashSet<usize>,
    failures: &mut Vec<Failure>,
) {
    if block.list.is_empty() {
        return;
    }
    let block_start = line_of(pass, block.lbrace.0);
    let first_stmt = line_of(pass, block.list[0].pos().0);
    let first_block_line = block_start + 1;
    let first_is_stmt = first_stmt <= first_block_line;
    let first_is_comment = comment_lines.contains(&first_block_line);
    if !first_is_stmt && !first_is_comment {
        failures.push(Failure {
            rule: "empty-lines",
            pos: block.lbrace.0 as u32,
            message: "extra empty line at the start of a block".into(),
            ..Failure::default()
        });
    }

    let block_end = line_of(pass, block.rbrace.0);
    let last_stmt = line_of(pass, block.list.last().map(|s| s.end().0).unwrap_or(0));
    let last_block_line = block_end.saturating_sub(1);
    let last_is_stmt = last_block_line <= last_stmt;
    let last_is_comment = comment_lines.contains(&last_block_line);
    if !last_is_stmt && !last_is_comment {
        failures.push(Failure {
            rule: "empty-lines",
            // Upstream's failure node is the block for both checks, so even the
            // end-of-block finding is reported at the opening brace.
            pos: block.lbrace.0 as u32,
            message: "extra empty line at the end of a block".into(),
            ..Failure::default()
        });
    }
}
