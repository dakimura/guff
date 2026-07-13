//! SA9003 — empty body in an if or else branch.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9003`.

use std::sync::OnceLock;

use guff::ast::{BlockStmt, IfStmt, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9003 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::IfStmt(ifs) = n else {
            return;
        };
        if let Some(else_) = ifs.else_.as_deref() {
            if let Stmt::BlockStmt(BlockStmt { list, .. }) = else_ {
                if list.is_empty() {
                    let pos = else_.pos().0 as u32;
                    if !is_generated_at(pass, pos) {
                        pending.push(pos);
                    }
                }
            }
        }
        if ifs.body.list.is_empty() {
            let pos = ifs.if_.0 as u32;
            if !is_generated_at(pass, pos) {
                pending.push(pos);
            }
        }
    });
    for pos in pending {
        pass.reportf(pos, "empty branch");
    }
    Ok(None)
}

fn sa9003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9003",
        doc: "empty body in an if or else branch",
        url: "https://staticcheck.dev/docs/checks/#SA9003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
