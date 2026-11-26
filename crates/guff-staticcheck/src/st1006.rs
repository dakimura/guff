//! ST1006 — poorly chosen receiver name (`self` / `this` / `_`).
//!
//! Port of `honnef.co/go/tools/stylecheck/st1006`.
//! AST-based (upstream uses buildir + method sets); methods declared in this
//! package are inspected directly, which naturally skips embedded methods.

use std::sync::OnceLock;

use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl), pass.files(), |node| {
        let NodeRef::FuncDecl(fd) = node else {
            return;
        };
        let Some(recv) = &fd.recv else {
            return;
        };
        for field in &recv.list {
            for name in &field.names {
                if name.name == "self" || name.name == "this" {
                    pending.push((
                        name.pos().0 as u32,
                        "receiver name should be a reflection of its identity; don't use generic names such as \"this\" or \"self\"".into(),
                    ));
                } else if name.name == "_" {
                    pending.push((
                        name.pos().0 as u32,
                        "receiver name should not be an underscore, omit the name if it is unused"
                            .into(),
                    ));
                }
            }
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1006",
        doc: "poorly chosen receiver name",
        url: "https://staticcheck.dev/docs/checks/#ST1006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
