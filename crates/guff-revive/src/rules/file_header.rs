//! `file-header` — enforce a common header in each source file.

use guff_analysis::Pass;
use regex::Regex;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let pattern = config::file_header_pattern();
    if pattern.is_empty() {
        return Vec::new();
    }
    let Ok(regex) = Regex::new(pattern) else {
        return Vec::new();
    };

    let mut failures = Vec::new();
    for file in pass.files() {
        let header = first_comment_block(file);
        if header.is_empty() || !regex.is_match(&header) {
            failures.push(Failure {
                rule: "file-header",
                pos: file.package.0 as u32,
                message: "the file doesn't have an appropriate header".into(),
            });
        }
    }
    failures
}

fn first_comment_block(file: &guff::ast::File) -> String {
    let group = file.doc.as_ref().or_else(|| file.comments.first());
    let Some(group) = group else {
        return String::new();
    };
    let mut out = String::new();
    for comment in &group.list {
        let text = comment.text.as_str();
        let body = if text.starts_with("/*") {
            text.strip_prefix("/*")
                .and_then(|s| s.strip_suffix("*/"))
                .unwrap_or(text)
        } else if let Some(rest) = text.strip_prefix("//") {
            rest
        } else {
            text
        };
        out.push_str(body);
    }
    out
}
