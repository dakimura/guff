//! SA9001 — defers in range loops over channels may not run.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9001`.

use std::sync::OnceLock;

use guff::ast::{BranchStmt, DeferStmt, FuncLit, RangeStmt, ReturnStmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::{NodeRef, preorder};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(RangeStmt), pass.files(), |n| {
        let NodeRef::RangeStmt(rng) = n else {
            return;
        };
        let info = match pass.types_info() {
            Some(i) => i,
            None => return,
        };
        let artifacts = match pass.pkg().type_artifacts.as_ref() {
            Some(a) => a,
            None => return,
        };
        let typ = match info.types.get(&rng.x.id()) {
            Some(t) => t.typ,
            None => return,
        };
        if !matches!(
            artifacts.types.get(typ.underlying(&artifacts.types)),
            TypeData::Chan(_)
        ) {
            return;
        }
        let mut exits = false;
        let mut defers: Vec<&DeferStmt> = Vec::new();
        preorder(NodeRef::BlockStmt(&rng.body), &mut |n| {
            match n {
                NodeRef::ReturnStmt(ReturnStmt { .. }) => {
                    exits = true;
                    return false;
                }
                NodeRef::BranchStmt(BranchStmt { tok, .. }) if *tok == Token::BREAK => {
                    exits = true;
                    return false;
                }
                NodeRef::DeferStmt(d) => defers.push(d),
                NodeRef::FuncLit(FuncLit { .. }) => return false,
                _ => {}
            }
            true
        });
        if exits {
            return;
        }
        for d in defers {
            pending.push(d.defer_.0 as u32);
        }
    });
    for pos in pending {
        pass.report_unless_generated(
            pos,
            "defers in this range loop won't run unless the channel gets closed",
        );
    }
    Ok(None)
}

fn sa9001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9001",
        doc: "defers in range loops may not run when you expect them to",
        url: "https://staticcheck.dev/docs/checks/#SA9001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
