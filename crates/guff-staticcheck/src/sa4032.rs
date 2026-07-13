//! SA4032 — comparing runtime.GOOS/GOARCH against impossible value.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4032` (simplified build-tag check).

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{expr_to_string, selector_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn file_build_tags(pass: &Pass<'_>, file_idx: usize, file: &guff::ast::File) -> Vec<String> {
    let mut tags = file_build_tags_from_ast(file);
    if !tags.is_empty() {
        return tags;
    }
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return tags;
    };
    let Ok(src) = std::fs::read_to_string(path) else {
        return tags;
    };
    for line in src.lines() {
        let text = line.trim();
        if let Some(rest) = text.strip_prefix("//go:build ") {
            tags.push(rest.to_string());
        } else if let Some(rest) = text.strip_prefix("// +build ") {
            tags.push(rest.to_string());
        }
    }
    tags
}

fn file_build_tags_from_ast(file: &guff::ast::File) -> Vec<String> {
    let mut tags = Vec::new();
    for cg in &file.comments {
        for c in &cg.list {
            let text = c.text.trim();
            if let Some(rest) = text.strip_prefix("//go:build ") {
                tags.push(rest.to_string());
            } else if let Some(rest) = text.strip_prefix("// +build ") {
                tags.push(rest.to_string());
            }
        }
    }
    tags
}

fn tag_implies(tags: &[String], want: &str) -> bool {
    tags.iter().any(|tag| {
        tag.split_whitespace().any(|t| t == want)
            || tag.contains(want)
    })
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4032 requires inspect analyzer".to_string())?
        .clone();
    let tagged_files: Vec<_> = pass
        .files()
        .iter()
        .enumerate()
        .map(|(i, file)| (file, file_build_tags(pass, i, file)))
        .filter(|(_, tags)| !tags.is_empty())
        .collect();
    let mut all_pending = Vec::new();
    for (file, tags) in tagged_files {
        inspect.preorder(std::slice::from_ref(file), |node| {
            let NodeRef::BinaryExpr(bin) = node else {
                return;
            };
            if !matches!(bin.op, Token::EQL | Token::NEQ) {
                return;
            }
            let (sym, lit) = match (bin.x.as_ref(), bin.y.as_ref()) {
                (Expr::SelectorExpr(sel), lit) => (selector_name(pass, sel), lit),
                (lit, Expr::SelectorExpr(sel)) => (selector_name(pass, sel), lit),
                _ => return,
            };
            let Some(go_val) = expr_to_string(pass, lit) else {
                return;
            };
            let msg = match sym.as_deref() {
                Some(s) if s == "runtime.GOOS" && !tag_implies(&tags, &go_val) => Some(format!(
                    "due to the file's build constraints, runtime.GOOS will never equal {go_val:?}"
                )),
                Some(s) if s == "runtime.GOARCH" && !tag_implies(&tags, &go_val) => Some(format!(
                    "due to the file's build constraints, runtime.GOARCH will never equal {go_val:?}"
                )),
                _ => None,
            };
            if let Some(msg) = msg {
                all_pending.push((bin.op_pos.0 as u32, msg));
            }
        });
    }
    for (pos, msg) in all_pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4032_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4032",
        doc: "comparing runtime.GOOS or runtime.GOARCH against impossible value",
        url: "https://staticcheck.dev/docs/checks/#SA4032",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4032_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4032_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
