//! ST1023 — redundant type in variable declaration.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1023` via
//! `sharedcheck.RedundantTypeInDeclarationChecker("should", false)`.
//!
//! Mirrors upstream `types.CheckExpr` by reconstructing the RHS type in
//! isolation (BasicLit → untyped kind; Ident const → object type; other
//! typed exprs → Info type). Untyped RHS is only flagged when its default
//! type equals the LHS and the AST is a BasicLit or predeclared Ident.
//! Only function-local decls are flagged (`DeclStmt`).

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
use guff_types::basic::{lookup_basic, BasicKind};
use guff_types::predicates::{identical, is_untyped};
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

fn untyped_from_lit_kind(pass: &Pass<'_>, lit_kind: Token) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let kind = match lit_kind {
        Token::INT => BasicKind::UntypedInt,
        Token::FLOAT => BasicKind::UntypedFloat,
        Token::IMAG => BasicKind::UntypedComplex,
        Token::CHAR => BasicKind::UntypedRune,
        Token::STRING => BasicKind::UntypedString,
        _ => return None,
    };
    lookup_basic(&artifacts.types, kind)
}

/// Default type of an untyped basic kind (Go's `types.Default`).
fn default_of_untyped(pass: &Pass<'_>, untyped: TypeId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let TypeData::Basic(b) = artifacts.types.get(untyped) else {
        return Some(untyped);
    };
    let typed = match b.kind() {
        BasicKind::UntypedBool => BasicKind::Bool,
        BasicKind::UntypedInt => BasicKind::Int,
        BasicKind::UntypedRune => BasicKind::Int32,
        BasicKind::UntypedFloat => BasicKind::Float64,
        BasicKind::UntypedComplex => BasicKind::Complex128,
        BasicKind::UntypedString => BasicKind::String,
        _ => return Some(untyped),
    };
    lookup_basic(&artifacts.types, typed)
}

/// Reconstruct the type `types.CheckExpr` would give for `v` in isolation.
fn isolated_rhs_type(pass: &Pass<'_>, v: &Expr) -> Option<TypeId> {
    match v {
        Expr::BasicLit(lit) => {
            let kind = lit.kind?;
            untyped_from_lit_kind(pass, kind)
        }
        Expr::Ident(id) => {
            let obj = object_of(pass, id)?;
            let artifacts = pass.pkg().type_artifacts.as_ref()?;
            match artifacts.objects.get(obj) {
                ObjectData::Const(_) => obj.typ(&artifacts.objects),
                _ => {
                    let info = pass.types_info()?;
                    Some(info.types.get(&v.id())?.typ)
                }
            }
        }
        Expr::ParenExpr(p) => isolated_rhs_type(pass, &p.x),
        // Without a full CheckExpr engine, binary/unary stay opaque after
        // updateExprType — skip (flagHelpfulTypes=false).
        Expr::BinaryExpr(_) | Expr::UnaryExpr(_) => None,
        _ => {
            let info = pass.types_info()?;
            Some(info.types.get(&v.id())?.typ)
        }
    }
}

/// CheckExpr-equivalent gate for `flagHelpfulTypes = false`.
fn rhs_allows_redundant_flag(pass: &Pass<'_>, v: &Expr, tlhs: TypeId) -> bool {
    let Some(isolated) = isolated_rhs_type(pass, v) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    if is_untyped(&artifacts.types, isolated) {
        let Some(def) = default_of_untyped(pass, isolated) else {
            return false;
        };
        if !types_identical(pass, tlhs, def) {
            return false;
        }
        match v {
            Expr::BasicLit(_) => true,
            Expr::Ident(id) => {
                let Some(obj) = object_of(pass, id) else {
                    return false;
                };
                // Predeclared true/false/… have no package.
                obj.pkg(&artifacts.objects).is_none()
            }
            Expr::ParenExpr(p) => rhs_allows_redundant_flag(pass, &p.x, tlhs),
            _ => false,
        }
    } else {
        match v {
            Expr::Ident(id) => {
                let Some(obj) = object_of(pass, id) else {
                    return true;
                };
                match artifacts.objects.get(obj) {
                    ObjectData::Const(_) => obj.pkg(&artifacts.objects).is_none(),
                    _ => true,
                }
            }
            Expr::ParenExpr(p) => rhs_allows_redundant_flag(pass, &p.x, tlhs),
            Expr::BasicLit(lit) => {
                // Fallback if lit somehow typed in isolation.
                let Some(kind) = lit.kind else {
                    return false;
                };
                match kind {
                    Token::INT => is_basic_kind(pass, tlhs, BasicKind::Int),
                    Token::FLOAT => is_basic_kind(pass, tlhs, BasicKind::Float64),
                    Token::IMAG => is_basic_kind(pass, tlhs, BasicKind::Complex128),
                    Token::CHAR => is_basic_kind(pass, tlhs, BasicKind::Int32),
                    Token::STRING => is_basic_kind(pass, tlhs, BasicKind::String),
                    _ => false,
                }
            }
            Expr::BinaryExpr(_) | Expr::UnaryExpr(_) => false,
            _ => true,
        }
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
            // `var x T = nil` still needs T — untyped nil cannot be omitted.
            if matches!(v, Expr::Ident(id) if id.name == "nil") {
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
