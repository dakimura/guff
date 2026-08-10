//! `comments-density` — enforce a minimum comment / code line ratio.

use std::fs;
use std::path::Path;

use guff::ast::{File, Stmt};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::util::reparse_with_comments;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let minimum = config::rule_arg_int(pass, "comments-density", 0).unwrap_or(0);
    if minimum <= 0 {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for (i, file) in pass.files().iter().enumerate() {
        // The analysis AST is parsed without comments, so `file.comments` is
        // empty in production and every file looked like it had a density of 0%
        // — a false positive on any file whose comments were not doc comments.
        // Re-parse for the count, exactly as the other comment-reading rules do
        // (package-comments, goheader, govet's buildtag/directive).
        let reparsed = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .and_then(|path| reparse_with_comments(path, pass.pkg().source_bytes(i)));
        let comment_lines = count_comment_lines(reparsed.as_ref().map_or(file, |rp| &rp.file));
        let code_lines = count_statements(file);
        let total = comment_lines + code_lines;
        if total == 0 {
            continue;
        }
        let density = (comment_lines as f64 / total as f64) * 100.0;
        if density < minimum as f64 {
            failures.push(Failure {
                rule: "comments-density",
                pos: file.package.0 as u32,
                // Upstream's format verb is `%2.f%%`: width 2, no decimals, so a
                // single-digit density is padded ("density of  0%").
                message: format!(
                    "the file has a comment density of {density:2.0}% ({comment_lines} comment lines for {code_lines} code lines) but expected a minimum of {minimum}%"
                ),
                ..Failure::default()
            });
        }
    }
    failures
}

/// Upstream counts every comment group in `file.AST.Comments` — which already
/// includes doc comments — as `len(strings.Split(group.Text(), "\n")) - 1`.
fn count_comment_lines(file: &File) -> usize {
    file.comments
        .iter()
        .map(|group| group.text().lines().count())
        .sum()
}


fn count_statements(file: &guff::ast::File) -> usize {
    let mut count = 0usize;
    walk::inspect(NodeRef::File(file), |n| {
        if matches!(
            n,
            Some(NodeRef::ExprStmt(_))
                | Some(NodeRef::AssignStmt(_))
                | Some(NodeRef::ReturnStmt(_))
                | Some(NodeRef::GoStmt(_))
                | Some(NodeRef::DeferStmt(_))
                | Some(NodeRef::BranchStmt(_))
                | Some(NodeRef::IfStmt(_))
                | Some(NodeRef::SwitchStmt(_))
                | Some(NodeRef::TypeSwitchStmt(_))
                | Some(NodeRef::SelectStmt(_))
                | Some(NodeRef::ForStmt(_))
                | Some(NodeRef::RangeStmt(_))
                | Some(NodeRef::CaseClause(_))
                | Some(NodeRef::CommClause(_))
                | Some(NodeRef::DeclStmt(_))
                | Some(NodeRef::FuncDecl(_))
        ) {
            count += 1;
        }
        true
    });
    count
}
