//! `comments-density` — enforce a minimum comment / code line ratio.

use guff::ast::Stmt;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let minimum = config::rule_arg_int(pass, "comments-density", 0).unwrap_or(0);
    if minimum <= 0 {
        return Vec::new();
    }

    let mut failures = Vec::new();
    for file in pass.files() {
        let comment_lines = count_comment_lines(file);
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
                message: format!(
                    "the file has a comment density of {density:.0}% ({comment_lines} comment lines for {code_lines} code lines) but expected a minimum of {minimum}%"
                ),
            });
        }
    }
    failures
}

fn count_comment_lines(file: &guff::ast::File) -> usize {
    let mut lines = 0usize;
    let mut count_group = |group: &guff::ast::CommentGroup| {
        let text = group.text();
        if !text.is_empty() {
            lines += text.lines().count();
        }
    };
    if let Some(doc) = &file.doc {
        count_group(doc);
    }
    for group in &file.comments {
        count_group(group);
    }
    lines
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
