//! QF1011 — omit redundant type from variable declaration.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1011` via
//! `sharedcheck.RedundantTypeInDeclarationChecker("could", true)`; the body
//! lives in [`crate::redundant_type_decl`]. Same check as ST1023 with
//! `flagHelpfulTypes = true`: blank identifiers, named constants and untyped
//! expressions are flagged too, and low-level packages are not skipped.

use std::sync::OnceLock;

use guff::ast::Decl;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::redundant_type_decl::{check_gen_decl, report};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1011 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(DeclStmt), pass.files(), |node| {
        let NodeRef::DeclStmt(ds) = node else {
            return;
        };
        let Decl::GenDecl(gen) = &ds.decl else {
            return;
        };
        check_gen_decl(pass, gen, true, "could", &mut pending);
    });

    report(pass, pending);
    Ok(None)
}

fn qf1011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1011",
        doc: "omit redundant type from variable declaration",
        url: "https://staticcheck.dev/docs/checks/#QF1011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
