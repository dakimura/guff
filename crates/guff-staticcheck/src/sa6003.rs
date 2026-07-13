//! SA6003 — converting a string to a slice of runes before ranging over it.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa6003` (same logic as S1029).

use std::sync::OnceLock;

use guff::ast::{ArrayType, CallExpr, Expr, Ident, RangeStmt};
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn is_blank(ident: &Ident) -> bool {
    ident.name == "_"
}

fn is_rune_slice_conversion(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::ArrayType(ArrayType { len, elt, .. }) = &*call.fun else {
        return false;
    };
    if len.is_some() || call.args.len() != 1 {
        return false;
    }
    let Expr::Ident(elem) = &**elt else {
        return false;
    };
    if elem.name != "rune" && elem.name != "int32" {
        return false;
    }
    let info = match pass.types_info() {
        Some(i) => i,
        None => return true,
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return true,
    };
    let tav = match info.types.get(&call.args[0].id()) {
        Some(t) => t,
        None => return false,
    };
    matches!(
        artifacts.types.get(tav.typ.underlying(&artifacts.types)),
        TypeData::Basic(b) if b.kind() == BasicKind::String
    )
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6003 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::RangeStmt(rng) = n else {
            return;
        };
        let Some(key) = rng.key.as_ref() else {
            return;
        };
        let Expr::Ident(key_id) = key else {
            return;
        };
        if !is_blank(key_id) {
            return;
        }
        let Expr::CallExpr(call) = &rng.x else {
            return;
        };
        if is_rune_slice_conversion(pass, call) {
            pending.push(rng.for_.0 as u32);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, "should range over string, not []rune(string)");
    }
    Ok(None)
}

fn sa6003_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6003",
        doc: "converting a string to a slice of runes before ranging over it",
        url: "https://staticcheck.dev/docs/checks/#SA6003",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6003_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6003_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
