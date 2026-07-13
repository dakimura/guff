//! SA9008 — else branch of a type assertion reads the wrong value.
//!
//! Simplified port of `honnef.co/go/tools/staticcheck/sa9008`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, IfStmt, Ident, Stmt, TypeAssertExpr};
use guff::walk::{NodeRef, preorder, stmt_ref};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9008 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::IfStmt(ifs) = n else {
            return;
        };
        check_if(pass, ifs, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn check_if(pass: &Pass<'_>, ifs: &IfStmt, pending: &mut Vec<(u32, String)>) {
    let Some(init) = ifs.init.as_deref() else {
        return;
    };
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, .. }) = init else {
        return;
    };
    if lhs.len() != 2 {
        return;
    };
    let Expr::Ident(obj) = &lhs[0] else {
        return;
    };
    let Expr::Ident(ok) = &lhs[1] else {
        return;
    };
    if ok.name != "ok" && object_of(pass, ok).is_some() {
        return;
    }
    let Expr::TypeAssertExpr(TypeAssertExpr { x, .. }) = &rhs[0] else {
        return;
    };
    let obj_id = object_of(pass, obj);
    let Some(else_) = &ifs.else_ else {
        return;
    };
    preorder(stmt_ref(else_.as_ref()), &mut |n| {
        let NodeRef::Ident(id) = n else {
            return true;
        };
        if object_of(pass, id) == obj_id {
            pending.push((
                id.name_pos.0 as u32,
                format!(
                    "{} refers to the result of a failed type assertion and is a zero value, not the value that was being type-asserted",
                    id.name
                ),
            ));
        }
        true
    });
    let _ = x;
}

fn sa9008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9008",
        doc: "else branch of a type assertion is probably not reading the right value",
        url: "https://staticcheck.dev/docs/checks/#SA9008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
