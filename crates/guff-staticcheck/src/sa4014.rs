//! SA4014 — duplicate conditions in if/else if chain
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4014`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};


use std::collections::HashMap;

use guff::ast::{Expr, IfStmt, Stmt};
use guff::walk::NodeRef;

use crate::render::render_expr;

fn may_have_side_effects(expr: &Expr) -> bool {
    matches!(expr, Expr::CallExpr(_))
}

fn collect_conds(if_: &IfStmt) -> Option<Vec<&Expr>> {
    if if_.init.is_some() || may_have_side_effects(&if_.cond) { return None; }
    let mut conds = vec![&if_.cond];
    let mut cur = if_.else_.as_deref();
    while let Some(Stmt::IfStmt(elif)) = cur {
        if elif.init.is_some() || may_have_side_effects(&elif.cond) { return None; }
        conds.push(&elif.cond);
        cur = elif.else_.as_deref();
    }
    Some(conds)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4014 requires inspect analyzer".to_string())?
        .clone();
    let mut seen_ifs: HashMap<u32, ()> = HashMap::new();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::IfStmt(if_) = node else { return };
        if if_.else_.is_none() { return; }
        let Some(conds) = collect_conds(if_) else { return };
        if conds.len() < 2 { return; }
        let mut counts: HashMap<String, usize> = HashMap::new();
        for cond in conds {
            let s = render_expr(cond);
            let c = counts.entry(s).or_insert(0);
            *c += 1;
            if *c == 2 {
                pending.push((cond.id() as u32, "this condition occurs multiple times in this if/else if chain".into()));
            }
        }
    });
    for (pos, msg) in pending { pass.reportf(pos, msg); }
    Ok(None)
}


fn sa4014_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4014",
        doc: "duplicate conditions in if/else if chain",
        url: "https://staticcheck.dev/docs/checks/#SA4014",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4014_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4014_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
