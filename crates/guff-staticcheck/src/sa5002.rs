//! SA5002 — empty `for {}` loop spins.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5002`.

use std::sync::OnceLock;

use guff::ast::{Expr, ForStmt};
use guff::walk::NodeRef;
use guff_analysis::code::{is_bool_const, predeclared_bool_ident};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn may_have_side_effects(expr: &Expr) -> bool {
    match expr {
        Expr::CallExpr(_) => true,
        Expr::UnaryExpr(u) => may_have_side_effects(&u.x),
        Expr::BinaryExpr(b) => may_have_side_effects(&b.x) || may_have_side_effects(&b.y),
        Expr::IndexExpr(i) => may_have_side_effects(&i.x) || may_have_side_effects(&i.index),
        Expr::SelectorExpr(s) => may_have_side_effects(&s.x),
        Expr::StarExpr(s) => may_have_side_effects(&s.x),
        Expr::ParenExpr(p) => may_have_side_effects(&p.x),
        Expr::SliceExpr(s) => {
            may_have_side_effects(&s.x)
                || s.low.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.high.as_ref().is_some_and(|e| may_have_side_effects(e))
                || s.max.as_ref().is_some_and(|e| may_have_side_effects(e))
        }
        _ => false,
    }
}

fn check_loop(pass: &Pass<'_>, loop_: &ForStmt, pending: &mut Vec<(u32, String)>) {
    if !loop_.body.list.is_empty() || loop_.post.is_some() || loop_.init.is_some() {
        return;
    }
    if let Some(cond) = &loop_.cond {
        if may_have_side_effects(cond) {
            return;
        }
        if let Expr::Ident(ident) = cond {
            if let Some(false) = predeclared_bool_ident(pass, ident) {
                return;
            }
            if is_bool_const(pass, cond) && !guff_analysis::code::bool_const(pass, cond) {
                return;
            }
        }
        pending.push((
            loop_.for_.0 as u32,
            "loop condition never changes or has a race condition".into(),
        ));
    }
    pending.push((
        loop_.for_.0 as u32,
        "this loop will spin, using 100% CPU".into(),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA5002 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::ForStmt(loop_) = n else {
            return;
        };
        check_loop(pass, loop_, &mut pending);
    });
    for (pos, msg) in pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa5002_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA5002",
        doc: "the empty for loop (for {}) spins and can block the scheduler",
        url: "https://staticcheck.dev/docs/checks/#SA5002",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa5002_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa5002_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
