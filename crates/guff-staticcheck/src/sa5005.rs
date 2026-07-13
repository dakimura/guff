//! SA5005 — finalizer references the finalized object.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5005` (AST-based).

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, FuncLit, Ident};
use guff::walk::{NodeRef, preorder};
use guff_analysis::code::{is_call_to, object_of};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn closure_captures(pass: &Pass<'_>, lit: &FuncLit, obj: guff_types::ObjectId) -> bool {
    let mut found = false;
    preorder(NodeRef::BlockStmt(&lit.body), &mut |n| {
        if let NodeRef::Ident(id) = n {
            if object_of(pass, id) == Some(obj) {
                found = true;
                return false;
            }
        }
        true
    });
    found
}

fn check_call(pass: &Pass<'_>, call: &CallExpr) -> Option<u32> {
    if !is_call_to(pass, call, "runtime.SetFinalizer") || call.args.len() < 2 {
        return None;
    }
    let Expr::Ident(obj) = &call.args[0] else {
        return None;
    };
    let obj_id = object_of(pass, obj)?;
    let Expr::FuncLit(lit) = &call.args[1] else {
        return None;
    };
    if closure_captures(pass, lit, obj_id) {
        Some(call.lparen.0 as u32)
    } else {
        None
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5005 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if let Some(pos) = check_call(pass, call) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(
            pos,
            "the finalizer closes over the object, preventing the finalizer from ever running",
        );
    }
    Ok(None)
}

fn sa5005_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5005",
        doc: "the finalizer references the finalized object, preventing garbage collection",
        url: "https://staticcheck.dev/docs/checks/#SA5005",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5005_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5005_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
