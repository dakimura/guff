//! SA5004 — `for { select { ...` with an empty default branch spins.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5004`.

use std::sync::OnceLock;

use guff::ast::{CommClause, ForStmt, Stmt};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn check_for_select(pass: &Pass<'_>, fs: &ForStmt, pending: &mut Vec<(u32, String)>) {
    if fs.init.is_some() || fs.cond.is_some() || fs.post.is_some() || fs.body.list.len() != 1 {
        return;
    }
    let Stmt::SelectStmt(sel) = &fs.body.list[0] else {
        return;
    };
    for clause in &sel.body.list {
        let Stmt::CommClause(CommClause { comm, body, .. }) = clause else {
            continue;
        };
        if comm.is_none() && body.is_empty() {
            pending.push((
                sel.select_.0 as u32,
                "should not have an empty default case in a for+select loop; the loop will spin"
                    .into(),
            ));
            break;
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::ForStmt(fs) = n else {
            return;
        };
        check_for_select(pass, fs, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa5004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5004",
        doc: "for { select { ... with an empty default branch spins",
        url: "https://staticcheck.dev/docs/checks/#SA5004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
