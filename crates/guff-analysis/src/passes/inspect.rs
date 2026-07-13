//! The `inspect` analyzer — preorder AST traversal for dependent passes.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/inspect`.

use std::sync::OnceLock;

use guff::ast::File;
use guff::walk::{NodeRef, preorder};

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;

/// Result of the `inspect` analyzer.
///
/// Simplified stand-in for Go's `ast/inspector.Inspector`. Dependent analyzers
/// call [`InspectResult::preorder`] with the same [`File`] slice from the pass.
#[derive(Clone)]
pub struct InspectResult {
    /// Preorder node ids collected when the analyzer ran (for tests / caching).
    pub node_ids: Vec<u32>,
}

impl InspectResult {
    /// Visit every AST node in each file once, in preorder.
    pub fn preorder<F>(&self, files: &[File], mut f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        for file in files {
            preorder(NodeRef::File(file), |n| {
                f(n);
                true
            });
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut node_ids = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let Some(id) = stamped_id(n) {
                node_ids.push(id);
            }
            true
        });
    }
    Ok(Some(Box::new(InspectResult { node_ids })))
}

fn stamped_id(n: NodeRef<'_>) -> Option<u32> {
    match n {
        NodeRef::Ident(i) => Some(i.id),
        NodeRef::FuncDecl(f) => Some(f.name.id),
        NodeRef::BinaryExpr(b) => Some(b.id),
        NodeRef::Field(f) => Some(f.id),
        NodeRef::BlockStmt(b) => Some(b.id),
        NodeRef::File(f) => Some(f.id),
        _ => None,
    }
}

fn inspect_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "inspect",
        doc: "optimize AST traversal for later passes",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/inspect",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    }
}

/// The `inspect` analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(inspect_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use guff::parser::{parse_file, Mode};
    use guff::position::FileSet;

    use super::*;

    const SRC: &str = "package p\n\nfunc f(x int) int {\n\treturn x + 1\n}\n";

    #[test]
    fn inspect_preorder_visits_each_node_once() {
        let fset = FileSet::new();
        let file = parse_file(&fset, "p.go", SRC.as_bytes(), Mode::NONE).expect("parse");

        let mut first_count = 0usize;
        preorder(NodeRef::File(&file), |_| {
            first_count += 1;
            true
        });

        let result = InspectResult { node_ids: vec![] };
        let mut second_count = 0usize;
        result.preorder(std::slice::from_ref(&file), |_| {
            second_count += 1;
        });

        assert!(first_count > 5, "expected many nodes, got {first_count}");
        assert_eq!(first_count, second_count);
    }

    #[test]
    fn inspect_analyzer_validates() {
        assert!(crate::validate::validate(&[analyzer()]).is_ok());
    }
}
