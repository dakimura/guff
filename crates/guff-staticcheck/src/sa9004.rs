//! SA9004 — only the first constant has an explicit type.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa9004`.

use std::sync::OnceLock;

use guff::ast::{BasicLit, Expr, GenDecl, ValueSpec};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::api_predicates::api_convertible_to;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA9004 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |n| {
        let NodeRef::GenDecl(decl) = n else {
            return;
        };
        check_const_decl(pass, decl, &mut pending);
    });
    for pos in pending {
        pass.report_unless_generated(pos, "only the first constant in this group has an explicit type");
    }
    Ok(None)
}

fn check_const_decl(pass: &Pass<'_>, decl: &GenDecl, pending: &mut Vec<u32>) {
    if decl.tok != Some(Token::CONST) || !decl.lparen.is_valid() {
        return;
    }
    let groups = group_specs(decl);
    for group in groups {
        if group.len() < 2 {
            continue;
        }
        let first = group[0];
        if first.ty.is_none() {
            continue;
        }
        let info = match pass.types_info() {
            Some(i) => i,
            None => continue,
        };
        let first_type = match first.values.first() {
            Some(v) => info.types.get(&v.id()).map(|t| t.typ),
            None => None,
        };
        let Some(first_type) = first_type else {
            continue;
        };
        let mut ok_group = true;
        for spec in group.iter().skip(1) {
            if spec.ty.is_some()
                || spec.names.len() != 1
                || spec.values.len() != 1
                || !is_simple_literal(&spec.values[0])
            {
                ok_group = false;
                break;
            }
            let typ = info.types.get(&spec.values[0].id()).map(|t| t.typ);
            let Some(typ) = typ else {
                ok_group = false;
                break;
            };
            let artifacts = match pass.pkg().type_artifacts.as_ref() {
                Some(a) => a,
                None => {
                    ok_group = false;
                    break;
                }
            };
            if !api_convertible_to(
                &mut artifacts.types.clone(),
                &artifacts.objects,
                &artifacts.packages,
                typ,
                first_type,
            ) {
                ok_group = false;
                break;
            }
        }
        if ok_group {
            pending.push(
                first
                    .ty
                    .as_ref()
                    .map(|t| t.pos().0 as u32)
                    .unwrap_or(decl.lparen.0 as u32),
            );
        }
    }
}

fn is_simple_literal(expr: &Expr) -> bool {
    match expr {
        Expr::BasicLit(BasicLit { .. }) => true,
        Expr::UnaryExpr(u) => matches!(&*u.x, Expr::BasicLit(_)),
        _ => false,
    }
}

fn group_specs(decl: &GenDecl) -> Vec<Vec<&ValueSpec>> {
    let mut groups = Vec::new();
    let mut current: Vec<&ValueSpec> = Vec::new();
    for spec in &decl.specs {
        let guff::ast::Spec::ValueSpec(vs) = spec else {
            continue;
        };
        if vs.values.is_empty() && !current.is_empty() {
            groups.push(current);
            current = Vec::new();
        }
        current.push(vs);
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn sa9004_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA9004",
        doc: "only the first constant has an explicit type",
        url: "https://staticcheck.dev/docs/checks/#SA9004",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa9004_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa9004_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
