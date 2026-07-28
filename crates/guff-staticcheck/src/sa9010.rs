//! SA9010 — returned function should be called in defer.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9010`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, DeferStmt, Expr};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::{ObjectArena, TypeData, TypeId};
use guff_types::signature::signature_results;
use guff_types::tuple::{tuple_at, tuple_len};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9010 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(DeferStmt), pass.files(), |n| {
        let NodeRef::DeferStmt(def) = n else {
            return;
        };
        if defer_returns_function(pass, def) {
            pending.push(def.defer_.0 as u32);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, "deferred return function not called");
    }
    Ok(None)
}

fn defer_returns_function(pass: &Pass<'_>, def: &DeferStmt) -> bool {
    let Some(result_typ) = defer_call_type(pass, &def.call) else {
        return false;
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    matches!(
        artifacts.types.get(result_typ.underlying(&artifacts.types)),
        TypeData::Signature(_)
    )
}

fn defer_call_type(pass: &Pass<'_>, call: &CallExpr) -> Option<TypeId> {
    let info = pass.types_info()?;
    if call.id != 0 {
        if let Some(tav) = info.types.get(&call.id) {
            return Some(tav.typ);
        }
    }
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let callee_sig = callee_signature(pass, &call.fun, &artifacts.objects)?;
    let results = signature_results(&artifacts.types, callee_sig)?;
    if tuple_len(&artifacts.types, Some(results)) != 1 {
        return None;
    }
    Some(tuple_at(&artifacts.types, results, 0).typ(&artifacts.objects)?)
}

fn callee_signature(pass: &Pass<'_>, fun: &Expr, objects: &ObjectArena) -> Option<TypeId> {
    let obj = match fun {
        Expr::Ident(id) => object_of(pass, id)?,
        _ => return None,
    };
    obj.typ(objects)
}

fn sa9010_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9010",
        doc: "returned function should be called in defer",
        url: "https://staticcheck.dev/docs/checks/#SA9010",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9010_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9010_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
