//! SA5001 — deferring `Close` before checking for a possible error.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5001`.

use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, DeferStmt, Expr, SelectorExpr, Stmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, object_of, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::signature::signature_results;
use guff_types::tuple::tuple_at;

fn selector_x(sel: &SelectorExpr) -> &Expr {
    match &*sel.x {
        Expr::SelectorExpr(inner) => selector_x(inner),
        other => other,
    }
}

fn returns_error(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(obj) = guff_analysis::code::call_target_object(pass, &call.fun) else {
        return false;
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let Some(sig) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let Some(results) = signature_results(&artifacts.types, sig) else {
        return false;
    };
    let n = guff_types::tuple::tuple_len(&artifacts.types, Some(results));
    if n < 2 {
        return false;
    }
    let last = tuple_at(&artifacts.types, results, n - 1);
    let Some(last_typ) = last.typ(&artifacts.objects) else {
        return false;
    };
    type_with_name(pass, last_typ, "error")
}

fn check_block(pass: &Pass<'_>, block: &BlockStmt, pending: &mut Vec<(u32, String)>) {
    if block.list.len() < 2 {
        return;
    }
    for (i, stmt) in block.list.iter().enumerate() {
        if i + 1 >= block.list.len() {
            break;
        }
        let Stmt::AssignStmt(assign) = stmt else {
            continue;
        };
        if assign.rhs.len() != 1 || assign.lhs.len() < 2 {
            continue;
        }
        if let Expr::Ident(lhs) = &assign.lhs[assign.lhs.len() - 1] {
            if lhs.name == "_" {
                continue;
            }
        }
        let Expr::CallExpr(call) = &assign.rhs[0] else {
            continue;
        };
        if !returns_error(pass, call) {
            continue;
        }
        let Expr::Ident(lhs) = &assign.lhs[0] else {
            continue;
        };
        let Stmt::DeferStmt(DeferStmt { defer_, call: def_call, .. }) = &block.list[i + 1] else {
            continue;
        };
        let Expr::SelectorExpr(sel) = def_call.fun.as_ref() else {
            continue;
        };
        let Expr::Ident(ident) = selector_x(sel) else {
            continue;
        };
        if object_of(pass, ident) != object_of(pass, lhs) {
            continue;
        }
        if sel.sel.name != "Close" {
            continue;
        }
        let fun = call_name(pass, &call.fun).unwrap_or_else(|| "?".into());
        let defer_call = call_name(pass, &def_call.fun)
            .map(|n| format!("{n}()"))
            .unwrap_or_else(|| "Close()".into());
        pending.push((
            defer_.0 as u32,
            format!("should check error returned from {fun}() before deferring {defer_call}"),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BlockStmt), pass.files(), |n| {
        let NodeRef::BlockStmt(block) = n else {
            return;
        };
        check_block(pass, block, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa5001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5001",
        doc: "deferring Close before checking for a possible error",
        url: "https://staticcheck.dev/docs/checks/#SA5001",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
