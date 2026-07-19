//! `empty-lines` — warn on leading/trailing blank lines inside blocks.

use guff::ast::{BlockStmt, File};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::line_of;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        let comment_lines = comment_lines(pass, file);
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::BlockStmt(block)) = n else {
                return true;
            };
            check_block(pass, block, &comment_lines, &mut failures);
            true
        });
    }
    failures
}

fn comment_lines(pass: &Pass<'_>, file: &File) -> std::collections::HashSet<usize> {
    let mut lines = std::collections::HashSet::new();
    for group in &file.comments {
        for comment in &group.list {
            let start = line_of(pass, comment.slash.0);
            let end = line_of(pass, comment.end().0);
            for line in start..=end {
                lines.insert(line);
            }
        }
    }
    lines
}

fn check_block(
    pass: &Pass<'_>,
    block: &BlockStmt,
    comment_lines: &std::collections::HashSet<usize>,
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
            confidence: None,
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
            pos: block.rbrace.0 as u32,
            message: "extra empty line at the end of a block".into(),
            confidence: None,
        });
    }
}
