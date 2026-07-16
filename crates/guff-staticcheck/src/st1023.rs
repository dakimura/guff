//! ST1023 — redundant type in variable declaration.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1023` via
//! `sharedcheck.RedundantTypeInDeclarationChecker("should", false)`.
//! Approximates upstream `types.CheckExpr` by classifying RHS AST shapes
//! (BasicLit default kinds / named vs predeclared consts / typed exprs).
//! Only function-local decls are flagged (`DeclStmt`); package-level vars are
//! skipped for godoc readability (matches upstream).

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec};
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

/// Whether the RHS is safe to treat as "typed enough" that an identical LHS
/// type is truly redundant (without re-running `CheckExpr`).
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
                ObjectData::Const(_) => {
                    // Named package constants: keep explicit type for readability.
                    // Predeclared (true/false/…) have no package.
                    obj.pkg(&artifacts.objects).is_none()
                }
                _ => true,
            }
        }
        Expr::ParenExpr(p) => rhs_allows_redundant_flag(pass, &p.x, tlhs),
        // Untyped composites (binary/unary/…) — skip unless flagHelpfulTypes.
        Expr::BinaryExpr(_) | Expr::UnaryExpr(_) => false,
        // Typed expressions (calls, selectors, …).
        _ => true,
    }
}

fn import_path(spec: &guff::ast::ImportSpec) -> String {
    spec.path.value.trim_matches('"').to_string()
}

fn package_imports_low_level(pass: &Pass<'_>) -> bool {
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::ImportSpec(is) = spec else {
                    continue;
                };
                let path = import_path(is);
                if path == "syscall" || path == "unsafe" {
                    return true;
                }
            }
        }
    }
    false
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
            if vs.names[i].name == "_" {
                continue 'spec;
            }
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
                "should omit type {typ_s} from declaration; it will be inferred from the right-hand side"
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if package_imports_low_level(pass) {
        return Ok(None);
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1023 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, u32, String)> = Vec::new();
    // DeclStmt only appears inside function / func-lit bodies — package-level
    // vars are GenDecl on File.decls and are intentionally not flagged.
    inspect.preorder(pass.files(), |node| {
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

fn st1023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1023",
        doc: "redundant type in variable declaration",
        url: "https://staticcheck.dev/docs/checks/#ST1023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
