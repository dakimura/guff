//! The `inspect` analyzer — preorder AST traversal for dependent passes.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/inspect`.

use std::sync::OnceLock;

use guff::ast::File;
use guff::walk::{NodeRef, preorder_stack};

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;

/// Result of the `inspect` analyzer.
///
/// Simplified stand-in for Go's `ast/inspector.Inspector`. Dependent analyzers
/// call [`InspectResult::preorder`] with the same [`File`] slice from the pass.
///
/// Empty on purpose: this port rewalks on each [`preorder`] call, so collecting
/// node ids at analyzer-run time was unused overhead.
#[derive(Clone, Default)]
pub struct InspectResult {}

impl InspectResult {
    /// Visit every AST node in each file once, in preorder.
    pub fn preorder<F>(&self, files: &[File], mut f: F)
    where
        F: FnMut(NodeRef<'_>),
    {
        let mut stack = Vec::new();
        for file in files {
            preorder_stack(NodeRef::File(file), &mut stack, |n, _| {
                f(n);
                true
            });
        }
    }
}

fn run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    Ok(Some(Box::new(InspectResult::default())))
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
    use guff::walk::preorder;

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

        let result = InspectResult::default();
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
