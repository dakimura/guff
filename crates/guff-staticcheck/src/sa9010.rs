//! SA9010 — returned function should be called in defer.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9010` (new in v0.8.1,
//! golangci-lint 2.14.0): `defer f()` where `f()` returns a function is
//! almost always meant to be `defer f()()`.

use std::sync::OnceLock;

use guff::walk::{self, NodeRef};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let (Some(info), Some(artifacts)) = (pass.types_info(), pass.pkg().type_artifacts.as_ref())
    else {
        return Ok(None);
    };
    let types = &artifacts.types;
    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            if let NodeRef::DeferStmt(d) = n {
                // `pass.TypesInfo.TypeOf(def.Call).Underlying().(*types.Signature)`
                if let Some(tv) = info.types.get(&d.call.id) {
                    if matches!(types.get(tv.typ.underlying(types)), TypeData::Signature(_)) {
                        pending.push(d.defer_.0 as u32);
                    }
                }
            }
            true
        });
    }
    for pos in pending {
        pass.reportf(pos, "deferred return function not called".to_string());
    }
    Ok(None)
}

fn sa9010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9010",
        doc: "returned function should be called in defer",
        url: "https://staticcheck.dev/docs/checks/#SA9010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
