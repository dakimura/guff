//! SA4030 — ineffective attempt at generating random number.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4030`.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, is_integer_literal};

use crate::render::render_expr;

fn is_rand_intn(name: &str) -> bool {
    name.contains("rand") && (name.ends_with("Intn") || name.ends_with("Int31n") || name.ends_with("Int63n"))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4030 requires inspect analyzer".to_string())?
        .clone();
    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        if call.args.len() != 1 || !is_integer_literal(pass, &call.args[0], 1) {
            return;
        }
        let Some(name) = call_name(pass, &call.fun) else {
            return;
        };
        if !is_rand_intn(&name) {
            return;
        }
        let rendered = render_expr(&guff::ast::Expr::CallExpr(call.clone()));
        pending.push((
            match_pos(node),
            format!(
                "{name}(n) generates a random value 0 <= x < n; that is, the generated values don't include n; {rendered} therefore always returns 0"
            ),
        ));
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4030_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4030",
        doc: "ineffective attempt at generating random number",
        url: "https://staticcheck.dev/docs/checks/#SA4030",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4030_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4030_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
