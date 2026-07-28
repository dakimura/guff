//! QF1011 — omit redundant type from variable declaration.
//!
//! Port of `honnef.co/go/tools/quickfix/qf1011` via
//! `sharedcheck.RedundantTypeInDeclarationChecker("could", true)`.
//! Same approximation as ST1023, but with `flagHelpfulTypes = true`:
//! flags blank identifiers, named constants, and untyped binary/unary RHS.

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use guff_types::arena::{ObjectData, TypeData};
use guff_types::basic::BasicKind;
use guff_types::predicates::identical;
use guff_types::typestring::type_string;
use guff_types::TypeId;

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn is_basic_kind(pass: &Pass<'_>, typ: TypeId, kind: BasicKind) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(artifacts.types.get(typ), TypeData::Basic(b) if b.kind() == kind)
}

fn lit_default_matches_lhs(pass: &Pass<'_>, lit_kind: Token, tlhs: TypeId) -> bool {
    match lit_kind {
        Token::INT => is_basic_kind(pass, tlhs, BasicKind::Int),
        Token::FLOAT => is_basic_kind(pass, tlhs, BasicKind::Float64),
        Token::IMAG => is_basic_kind(pass, tlhs, BasicKind::Complex128),
        Token::CHAR => is_basic_kind(pass, tlhs, BasicKind::Int32),
        Token::STRING => is_basic_kind(pass, tlhs, BasicKind::String),
        _ => false,
    }
}

/// `flagHelpfulTypes = true`: allow named package constants and binary/unary.
fn rhs_allows_redundant_flag(pass: &Pass<'_>, v: &Expr, tlhs: TypeId) -> bool {
    match v {
        Expr::BasicLit(lit) => {
            let Some(kind) = lit.kind else {
                return false;
            };
            lit_default_matches_lhs(pass, kind, tlhs)
        }
        Expr::Ident(id) => {
            let Some(obj) = object_of(pass, id) else {
                return true;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return false;
            };
            match artifacts.objects.get(obj) {
                ObjectData::Const(_) => true, // named + predeclared both OK when helpful
                _ => true,
            }
        }
        Expr::ParenExpr(p) => rhs_allows_redundant_flag(pass, &p.x, tlhs),
        // flagHelpfulTypes: also flag untyped binary/unary when types identical.
        Expr::BinaryExpr(_) | Expr::UnaryExpr(_) => true,
        _ => true,
    }
}

fn check_gen_decl(pass: &Pass<'_>, gen: &guff::ast::GenDecl, pending: &mut Vec<(u32, u32, String)>) {
    if gen.tok != Some(Token::VAR) {
        return;
    }
    'spec: for spec in &gen.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        let Some(ty_expr) = &vs.ty else {
            continue;
        };
        if vs.names.len() != vs.values.len() {
            continue;
        }
        let info = match pass.types_info() {
            Some(i) => i,
            None => continue,
        };
        let Some(tlhs) = info.types.get(&ty_expr.id()).map(|tv| tv.typ) else {
            continue;
        };
        for (i, v) in vs.values.iter().enumerate() {
            // flagHelpfulTypes: do not skip blank identifiers.
            let _ = &vs.names[i];
            let Some(trhs) = info.types.get(&v.id()).map(|tv| tv.typ) else {
                continue 'spec;
            };
            if !types_identical(pass, tlhs, trhs) {
                continue 'spec;
            }
            if !rhs_allows_redundant_flag(pass, v, tlhs) {
                continue 'spec;
            }
        }
        let typ_s = {
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                continue;
            };
            type_string(
                &artifacts.types,
                &artifacts.objects,
                &artifacts.packages,
                tlhs,
                None,
            )
        };
        pending.push((
            ty_expr.pos().0 as u32,
            ty_expr.end().0 as u32,
            format!(
                "could omit type {typ_s} from declaration; it will be inferred from the right-hand side"
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    // flagHelpfulTypes=true: do not skip syscall/unsafe packages.

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "QF1011 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(DeclStmt), pass.files(), |node| {
        let NodeRef::DeclStmt(ds) = node else {
            return;
        };
        let Decl::GenDecl(gen) = &ds.decl else {
            return;
        };
        check_gen_decl(pass, gen, &mut pending);
    });

    for (pos, end, message) in pending {
        pass.report(Diagnostic {
            pos,
            end,
            message: message.clone(),
            suggested_fixes: vec![SuggestedFix {
                message: "Remove redundant type".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }],
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

fn qf1011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "QF1011",
        doc: "omit redundant type from variable declaration",
        url: "https://staticcheck.dev/docs/checks/#QF1011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(qf1011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn qf1011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
