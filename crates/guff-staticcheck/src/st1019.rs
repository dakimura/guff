//! ST1019 — importing the same package multiple times.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1019`.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::code::is_generated_at;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RelatedInformation, RunError, RunFn,
};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut pending: Vec<Diagnostic> = Vec::new();

    for file in pass.files() {
        let mut imports: HashMap<&str, Vec<&guff::ast::ImportSpec>> = HashMap::new();
        for imp in &file.imports {
            if imp
                .name
                .as_ref()
                .is_some_and(|n| n.name == "_")
            {
                // Allow blank imports to coexist with one normal import.
                continue;
            }
            imports
                .entry(imp.path.value.as_str())
                .or_default()
                .push(imp);
        }

        for (path, specs) in imports {
            let unquoted = path.trim_matches('"');
            if unquoted == "unsafe" {
                // Cgo-generated code imports unsafe as _cgo_unsafe in addition
                // to the user's import.
                continue;
            }
            if specs.len() <= 1 {
                continue;
            }
            let first = specs[0];
            let pos = first.path.value_pos.0 as u32;
            if is_generated_at(pass, pos) {
                continue;
            }
            let mut related = Vec::new();
            for other in &specs[1..] {
                related.push(RelatedInformation {
                    pos: other.path.value_pos.0 as u32,
                    end: other.path.end().0 as u32,
                    message: format!("other import of {path}"),
                });
            }
            pending.push(Diagnostic {
                pos,
                end: first.path.end().0 as u32,
                message: format!("package {path} is being imported more than once"),
                related,
                ..Diagnostic::default()
            });
        }
    }

    for diag in pending {
        pass.report(diag);
    }
    Ok(None)
}

fn st1019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1019",
        doc: "importing the same package multiple times",
        url: "https://staticcheck.dev/docs/checks/#ST1019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
