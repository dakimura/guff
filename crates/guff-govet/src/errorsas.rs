//! `errorsas` — check second argument to `errors.As`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, ExprStmt, SelectorExpr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::api_assignable_to;

use crate::expreq::unparen;

fn universe_iface_type(pass: &Pass<'_>, name: &str) -> Option<guff_types::TypeId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for tv in info.types.values() {
        let t = tv.typ;
        if !guff_types::predicates::is_valid(&artifacts.types, t) {
            continue;
        }
        let type_name = guff_types::typestring::type_string(
            &artifacts.types,
            &artifacts.objects,
            &artifacts.packages,
            t,
            None,
        );
        if type_name != name {
            continue;
        }
        if matches!(
            artifacts.types.get(t.underlying(&artifacts.types)),
            TypeData::Interface(_)
        ) {
            return Some(t);
        }
    }
    None
}

fn is_errors_as(fun: &Expr) -> bool {
    match unparen(fun) {
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            matches!(x.as_ref(), Expr::Ident(id) if id.name == "errors") && sel.name == "As"
        }
        _ => false,
    }
}

fn check_as_target(pass: &Pass<'_>, arg: &Expr) -> Option<&'static str> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tv = info.types.get(&arg.id())?;
    let t = tv.typ;
    if !guff_types::predicates::is_valid(&artifacts.types, t) {
        return None;
    }
    let u = t.underlying(&artifacts.types);
    let type_name = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        u,
        None,
    );
    if type_name == "any" {
        return None;
    }
    let (elem, ok) = match artifacts.types.get(u) {
        TypeData::Pointer(p) => (p.elem(), true),
        _ => (t, false),
    };
    if !ok {
        return Some(
            "second argument to errors.As must be a non-nil pointer to either a type that implements error, or to any interface type",
        );
    }
    let eu = elem.underlying(&artifacts.types);
    let elem_name = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        elem,
        None,
    );
    if elem_name == "error" {
        return Some("second argument to errors.As should not be *error");
    }
    let error_iface = universe_iface_type(pass, "error")?;
    if matches!(artifacts.types.get(eu), TypeData::Interface(_)) {
        return None;
    }
    let mut types = artifacts.types.clone();
    if api_assignable_to(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        elem,
        error_iface,
    ) {
        return None;
    }
    Some(
        "second argument to errors.As must be a non-nil pointer to either a type that implements error, or to any interface type",
    )
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if matches!(pass.pkg().pkg_path.as_str(), "errors" | "errors_test") {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errorsas requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !is_errors_as(&call.fun) {
            return;
        }
        if call.args.len() < 2 {
            return;
        }
        if let Some(msg) = check_as_target(pass, &call.args[1]) {
            pending.push((call.lparen.0 as u32, msg.to_string()));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "errorsas",
        doc: "report passing non-pointer or non-error values to errors.As",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/errorsas",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
