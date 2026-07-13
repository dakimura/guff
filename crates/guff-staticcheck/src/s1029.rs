//! S1029 — range over the string directly (not `[]rune(s)`).
//!
//! AST simplification of `honnef.co/go/tools/simple/s1029` (Go uses buildir IR).

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

fn check_range(pass: &Pass<'_>, rng: &RangeStmt) -> Option<u32> {
    let key = rng.key.as_ref()?;
    let Expr::Ident(key_id) = key else {
        return None;
    };
    if !is_blank(key_id) {
        return None;
    }
    let Expr::CallExpr(call) = &rng.x else {
        return None;
    };
    if !is_rune_slice_conversion(pass, call) {
        return None;
    }
    Some(rng.for_.0 as u32)
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1029 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<u32> = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::RangeStmt(rng) = n else {
            return;
        };
        if let Some(pos) = check_range(pass, rng) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(pos, "should range over string, not []rune(string)");
    }
    Ok(None)
}

fn s1029_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1029",
        doc: "range over the string directly",
        url: "https://staticcheck.dev/docs/checks/#S1029",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1029_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1029_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
