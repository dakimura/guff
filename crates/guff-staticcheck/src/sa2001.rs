//! SA2001 — empty critical section (Lock immediately followed by Unlock).
//!
//! Port of `honnef.co/go/tools/staticcheck/sa2001`.

use std::sync::OnceLock;

use guff::ast::{BlockStmt, Expr, SelectorExpr, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::ObjectData;
use guff_types::signature::{signature_params, signature_results};
use guff_types::tuple::tuple_len;

use crate::render::render_expr;

fn mutex_params<'a>(pass: &Pass<'_>, stmt: &'a Stmt) -> Option<(&'a Expr, String)> {
    let Stmt::ExprStmt(es) = stmt else {
        return None;
    };
    let Expr::CallExpr(call) = unparen_expr(&es.x) else {
        return None;
    };
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen_expr(call.fun.as_ref()) else {
        return None;
    };
    let obj = object_of(pass, sel)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return None;
    };
    let sig = obj.typ(&artifacts.objects)?;
    let n_params = signature_params(&artifacts.types, sig)
        .map(|p| tuple_len(&artifacts.types, Some(p)))
        .unwrap_or(0);
    let n_results = signature_results(&artifacts.types, sig)
        .map(|r| tuple_len(&artifacts.types, Some(r)))
        .unwrap_or(0);
    if n_params != 0 || n_results != 0 {
        return None;
    }
    Some((x, sel.name.clone()))
}

fn check_block(pass: &Pass<'_>, block: &BlockStmt) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    if block.list.len() < 2 {
        return out;
    }
    for i in 0..block.list.len() - 1 {
        let Some((sel1, method1)) = mutex_params(pass, &block.list[i]) else {
            continue;
        };
        let Some((sel2, method2)) = mutex_params(pass, &block.list[i + 1]) else {
            continue;
        };
        if render_expr(sel1) != render_expr(sel2) {
            continue;
        }
        if (method1 == "Lock" && method2 == "Unlock")
            || (method1 == "RLock" && method2 == "RUnlock")
        {
            let pos = block.list[i + 1].pos().0 as u32;
            out.push((pos, "empty critical section".into()));
        }
    }
    out
}

fn unparen_expr(expr: &Expr) -> &Expr {
    match expr {
        Expr::ParenExpr(p) => unparen_expr(&p.x),
        other => other,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass.pkg().pkg_path == "sync_test" {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA2001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::BlockStmt(block) = node else {
            return;
        };
        pending.extend(check_block(pass, block));
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa2001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA2001",
        doc: "empty critical section",
        url: "https://staticcheck.dev/docs/checks/#SA2001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA2001 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa2001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa2001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
