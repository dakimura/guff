//! SA4019 — multiple identical build constraints in the same file
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4019`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn build_tags(file: &guff::ast::File) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for cg in &file.comments {
        for c in &cg.list {
            let text = c.text.trim();
            if let Some(rest) = text.strip_prefix("//go:build ") {
                out.push(rest.split_whitespace().map(String::from).collect());
            } else if let Some(rest) = text.strip_prefix("// +build ") {
                out.push(rest.split_whitespace().map(String::from).collect());
            }
        }
    }
    out
}

fn identical(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() { return false; }
    let mut sa: Vec<_> = a.iter().collect();
    let mut sb: Vec<_> = b.iter().collect();
    sa.sort();
    sb.sort();
    sa == sb
}

fn build_tags_from_source(pass: &Pass<'_>, file_idx: usize) -> Vec<Vec<String>> {
    if let Some(file) = pass.files().get(file_idx) {
        let from_ast = build_tags(file);
        if !from_ast.is_empty() {
            return from_ast;
        }
    }
    let Some(path) = pass.pkg().compiled_go_files.get(file_idx) else {
        return Vec::new();
    };
    let Ok(src) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in src.lines() {
        let text = line.trim();
        if let Some(rest) = text.strip_prefix("//go:build ") {
            out.push(rest.split_whitespace().map(String::from).collect());
        } else if let Some(rest) = text.strip_prefix("// +build ") {
            out.push(rest.split_whitespace().map(String::from).collect());
        }
    }
    out
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4019 requires inspect analyzer".to_string())?;
    let mut all_pending = Vec::new();
    for (file_idx, file) in pass.files().iter().enumerate() {
        let mut constraints = build_tags_from_source(pass, file_idx);
        if constraints.is_empty() {
            constraints = build_tags(file);
        }
        for i in 0..constraints.len() {
            for j in (i + 1)..constraints.len() {
                if identical(&constraints[i], &constraints[j]) {
                    let msg = format!(
                        "identical build constraints {:?} and {:?}",
                        constraints[i].join(" "),
                        constraints[j].join(" ")
                    );
                    all_pending.push((file.package.0 as u32, msg));
                }
            }
        }
    }
    for (pos, msg) in all_pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}


fn sa4019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4019",
        doc: "multiple identical build constraints in the same file",
        url: "https://staticcheck.dev/docs/checks/#SA4019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
