//! Port of [`github.com/sashamelentyev/interfacebloat`](https://github.com/sashamelentyev/interfacebloat).
//!
//! Reports interface types that declare more than `max` methods (default 10).
//! Each entry in the interface's method list counts once, matching upstream's
//! `len(i.Methods.List)` (a method or an embedded/constraint element).

use std::sync::OnceLock;

use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::InterfacebloatOptions;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "interfacebloat requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<InterfacebloatOptions>("interfacebloat")
        .copied()
        .unwrap_or_default();
    let max = opts.max;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::InterfaceType(it) = n {
                let count = it.methods.list.len();
                if count > max {
                    pending.push((
                        it.interface_.0 as u32,
                        format!("the interface has more than {max} methods: {count}"),
                    ));
                }
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "interfacebloat",
        doc: "A linter that checks the number of methods inside an interface.",
        url: "https://github.com/sashamelentyev/interfacebloat",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
