//! SA1006 — `Printf` with dynamic first argument and no further arguments.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1006`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, is_call_to_any};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;

const PRINTF_ONE_ARG: &[&str] = &[
    "fmt.Errorf",
    "fmt.Printf",
    "fmt.Sprintf",
    "log.Fatalf",
    "log.Panicf",
    "log.Printf",
    "(*log.Logger).Printf",
    "(*testing.common).Logf",
    "(*testing.common).Errorf",
    "(*testing.common).Fatalf",
    "(*testing.common).Skipf",
    "(testing.TB).Logf",
    "(testing.TB).Errorf",
    "(testing.TB).Fatalf",
    "(testing.TB).Skipf",
];

fn is_dynamic_format_arg(expr: &Expr) -> bool {
    matches!(expr, Expr::CallExpr(_) | Expr::Ident(_))
}

fn is_splatted_tuple(pass: &Pass<'_>, expr: &Expr) -> bool {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return false,
    };
    let typ = match info.types.get(&expr.id()) {
        Some(tv) => tv.typ,
        None => return false,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    matches!(
        artifacts.types.get(typ.underlying(&artifacts.types)),
        TypeData::Tuple(_)
    )
}

fn check_printf_call(pass: &Pass<'_>, call: &CallExpr, pending: &mut Vec<(u32, String)>) {
    let format = if is_call_to(pass, call, "fmt.Fprintf") {
        if call.args.len() != 2 {
            return;
        }
        &call.args[1]
    } else if is_call_to_any(pass, call, PRINTF_ONE_ARG) {
        if call.args.len() != 1 {
            return;
        }
        &call.args[0]
    } else {
        return;
    };

    if !is_dynamic_format_arg(format) || is_splatted_tuple(pass, format) {
        return;
    }
    pending.push((
        call.lparen.0 as u32,
        "printf-style function with dynamic format string and no further arguments should use print-style function instead"
            .into(),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1006 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |node| {
        let NodeRef::CallExpr(call) = node else {
            return;
        };
        check_printf_call(pass, call, &mut pending);
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn sa1006_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1006",
        doc: "Printf with dynamic first argument and no further arguments",
        url: "https://staticcheck.dev/docs/checks/#SA1006",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1006 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1006_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1006_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
