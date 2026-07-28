//! SA6001 — map indexing with `string(key)` should inline conversion.
//!
//! AST simplification of `honnef.co/go/tools/staticcheck/sa6001`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, IndexExpr, Stmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;

fn is_byte_slice_type(pass: &Pass<'_>, expr: &Expr) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(tav) = info.types.get(&expr.id()) else {
        return false;
    };
    let TypeData::Slice(s) = artifacts.types.get(tav.typ.underlying(&artifacts.types)) else {
        return false;
    };
    let elem = s.elem().underlying(&artifacts.types);
    matches!(
        artifacts.types.get(elem),
        TypeData::Basic(b) if b.kind() == BasicKind::Uint8
    )
}

fn is_string_convert(call: &CallExpr) -> bool {
    matches!(&*call.fun, Expr::Ident(Ident { name, .. }) if name == "string")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA6001 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt), pass.files(), |n| {
        let NodeRef::AssignStmt(assign) = n else {
            return;
        };
        if let Some(pos) = check_assign(pass, assign) {
            pending.push(pos);
        }
    });
    for pos in pending {
        pass.report_unless_generated(
            pos,
            "m[string(key)] would be more efficient than k := string(key); m[k]",
        );
    }
    Ok(None)
}

fn check_assign(pass: &Pass<'_>, assign: &AssignStmt) -> Option<u32> {
    if assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    let Expr::Ident(key) = &assign.lhs[0] else {
        return None;
    };
    let Expr::CallExpr(call) = &assign.rhs[0] else {
        return None;
    };
    if !is_string_convert(call) || !is_byte_slice_type(pass, &call.args[0]) {
        return None;
    }
    let key_obj = object_of(pass, key)?;
    let mut map_lookups = 0;
    let mut only_maps = true;
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())?;
    inspect.preorder_typed(node_mask!(IndexExpr), pass.files(), |n| {
        let NodeRef::IndexExpr(IndexExpr { x, index, .. }) = n else {
            return;
        };
        let Expr::Ident(id) = index.as_ref() else {
            return;
        };
        if object_of(pass, id) != Some(key_obj) {
            return;
        }
        let info = pass.types_info().unwrap();
        let artifacts = pass.pkg().type_artifacts.as_ref().unwrap();
        let map_typ = info.types.get(&x.id()).map(|t| t.typ);
        if map_typ.is_some_and(|t| {
            matches!(
                artifacts.types.get(t.underlying(&artifacts.types)),
                TypeData::Map(_)
            )
        }) {
            map_lookups += 1;
        } else {
            only_maps = false;
        }
    });
    if map_lookups >= 2 && only_maps {
        Some(assign.tok_pos.0 as u32)
    } else {
        None
    }
}

fn sa6001_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA6001",
        doc: "missing an optimization opportunity when indexing maps by byte slices",
        url: "https://staticcheck.dev/docs/checks/#SA6001",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa6001_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa6001_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
