//! Port of [`github.com/sivchari/containedctx`](https://github.com/sivchari/containedctx)
//! (golangci-lint wrapper in `pkg/golinters/containedctx`).
//!
//! Reports struct fields whose type is exactly `context.Context`
//! (`pass.TypesInfo.TypeOf(field.Type).String() == "context.Context"`).

use std::sync::OnceLock;

use guff::walk::{preorder, NodeRef};
use guff_analysis::code::is_of_type_with_name;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "containedctx requires inspect analyzer".to_string())?;

    let mut pending: Vec<u32> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::StructType(st) = n {
                for field in &st.fields.list {
                    let Some(ty) = field.ty.as_ref() else {
                        continue;
                    };
                    if is_of_type_with_name(pass, ty, "context.Context") {
                        pending.push(field.pos().0 as u32);
                    }
                }
            }
            true
        });
    }

    for pos in pending {
        pass.reportf(pos, "found a struct that contains a context.Context field");
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "containedctx",
        doc: "A linter that detects structs containing a context.Context field.",
        url: "https://github.com/sivchari/containedctx",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
