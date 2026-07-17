//! Port of [`github.com/alecthomas/go-check-sumtype`](https://github.com/alecthomas/go-check-sumtype)
//! (golangci-lint wrapper in `pkg/golinters/gochecksumtype`).
//!
//! Exhaustiveness checks for interfaces marked `//sumtype:decl`. Variants are
//! same-package types that implement the sealed interface (at least one
//! unexported method).
//!
//! Defaults match golangci: `default-signifies-exhaustive: true`,
//! `include-shared-interfaces: false`.
//!
//! Re-parses with `PARSE_COMMENTS` because load uses `Mode::NONE` (declaration
//! docs are otherwise dropped). Raw comment text is inspected — `CommentGroup::text`
//! strips `sumtype:decl` as a Go directive.

use std::fs;
use std::sync::{Arc, OnceLock};

use guff::ast::{Decl, Expr, File, Spec, Stmt, TypeSwitchStmt};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectData, ObjectId, TypeData, TypeId};
use guff_types::interface::{interface_method, interface_num_methods};
use guff_types::object::is_exported;
use guff_types::pointer::{new_pointer, pointer_elem};
use guff_types::predicates::is_interface;

use crate::options::GochecksumtypeOptions;

#[derive(Clone)]
struct SumTypeDecl {
    type_name: String,
    /// TypeSpec name position (for decl errors / "from" in messages).
    pos: Pos,
    /// `file:line:col` for the TypeSpec (upstream `Decl.Pos.String()`).
    location: String,
}

#[derive(Clone)]
struct SumTypeDef {
    decl: SumTypeDecl,
    /// Underlying interface type id.
    iface: TypeId,
    /// Variant TypeName objects (same package).
    variants: Vec<ObjectId>,
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

fn pos_location(fset: &FileSet, pos: Pos) -> String {
    let p = fset.position(pos);
    format!("{}:{}:{}", p.filename, p.line, p.column)
}

fn find_sumtype_decls(pass: &Pass<'_>) -> Vec<SumTypeDecl> {
    let mut decls = Vec::new();
    for (i, _file) in pass.files().iter().enumerate() {
        let path = pass
            .pkg()
            .compiled_go_files
            .get(i)
            .cloned()
            .or_else(|| pass.pkg().go_files.get(i).cloned());
        let Some(path) = path else {
            continue;
        };
        let Some((re_fset, parsed)) = reparse(&path) else {
            continue;
        };
        for decl in &parsed.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            let Some(doc) = gen.doc.as_ref() else {
                continue;
            };
            let has_sumtype = doc
                .list
                .iter()
                .any(|c| c.text.starts_with("//sumtype:decl"));
            if !has_sumtype {
                continue;
            }
            // Upstream takes the last TypeSpec in the GenDecl.
            let mut tspec_name: Option<(String, Pos)> = None;
            for spec in &gen.specs {
                let Spec::TypeSpec(ts) = spec else {
                    continue;
                };
                tspec_name = Some((ts.name.name.clone(), ts.name.pos()));
            }
            let Some((type_name, pos)) = tspec_name else {
                // No TypeSpec — skip (upstream returns notFoundError at GenDecl).
                continue;
            };
            decls.push(SumTypeDecl {
                type_name,
                pos,
                location: pos_location(&re_fset, pos),
            });
        }
    }
    decls
}

fn underlying_iface(pass: &Pass<'_>, typ: TypeId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let under = typ.underlying(&artifacts.types);
    if matches!(artifacts.types.get(under), TypeData::Interface(_)) {
        Some(under)
    } else {
        None
    }
}

fn has_unexported_method(pass: &Pass<'_>, iface: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    let n = interface_num_methods(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        iface,
    );
    for i in 0..n {
        let mid = interface_method(
            &mut types,
            &artifacts.objects,
            &artifacts.packages,
            iface,
            i,
        );
        let name = match artifacts.objects.get(mid) {
            ObjectData::Func(f) => f.name(),
            _ => continue,
        };
        if !is_exported(name) {
            return true;
        }
    }
    false
}

fn named_type_params(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(n) => n
            .type_params()
            .map(|p| !p.list().is_empty())
            .unwrap_or(false),
        _ => false,
    }
}

fn types_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn type_implements(pass: &Pass<'_>, v: TypeId, iface: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    if api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        v,
        iface,
    ) {
        return true;
    }
    let ptr = new_pointer(&mut types, v);
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        ptr,
        iface,
    )
}

fn indirect(pass: &Pass<'_>, ty: TypeId) -> TypeId {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return ty;
    };
    let mut cur = ty;
    loop {
        match artifacts.types.get(cur) {
            TypeData::Pointer(_) => {
                cur = pointer_elem(&artifacts.types, cur);
            }
            _ => return cur,
        }
    }
}

fn is_iface_type(pass: &Pass<'_>, ty: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    is_interface(&artifacts.types, indirect(pass, ty))
}

fn build_def(
    pass: &Pass<'_>,
    decl: SumTypeDecl,
    pending: &mut Vec<(u32, String)>,
) -> Option<SumTypeDef> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pkg_scope = pass.type_pkg().map(|pid| artifacts.packages.get(pid).scope())?;
    let obj = artifacts.scopes.get(pkg_scope).lookup_local(&decl.type_name)?;
    let ObjectData::TypeName(tn) = artifacts.objects.get(obj) else {
        pending.push((
            decl.pos.0 as u32,
            format!("type '{}' is not defined", decl.type_name),
        ));
        return None;
    };
    let Some(typ) = tn.typ() else {
        pending.push((
            decl.pos.0 as u32,
            format!("type '{}' is not defined", decl.type_name),
        ));
        return None;
    };
    let Some(iface) = underlying_iface(pass, typ) else {
        pending.push((
            decl.pos.0 as u32,
            format!("type '{}' is not an interface", decl.type_name),
        ));
        return None;
    };
    if !has_unexported_method(pass, iface) {
        pending.push((
            decl.pos.0 as u32,
            format!(
                "interface '{}' is not sealed (sealing requires at least one unexported method)",
                decl.type_name
            ),
        ));
        return None;
    }

    let mut variants = Vec::new();
    for name in artifacts.scopes.get(pkg_scope).names() {
        let Some(cand) = artifacts.scopes.get(pkg_scope).lookup_local(&name) else {
            continue;
        };
        let ObjectData::TypeName(ctn) = artifacts.objects.get(cand) else {
            continue;
        };
        let Some(cty) = ctn.typ() else {
            continue;
        };
        // Skip the sum type itself.
        if let Some(c_iface) = underlying_iface(pass, cty) {
            if types_identical(pass, c_iface, iface) {
                continue;
            }
        }
        if named_type_params(pass, cty) {
            continue;
        }
        if type_implements(pass, cty, iface) {
            variants.push(cand);
        }
    }

    Some(SumTypeDef {
        decl,
        iface,
        variants,
    })
}

fn find_def<'a>(defs: &'a [SumTypeDef], pass: &Pass<'_>, needle: TypeId) -> Option<&'a SumTypeDef> {
    let under = {
        let artifacts = pass.pkg().type_artifacts.as_ref()?;
        needle.underlying(&artifacts.types)
    };
    defs.iter()
        .find(|d| types_identical(pass, under, d.iface))
}

fn find_type_assert_expr(swtch: &TypeSwitchStmt) -> Option<&Expr> {
    match swtch.assign.as_ref() {
        Stmt::AssignStmt(asgn) if !asgn.rhs.is_empty() => match &asgn.rhs[0] {
            Expr::TypeAssertExpr(ta) => Some(ta.x.as_ref()),
            _ => None,
        },
        Stmt::ExprStmt(es) => match &es.x {
            Expr::TypeAssertExpr(ta) => Some(ta.x.as_ref()),
            _ => None,
        },
        _ => None,
    }
}

fn switch_variants<'a>(swtch: &'a TypeSwitchStmt) -> (Vec<&'a Expr>, bool) {
    let mut exprs = Vec::new();
    let mut has_default = false;
    for stmt in &swtch.body.list {
        let Stmt::CaseClause(clause) = stmt else {
            continue;
        };
        if clause.list.is_empty() {
            has_default = true;
        } else {
            exprs.extend(clause.list.iter());
        }
    }
    (exprs, has_default)
}

fn default_clause_always_panics(swtch: &TypeSwitchStmt) -> bool {
    let mut clause = None;
    for stmt in &swtch.body.list {
        let Stmt::CaseClause(c) = stmt else {
            continue;
        };
        if c.list.is_empty() {
            clause = Some(c);
            break;
        }
    }
    let Some(clause) = clause else {
        return false;
    };
    if clause.body.len() != 1 {
        return false;
    }
    let Stmt::ExprStmt(es) = &clause.body[0] else {
        return false;
    };
    let Expr::CallExpr(call) = &es.x else {
        return false;
    };
    match call.fun.as_ref() {
        Expr::Ident(id) => id.name == "panic",
        _ => false,
    }
}

fn type_of(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()
        .and_then(|info| info.types.get(&expr.id()))
        .map(|tv| tv.typ)
}

fn variant_type(pass: &Pass<'_>, obj: ObjectId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.objects.get(obj) {
        ObjectData::TypeName(tn) => tn.typ(),
        _ => None,
    }
}

fn variant_name(pass: &Pass<'_>, obj: ObjectId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    match artifacts.objects.get(obj) {
        ObjectData::TypeName(tn) => tn.name().to_string(),
        _ => String::new(),
    }
}

fn implements_shared(pass: &Pass<'_>, varty: TypeId, case_ty: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let under = case_ty.underlying(&artifacts.types);
    if !matches!(artifacts.types.get(under), TypeData::Interface(_)) {
        return false;
    }
    type_implements(pass, varty, under)
}

fn missing_variants(
    pass: &Pass<'_>,
    def: &SumTypeDef,
    case_tys: &[TypeId],
    include_shared: bool,
) -> Vec<ObjectId> {
    let mut missing = Vec::new();
    for &v in &def.variants {
        let Some(varty) = variant_type(pass, v) else {
            continue;
        };
        let varty = indirect(pass, varty);
        let mut found = false;
        for &ty in case_tys {
            let ty = indirect(pass, ty);
            if types_identical(pass, varty, ty) {
                found = true;
                break;
            }
            if include_shared && implements_shared(pass, varty, ty) {
                found = true;
                break;
            }
        }
        if !found && !is_iface_type(pass, varty) {
            missing.push(v);
        }
    }
    missing
}

fn check_switch(
    pass: &Pass<'_>,
    defs: &[SumTypeDef],
    swtch: &TypeSwitchStmt,
    opts: &GochecksumtypeOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(asserted) = find_type_assert_expr(swtch) else {
        return;
    };
    let Some(ty) = type_of(pass, asserted) else {
        return;
    };
    let Some(def) = find_def(defs, pass, ty) else {
        return;
    };
    let (variant_exprs, has_default) = switch_variants(swtch);
    if opts.default_signifies_exhaustive
        && has_default
        && !default_clause_always_panics(swtch)
    {
        return;
    }
    let mut case_tys = Vec::new();
    for expr in variant_exprs {
        if let Some(t) = type_of(pass, expr) {
            case_tys.push(t);
        }
    }
    let missing = missing_variants(pass, def, &case_tys, opts.include_shared_interfaces);
    if missing.is_empty() {
        return;
    }
    let mut names: Vec<String> = missing.iter().map(|&o| variant_name(pass, o)).collect();
    names.sort();
    pending.push((
        swtch.switch.0 as u32,
        format!(
            "exhaustiveness check failed for sum type \"{}\" (from {}): missing cases for {}",
            def.decl.type_name,
            def.decl.location,
            names.join(", ")
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gochecksumtype requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GochecksumtypeOptions>("gochecksumtype")
        .cloned()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    let decls = find_sumtype_decls(pass);
    let mut defs = Vec::new();
    for decl in decls {
        if let Some(def) = build_def(pass, decl, &mut pending) {
            defs.push(def);
        }
    }

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::TypeSwitchStmt(swtch) = n {
                check_switch(pass, &defs, swtch, &options, &mut pending);
            }
            true
        });
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "gochecksumtype",
        doc: "Run exhaustiveness checks on Go \"sum types\"",
        url: "https://github.com/alecthomas/go-check-sumtype",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_options_match_golangci() {
        let o = GochecksumtypeOptions::default();
        assert!(o.default_signifies_exhaustive);
        assert!(!o.include_shared_interfaces);
    }

    #[test]
    fn sumtype_decl_prefix() {
        assert!("//sumtype:decl".starts_with("//sumtype:decl"));
        assert!("//sumtype:decl extra".starts_with("//sumtype:decl"));
        assert!(!"// sumtype:decl".starts_with("//sumtype:decl"));
    }
}
