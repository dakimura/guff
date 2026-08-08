//! S1040 — type assertion to current type.
//!
//! Port of `honnef.co/go/tools/simple/s1040`.

use guff::ast::Expr;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::predicates::is_interface;
use guff_types::TypeId;
use std::sync::OnceLock;

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn render_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types,
        &a.objects,
        &a.packages,
        typ,
        None,
    ))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1040 requires inspect analyzer".to_string())?
        .clone();

    if pass.pkg().type_artifacts.is_none() {
        return Ok(None);
    }

    let mut pending: Vec<(u32, String)> = Vec::new();

    inspect.preorder_typed(node_mask!(TypeAssertExpr), pass.files(), |n| {
        let NodeRef::TypeAssertExpr(expr) = n else {
            return;
        };
        if expr.ty.is_none() {
            return;
        }
        let Some(t1) = expr_type(pass, expr.ty.as_ref().unwrap()) else {
            return;
        };
        let Some(t2) = expr_type(pass, &expr.x) else {
            return;
        };
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return;
        };
        if is_interface(&artifacts.types, t1) && render_type(pass, t1) == render_type(pass, t2) {
            // Upstream names the operand and its type — `i already has type
            // interface{}`, `e already has type error` — and reports the
            // assertion node, whose position is the start of the operand.
            let operand = crate::render::render_expr(&expr.x);
            let Some(typ) = render_type(pass, t2) else {
                return;
            };
            pending.push((
                expr.x.pos().0 as u32,
                format!("type assertion to the same type: {operand} already has type {typ}"),
            ));
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1040_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1040",
        doc: "type assertion to current type",
        url: "https://staticcheck.dev/docs/checks/#S1040",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1040_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1040_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
