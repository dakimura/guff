//! Sample AST-only analyzer for end-to-end smoke tests.
//!
//! Uses [`super::inspect`] to find the first top-level function and reports a
//! single diagnostic describing it — a minimal stand-in for debug printers.

use std::sync::OnceLock;

use guff::node_mask;
use guff::walk::NodeRef;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;
use crate::passes::inspect;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "printast requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    {
        let files = pass.files();
        let mut reported = false;
        inspect.preorder_typed(node_mask!(FuncDecl), files, |n| {
            if reported {
                return;
            }
            let NodeRef::FuncDecl(f) = n else {
                return;
            };
            pending.push((
                f.name.name_pos.0 as u32,
                format!("printast: found FuncDecl {}", f.name.name),
            ));
            reported = true;
        });
    }
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }

    Ok(None)
}

fn printast_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "printast",
        doc: "report the first top-level function declaration (E2E smoke)",
        url: "",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// Sample AST-only analyzer for smoke tests.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(printast_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate;

    #[test]
    fn printast_validates() {
        assert!(validate::validate(&[analyzer()]).is_ok());
    }
}
