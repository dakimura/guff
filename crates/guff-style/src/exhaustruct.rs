//! Port of [`dev.gaijin.team/go/exhaustruct/v4`](https://github.com/GaijinEntertainment/go-exhaustruct)
//! (golangci-lint wrapper in `pkg/golinters/exhaustruct`).
//!
//! Checks that struct composite literals initialize all required fields.
//! Fields tagged `` `exhaustruct:"optional"` `` are skipped. Cross-package
//! literals only require exported fields.
//!
//! Empty literals are allowed in non-nil error returns (upstream default).
//!
//! DEFERRED: `//exhaustruct:ignore` / `//exhaustruct:enforce` comment
//! directives (use `//nolint:exhaustruct` instead).

use std::sync::OnceLock;

use guff::ast::{CompositeLit, Expr, ReturnStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, TypeData};
use guff_types::object::is_exported;
use guff_types::TypeId;
use regex::Regex;

use crate::options::ExhaustructOptions;

struct TypeInfo {
    name: String,
    package_name: String,
    package_path: String,
}

impl TypeInfo {
    fn full(&self) -> String {
        format!("{}.{}", self.package_path, self.name)
    }

    fn short(&self) -> String {
        format!("{}.{}", self.package_name, self.name)
    }
}

struct FieldInfo {
    name: String,
    exported: bool,
    optional: bool,
}

struct Patterns {
    include: Vec<Regex>,
    exclude: Vec<Regex>,
    allow_empty: Vec<Regex>,
}

fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|p| Regex::new(p).ok())
        .collect()
}

fn match_full(patterns: &[Regex], s: &str) -> bool {
    patterns.iter().any(|re| {
        re.find(s)
            .is_some_and(|m| m.start() == 0 && m.end() == s.len())
    })
}

fn has_optional_tag(tag: &str) -> bool {
    // Go reflect.StructTag.Get("exhaustruct") == "optional"
    for part in tag.split('\t').flat_map(|p| p.split(' ')) {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("exhaustruct:") {
            let val = rest.trim_matches('"');
            if val == "optional" {
                return true;
            }
        }
    }
    false
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn type_of_node_id(pass: &Pass<'_>, id: u32) -> Option<TypeId> {
    if id == 0 {
        return None;
    }
    let info = pass.types_info()?;
    Some(info.types.get(&id)?.typ)
}

fn universe_error(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn implements_error(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(err) = universe_error(pass) else {
        return false;
    };
    let mut types = artifacts.types.clone();
    api_implements(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        err,
    )
}

fn get_struct_type(pass: &Pass<'_>, lit: &CompositeLit) -> Option<(TypeId, TypeInfo)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = type_of_node_id(pass, lit.id)?;
    let typ = unalias_readonly(&artifacts.types, typ);

    match artifacts.types.get(typ) {
        TypeData::Named(n) => {
            let under = typ.underlying(&artifacts.types);
            if !matches!(artifacts.types.get(under), TypeData::Struct(_)) {
                return None;
            }
            let obj = n.obj();
            let name = obj.name(&artifacts.objects).to_string();
            let (package_name, package_path) = match obj.pkg(&artifacts.objects) {
                Some(pid) => {
                    let pkg = artifacts.packages.get(pid);
                    (pkg.name().to_string(), pkg.path().to_string())
                }
                None => (pass.pkg().name.to_string(), pass.pkg().pkg_path.clone()),
            };
            Some((
                under,
                TypeInfo {
                    name,
                    package_name,
                    package_path,
                },
            ))
        }
        TypeData::Struct(_) => Some((
            typ,
            TypeInfo {
                name: "<anonymous>".into(),
                package_name: pass.pkg().name.to_string(),
                package_path: pass.pkg().pkg_path.clone(),
            },
        )),
        _ => None,
    }
}

fn struct_fields(pass: &Pass<'_>, struct_ty: TypeId) -> Vec<FieldInfo> {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Vec::new();
    };
    let TypeData::Struct(s) = artifacts.types.get(struct_ty) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(s.num_fields());
    for i in 0..s.num_fields() {
        let field = s.field(i);
        let name = field.name(&artifacts.objects).to_string();
        let exported = is_exported(&name);
        let optional = has_optional_tag(s.tag(i));
        out.push(FieldInfo {
            name,
            exported,
            optional,
        });
    }
    out
}

fn is_named_literal(lit: &CompositeLit) -> bool {
    matches!(lit.elts.first(), Some(Expr::KeyValueExpr(_)))
}

fn skipped_fields(fields: &[FieldInfo], lit: &CompositeLit, only_exported: bool) -> Vec<String> {
    if lit.elts.is_empty() {
        return fields
            .iter()
            .filter(|f| !f.optional && (f.exported || !only_exported))
            .map(|f| f.name.clone())
            .collect();
    }

    let mut present = vec![false; fields.len()];
    if is_named_literal(lit) {
        for elt in &lit.elts {
            let Expr::KeyValueExpr(kv) = elt else {
                continue;
            };
            let Expr::Ident(id) = kv.key.as_ref() else {
                continue;
            };
            if let Some(i) = fields.iter().position(|f| f.name == id.name) {
                present[i] = true;
            }
        }
    } else {
        for i in 0..lit.elts.len().min(fields.len()) {
            present[i] = true;
        }
    }

    fields
        .iter()
        .enumerate()
        .filter(|(i, f)| {
            !present[*i] && !f.optional && (f.exported || !only_exported)
        })
        .map(|(_, f)| f.name.clone())
        .collect()
}

fn should_process(patterns: &Patterns, info: &TypeInfo) -> bool {
    let name = info.full();
    if !patterns.include.is_empty() && !match_full(&patterns.include, &name) {
        return false;
    }
    if !patterns.exclude.is_empty() && match_full(&patterns.exclude, &name) {
        return false;
    }
    true
}

/// Walk ancestors from the composite lit upward (stack top = lit).
fn empty_struct_allowed(
    pass: &Pass<'_>,
    stack: &[NodeRef<'_>],
    info: &TypeInfo,
    options: &ExhaustructOptions,
    patterns: &Patterns,
) -> bool {
    if options.allow_empty {
        return true;
    }
    if match_full(&patterns.allow_empty, &info.full()) {
        return true;
    }

    if let Some(ret) = parent_return(stack) {
        if options.allow_empty_returns {
            return true;
        }
        if is_error_return(pass, ret, stack.last().copied()) {
            return true;
        }
    }

    if options.allow_empty_declarations && is_child_of_var_decl(stack) {
        return true;
    }

    false
}

fn parent_return<'a>(stack: &[NodeRef<'a>]) -> Option<&'a ReturnStmt> {
    for i in (0..stack.len().saturating_sub(1)).rev() {
        match stack[i] {
            NodeRef::ReturnStmt(r) => return Some(r),
            NodeRef::UnaryExpr(u) if u.op == Token::AND => continue,
            _ => return None,
        }
    }
    None
}

fn is_child_of_var_decl(stack: &[NodeRef<'_>]) -> bool {
    for i in (0..stack.len().saturating_sub(1)).rev() {
        match stack[i] {
            NodeRef::AssignStmt(a) if a.tok == Some(Token::DEFINE) => return true,
            NodeRef::ValueSpec(_) => return true,
            NodeRef::UnaryExpr(u) if u.op == Token::AND => continue,
            _ => return false,
        }
    }
    false
}

fn same_expr_as_composite(expr: &Expr, lit_id: u32) -> bool {
    match expr {
        Expr::CompositeLit(c) => c.id == lit_id,
        Expr::UnaryExpr(u) if u.op == Token::AND => same_expr_as_composite(&u.x, lit_id),
        Expr::ParenExpr(p) => same_expr_as_composite(&p.x, lit_id),
        _ => false,
    }
}

fn is_error_return(pass: &Pass<'_>, ret: &ReturnStmt, current: Option<NodeRef<'_>>) -> bool {
    let lit_id = match current {
        Some(NodeRef::CompositeLit(c)) => c.id,
        _ => 0,
    };
    for ri in ret.results.iter().rev() {
        if lit_id != 0 && same_expr_as_composite(ri, lit_id) {
            continue;
        }
        if let Expr::Ident(id) = ri {
            if id.name == "nil" {
                continue;
            }
        }
        if let Expr::UnaryExpr(u) = ri {
            if u.op == Token::AND {
                if lit_id != 0 && same_expr_as_composite(&u.x, lit_id) {
                    continue;
                }
            }
        }
        let Some(typ) = type_of_expr(pass, ri) else {
            continue;
        };
        if implements_error(pass, typ) {
            return true;
        }
    }
    false
}

fn check_lit(
    pass: &Pass<'_>,
    lit: &CompositeLit,
    stack: &[NodeRef<'_>],
    options: &ExhaustructOptions,
    patterns: &Patterns,
    pending: &mut Vec<(u32, String)>,
) {
    let Some((struct_ty, info)) = get_struct_type(pass, lit) else {
        return;
    };

    if lit.elts.is_empty() && empty_struct_allowed(pass, stack, &info, options, patterns) {
        return;
    }

    if !should_process(patterns, &info) {
        return;
    }

    let only_exported = info.package_path != pass.pkg().pkg_path;
    let fields = struct_fields(pass, struct_ty);
    let missing = skipped_fields(&fields, lit, only_exported);
    if missing.is_empty() {
        return;
    }

    // `lv.pass.Reportf(*pos, …)` with `pos := lv.lit.Pos()`. A
    // `CompositeLit`'s `Pos()` is its `Type.Pos()` when it has one, and only
    // falls back to the brace for an elided type (`[]T{{…}}`).
    let pos = lit
        .ty
        .as_ref()
        .map(|t| t.pos().0 as u32)
        .unwrap_or(lit.lbrace.0 as u32);
    let msg = if missing.len() == 1 {
        format!("{} is missing field {}", info.short(), missing[0])
    } else {
        format!(
            "{} is missing fields {}",
            info.short(),
            missing.join(", ")
        )
    };
    pending.push((pos, msg));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "exhaustruct requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ExhaustructOptions>("exhaustruct")
        .cloned()
        .unwrap_or_default();

    let patterns = Patterns {
        include: compile_patterns(&options.include),
        exclude: compile_patterns(&options.exclude),
        allow_empty: compile_patterns(&options.allow_empty_rx),
    };

    let mut pending = Vec::new();
    for file in pass.files() {
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(node) => {
                    stack.push(node);
                    if let NodeRef::CompositeLit(lit) = node {
                        check_lit(pass, lit, &stack, &options, &patterns, &mut pending);
                    }
                }
                None => {
                    stack.pop();
                }
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
        name: "exhaustruct",
        doc: "Checks if all structure fields are initialized",
        url: "https://github.com/GaijinEntertainment/go-exhaustruct",
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
    fn optional_tag_parsing() {
        assert!(has_optional_tag(r#"exhaustruct:"optional""#));
        assert!(has_optional_tag(r#"json:"e" exhaustruct:"optional""#));
        assert!(!has_optional_tag(r#"exhaustruct:"required""#));
        assert!(!has_optional_tag(r#"json:"e""#));
    }

    #[test]
    fn pattern_full_match() {
        let re = compile_patterns(&[r".*\.Test$".into(), r"foo\.Bar".into()]);
        assert!(match_full(&re, "example.com/pkg.Test"));
        assert!(match_full(&re, "foo.Bar"));
        assert!(!match_full(&re, "example.com/pkg.Test2"));
        assert!(!match_full(&re, "xfoo.Bar"));
    }
}
