//! `reflectvaluecompare` — `==`, `!=` and `reflect.DeepEqual` on `reflect.Value`.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/reflectvaluecompare`
//! (x/tools v0.52.0).
//!
//! Unlike most of its neighbours it has no direct-import gate: a package that
//! only reaches `reflect.Value` through another package's API is checked too.
//! `isReflectValue` looks through aliases (`typesinternal.IsTypeNamed` calls
//! `types.Unalias`) but not through defined types, and exempts an operand that
//! *is* a composite literal — `reflect.Value{}` — syntactically: the same
//! literal in parentheses is no longer an `*ast.CompositeLit` and is reported.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{callee_full_name, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;

fn is_reflect_value(pass: &Pass<'_>, e: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(typ) = info.types.get(&e.id()).map(|tv| tv.typ) else {
        return false;
    };
    if !type_with_name(pass, unalias_readonly(&artifacts.types, typ), "reflect.Value") {
        return false;
    }
    !matches!(e, Expr::CompositeLit(_))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "reflectvaluecompare requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(BinaryExpr, CallExpr), pass.files(), |n| match n {
        NodeRef::BinaryExpr(e) => {
            let message = match e.op {
                Token::EQL => "avoid using == with reflect.Value",
                Token::NEQ => "avoid using != with reflect.Value",
                _ => return,
            };
            if is_reflect_value(pass, &e.x) || is_reflect_value(pass, &e.y) {
                // `ReportRangef(n, …)`: a binary expression starts at its
                // left operand.
                pending.push((e.x.pos().0 as u32, message));
            }
        }
        NodeRef::CallExpr(call) => {
            if callee_full_name(pass, call).as_deref() != Some("reflect.DeepEqual") {
                return;
            }
            let [a, b, ..] = call.args.as_slice() else {
                return;
            };
            if is_reflect_value(pass, a) || is_reflect_value(pass, b) {
                pending.push((call.fun.pos().0 as u32, "avoid using reflect.DeepEqual with reflect.Value"));
            }
        }
        _ => {}
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "reflectvaluecompare",
        doc: "check for comparing reflect.Value values with == or reflect.DeepEqual",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/reflectvaluecompare",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
