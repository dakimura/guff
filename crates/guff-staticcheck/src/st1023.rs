//! ST1023 — redundant type in variable declaration.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1023` via
//! `sharedcheck.RedundantTypeInDeclarationChecker("should", false)`; the body
//! lives in [`crate::redundant_type_decl`]. Only function-local declarations
//! are flagged (`DeclStmt`) — upstream reaches the same place by walking out
//! to the enclosing `FuncDecl`/`FuncLit`.

use std::sync::OnceLock;

use guff::ast::{Decl, Spec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::redundant_type_decl::{check_gen_decl, report};

fn import_path(spec: &guff::ast::ImportSpec) -> String {
    spec.path.value.trim_matches('"').to_string()
}

fn package_imports_low_level(pass: &Pass<'_>) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::ImportSpec(is) = spec else {
                    continue;
                };
                let path = import_path(is);
                if path == "syscall" || path == "unsafe" {
                    return true;
                }
            }
        }
    }
    false
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // Don't look at code in low-level packages.
    if package_imports_low_level(pass) {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1023 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(DeclStmt), pass.files(), |node| {
        let NodeRef::DeclStmt(ds) = node else {
            return;
        };
        let Decl::GenDecl(gen) = &ds.decl else {
            return;
        };
        check_gen_decl(pass, gen, false, "should", &mut pending);
    });

    report(pass, pending);
    Ok(None)
}

fn st1023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1023",
        doc: "redundant type in variable declaration",
        url: "https://staticcheck.dev/docs/checks/#ST1023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
