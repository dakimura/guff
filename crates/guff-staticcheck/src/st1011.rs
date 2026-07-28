//! ST1011 — poorly chosen name for variable of type `time.Duration`.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1011`.
//!
//! DEFERRED: rely on `Info.Defs` for struct fields once guff-types
//! `struct_check` records them (currently a no-op; we fall back to the
//! field's type expression).

use std::sync::OnceLock;

use guff::ast::{Expr, Field, Ident, SelectorExpr, StarExpr};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, type_with_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::TypeId;

const SUFFIXES: &[&str] = &[
    "Sec",
    "Secs",
    "Seconds",
    "Msec",
    "Msecs",
    "Milli",
    "Millis",
    "Milliseconds",
    "Usec",
    "Usecs",
    "Microseconds",
    "MS",
    "Ms",
];

fn is_duration(pass: &Pass<'_>, typ: TypeId) -> bool {
    type_with_name(pass, typ, "time.Duration") || type_with_name(pass, typ, "*time.Duration")
}

fn type_string(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return "time.Duration".into();
    };
    guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn ast_is_duration(expr: &Expr) -> bool {
    match expr {
        Expr::SelectorExpr(SelectorExpr { x, sel, .. }) => {
            matches!(x.as_ref(), Expr::Ident(id) if id.name == "time") && sel.name == "Duration"
        }
        Expr::StarExpr(StarExpr { x, .. }) => ast_is_duration(x),
        Expr::ParenExpr(p) => ast_is_duration(&p.x),
        _ => false,
    }
}

fn report_suffix(
    name: &Ident,
    typ_label: &str,
    pending: &mut Vec<(u32, String)>,
) {
    for suffix in SUFFIXES {
        if name.name.ends_with(suffix) {
            pending.push((
                name.name_pos.0 as u32,
                format!(
                    "var {} is of type {}; don't use unit-specific suffix {:?}",
                    name.name, typ_label, suffix
                ),
            ));
            break;
        }
    }
}

fn check_names(pass: &Pass<'_>, names: &[Ident], pending: &mut Vec<(u32, String)>) {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };
    let Some(info) = pass.types_info() else {
        return;
    };
    for name in names {
        // Upstream skips idents that are not in Defs (e.g. reassignment LHS).
        if !info.defs.contains_key(&name.id) {
            continue;
        }
        let Some(obj) = object_of(pass, name) else {
            continue;
        };
        let Some(typ) = obj.typ(&artifacts.objects) else {
            continue;
        };
        if !is_duration(pass, typ) {
            continue;
        }
        report_suffix(name, &type_string(pass, typ), pending);
    }
}

fn check_field(pass: &Pass<'_>, field: &Field, pending: &mut Vec<(u32, String)>) {
    if field.names.is_empty() {
        return;
    }
    let Some(info) = pass.types_info() else {
        return;
    };
    // Params / results have Defs; struct fields currently do not.
    if field.names.iter().all(|n| info.defs.contains_key(&n.id)) {
        check_names(pass, &field.names, pending);
        return;
    }
    let Some(ty_expr) = &field.ty else {
        return;
    };
    let typ_label = if pass.pkg().type_artifacts.is_some() {
        if let Some(typ) = info.types.get(&ty_expr.id()).map(|tv| tv.typ) {
            if !is_duration(pass, typ) {
                return;
            }
            type_string(pass, typ)
        } else if ast_is_duration(ty_expr) {
            "time.Duration".into()
        } else {
            return;
        }
    } else if ast_is_duration(ty_expr) {
        "time.Duration".into()
    } else {
        return;
    };
    for name in &field.names {
        report_suffix(name, &typ_label, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1011 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(AssignStmt, Field, ValueSpec), pass.files(), |node| {
        match node {
            NodeRef::ValueSpec(spec) => {
                check_names(pass, &spec.names, &mut pending);
            }
            NodeRef::Field(field) => {
                check_field(pass, field, &mut pending);
            }
            NodeRef::AssignStmt(stmt) if stmt.tok == Some(Token::DEFINE) => {
                let names: Vec<Ident> = stmt
                    .lhs
                    .iter()
                    .filter_map(|e| match e {
                        Expr::Ident(id) => Some(id.clone()),
                        _ => None,
                    })
                    .collect();
                check_names(pass, &names, &mut pending);
            }
            _ => {}
        }
    });

    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn st1011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1011",
        doc: "poorly chosen name for variable of type time.Duration",
        url: "https://staticcheck.dev/docs/checks/#ST1011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
