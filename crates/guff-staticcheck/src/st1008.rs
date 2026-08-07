//! ST1008 — a function's error value should be its last return value.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1008`.
//! AST/types-based (upstream iterates IR `SrcFuncs` only to read signatures).

use std::sync::OnceLock;

use guff::ast::{Expr, Field, FuncType};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::type_with_name;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::TypeId;

fn field_type(pass: &Pass<'_>, field: &Field) -> Option<TypeId> {
    let ty = field.ty.as_ref()?;
    let info = pass.types_info()?;
    Some(info.types.get(&ty.id())?.typ)
}

fn is_basic_named(pass: &Pass<'_>, typ: TypeId, want: &str) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let under = unalias_readonly(&artifacts.types, typ);
    type_with_name(pass, under, want)
}

fn check_results(pass: &Pass<'_>, ft: &FuncType, pending: &mut Vec<(u32, String)>) {
    let Some(results) = &ft.results else {
        return;
    };
    // Expand names: each Field may declare multiple names sharing one type.
    let mut rets: Vec<(&Field, TypeId)> = Vec::new();
    for field in &results.list {
        let Some(typ) = field_type(pass, field) else {
            continue;
        };
        if field.names.is_empty() {
            rets.push((field, typ));
        } else {
            for _ in &field.names {
                rets.push((field, typ));
            }
        }
    }
    if rets.len() < 2 {
        return;
    }

    let last = rets[rets.len() - 1].1;
    if is_basic_named(pass, last, "error") {
        return;
    }
    if is_basic_named(pass, last, "bool")
        && rets.len() >= 2
        && is_basic_named(pass, rets[rets.len() - 2].1, "error")
    {
        // Accept (..., error, bool) as comma-ok style.
        return;
    }

    for i in (0..rets.len() - 1).rev() {
        if is_basic_named(pass, rets[i].1, "error") {
            // Upstream reports the field's *last* name when it has names, and
            // the type otherwise: `(a, b error, c int)` reports on `b`, while
            // `(error, int)` reports on `error`. Verified against
            // golangci-lint 2.12.2 with `(a, b, c error, d int)` -> `c`.
            let field = rets[i].0;
            let pos = match (field.names.last(), &field.ty) {
                (Some(name), _) => name.pos().0 as u32,
                (None, Some(e)) => e.pos().0 as u32,
                (None, None) => continue,
            };
            pending.push((
                pos,
                "error should be returned as the last argument".into(),
            ));
            return;
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1008 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |node| {
        match node {
            NodeRef::FuncDecl(fd) => check_results(pass, &fd.ty, &mut pending),
            NodeRef::FuncLit(fl) => check_results(pass, &fl.ty, &mut pending),
            _ => {}
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn st1008_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1008",
        doc: "A function's error value should be its last return value",
        url: "https://staticcheck.dev/docs/checks/#ST1008",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1008_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1008_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
