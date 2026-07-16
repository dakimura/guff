//! ST1001 — dot imports are discouraged.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1001`.
//!
//! DEFERRED: `dot_import_whitelist` config option.

use std::sync::OnceLock;

use guff::walk::NodeRef;
use guff_analysis::code::is_in_test_at;
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::ImportSpec(imp) = node else {
            return;
        };
        let Some(name) = &imp.name else {
            return;
        };
        if name.name != "." {
            return;
        }
        let pos = match_pos(node);
        if is_in_test_at(pass, pos) {
            return;
        }
        // DEFERRED: dot_import_whitelist
        pending.push((pos, "should not use dot imports".into()));
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1001",
        doc: "dot imports are discouraged",
        url: "https://staticcheck.dev/docs/checks/#ST1001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
