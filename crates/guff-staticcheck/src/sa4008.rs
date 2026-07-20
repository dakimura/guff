//! SA4008 — variable in loop condition never changes.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4008`.
//!
//! Upstream only considers loops whose post is an `IncDecStmt`. Assign-form
//! posts (`i += 2`, `t = next()`) are skipped — flagging them was a guff FP on
//! stepped loops in prometheus `model/textparse`.

use std::sync::OnceLock;

use guff::ast::{Expr, ForStmt, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn cond_var_never_incremented(pass: &Pass<'_>, loop_: &ForStmt) -> bool {
    let Some(init) = loop_.init.as_ref() else {
        return false;
    };
    let Stmt::AssignStmt(init) = &**init else {
        return false;
    };
    if init.lhs.len() != 1 {
        return false;
    }
    let Expr::Ident(init_id) = &init.lhs[0] else {
        return false;
    };
    let Some(init_obj) = object_of(pass, init_id) else {
        return false;
    };
    let Some(Expr::BinaryExpr(cond)) = loop_.cond.as_ref() else {
        return false;
    };
    let Expr::Ident(cond_id) = cond.x.as_ref() else {
        return false;
    };
    if object_of(pass, cond_id) != Some(init_obj) {
        return false;
    }
    let Some(post) = loop_.post.as_ref() else {
        return false;
    };
    // Match upstream: only IncDec posts are candidates. `i += n` / `t = f()` are
    // not flagged here (upstream uses IR Phi/Load for the remaining cases).
    let Stmt::IncDecStmt(inc) = &**post else {
        return false;
    };
    let Expr::Ident(inc_id) = &inc.x else {
        return false;
    };
    object_of(pass, inc_id) != Some(init_obj)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4008 requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::ForStmt(loop_) = node else {
            return;
        };
        if cond_var_never_incremented(pass, loop_) {
            if let Some(Expr::BinaryExpr(cond)) = loop_.cond.as_ref() {
                pending.push((
                    cond.op_pos.0 as u32,
                    "variable in loop condition never changes".into(),
                ));
            }
        }
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4008",
        doc: "the variable in the loop condition never changes",
        url: "https://staticcheck.dev/docs/checks/#SA4008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
