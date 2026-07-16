//! Port of [`github.com/nishanths/exhaustive`](https://github.com/nishanths/exhaustive)
//! (golangci-lint wrapper in `pkg/golinters/exhaustive`).
//!
//! Checks that switch statements on enum-like named types list all members.
//! An "enum" here is a named type whose underlying type is integer, float, or
//! string, with same-scope const members.
//!
//! Defaults match golangci / upstream: check switches only;
//! `default` does **not** satisfy exhaustiveness unless configured.
//!
//! DEFERRED: map-literal checks; `//exhaustive:ignore` / `//exhaustive:enforce`
//! comment directives; `check-generated`; type-parameter / union tags;
//! SuggestedFix.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec, Stmt, SwitchStmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Fact, FactTypeId, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::arena::{ObjectData, ObjectId, TypeData};
use guff_types::basic::{IS_FLOAT, IS_INTEGER, IS_STRING};
use guff_types::object::is_exported;
use guff_types::operand::OperandMode;
use guff_types::TypeId;
use regex::Regex;

use crate::options::ExhaustiveOptions;

/// Fact attached to an enum type's [`TypeName`] object listing its members.
#[derive(Clone, Debug, Default)]
struct EnumMembersFact {
    /// Member names in declaration order.
    names: Vec<String>,
    /// name → constant.ExactString()
    name_to_value: HashMap<String, String>,
}

impl Fact for EnumMembersFact {
    fn fact_type_id(&self) -> FactTypeId {
        FactTypeId::of::<Self>()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn clone_fact(&self) -> Box<dyn Fact> {
        Box::new(self.clone())
    }
}

#[derive(Clone)]
struct EnumTypeInfo {
    pkg_name: String,
    pkg_path: String,
    type_name_str: String,
    members: EnumMembersFact,
}

fn is_value_mode(mode: OperandMode) -> bool {
    matches!(
        mode,
        OperandMode::Constant
            | OperandMode::Variable
            | OperandMode::MapIndex
            | OperandMode::Value
            | OperandMode::NilValue
            | OperandMode::CommaOk
            | OperandMode::CommaErr
    )
}

fn valid_basic_underlying(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let under = typ.underlying(&artifacts.types);
    match artifacts.types.get(under) {
        TypeData::Basic(b) => {
            let info = b.info();
            info.contains(IS_INTEGER) || info.contains(IS_FLOAT) || info.contains(IS_STRING)
        }
        _ => false,
    }
}

fn named_type_name(pass: &Pass<'_>, typ: TypeId) -> Option<ObjectId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    match artifacts.types.get(typ) {
        TypeData::Named(n) => Some(n.obj()),
        _ => None,
    }
}

fn possible_enum_member(
    pass: &Pass<'_>,
    const_obj: ObjectId,
) -> Option<(ObjectId, String, String)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Const(c) = artifacts.objects.get(const_obj) else {
        return None;
    };
    let name = c.name().to_string();
    if name == "_" {
        return None;
    }
    let typ = c.typ();
    if !valid_basic_underlying(pass, typ) {
        return None;
    }
    let type_name = named_type_name(pass, typ)?;
    // Enum type and member must share the same declaring scope.
    if const_obj.parent(&artifacts.objects) != type_name.parent(&artifacts.objects) {
        return None;
    }
    let val = c.val().exact_string();
    Some((type_name, name, val))
}

fn find_enums(pass: &Pass<'_>, package_scope_only: bool) -> HashMap<ObjectId, EnumTypeInfo> {
    let mut by_type: HashMap<ObjectId, EnumMembersFact> = HashMap::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return HashMap::new();
    };
    let pkg_scope = pass
        .type_pkg()
        .map(|pid| artifacts.packages.get(pid).scope());

    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::CONST) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                for ident in &vs.names {
                    let Some(obj) = pass
                        .types_info()
                        .and_then(|info| info.defs.get(&ident.id).copied().flatten())
                    else {
                        continue;
                    };
                    let Some((type_name, member, val)) = possible_enum_member(pass, obj) else {
                        continue;
                    };
                    if package_scope_only {
                        let Some(ps) = pkg_scope else {
                            continue;
                        };
                        if type_name.parent(&artifacts.objects) != Some(ps) {
                            continue;
                        }
                    }
                    let entry = by_type.entry(type_name).or_default();
                    entry.names.push(member.clone());
                    entry.name_to_value.insert(member, val);
                }
            }
        }
    }

    let mut out = HashMap::new();
    for (type_name, members) in by_type {
        let ObjectData::TypeName(tn) = artifacts.objects.get(type_name) else {
            continue;
        };
        let type_name_str = tn.name().to_string();
        let (pkg_name, pkg_path) = match type_name.pkg(&artifacts.objects) {
            Some(pid) => {
                let p = artifacts.packages.get(pid);
                (p.name().to_string(), p.path().to_string())
            }
            None => (pass.pkg().name.to_string(), pass.pkg().pkg_path.clone()),
        };
        out.insert(
            type_name,
            EnumTypeInfo {
                pkg_name,
                pkg_path,
                type_name_str,
                members,
            },
        );
    }
    out
}

fn export_enum_facts(pass: &mut Pass<'_>, enums: &HashMap<ObjectId, EnumTypeInfo>) {
    for (type_name, info) in enums {
        pass.export_object_fact(*type_name, Box::new(info.members.clone()));
    }
}

fn import_enum_members(pass: &Pass<'_>, type_name: ObjectId) -> Option<EnumMembersFact> {
    let mut fact = EnumMembersFact::default();
    if pass.import_object_fact(type_name, &mut fact) {
        return Some(fact);
    }
    None
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<(TypeId, OperandMode)> {
    let info = pass.types_info()?;
    let tv = info.types.get(&expr.id())?;
    Some((tv.typ, tv.mode))
}

fn enum_for_tag(
    pass: &Pass<'_>,
    local: &HashMap<ObjectId, EnumTypeInfo>,
    tag_typ: TypeId,
) -> Option<EnumTypeInfo> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, tag_typ);
    let TypeData::Named(n) = artifacts.types.get(typ) else {
        return None;
    };
    let type_name = n.obj();
    if let Some(info) = local.get(&type_name) {
        return Some(info.clone());
    }
    let members = import_enum_members(pass, type_name)?;
    let ObjectData::TypeName(tn) = artifacts.objects.get(type_name) else {
        return None;
    };
    let type_name_str = tn.name().to_string();
    let (pkg_name, pkg_path) = match type_name.pkg(&artifacts.objects) {
        Some(pid) => {
            let p = artifacts.packages.get(pid);
            (p.name().to_string(), p.path().to_string())
        }
        None => (pass.pkg().name.to_string(), pass.pkg().pkg_path.clone()),
    };
    Some(EnumTypeInfo {
        pkg_name,
        pkg_path,
        type_name_str,
        members,
    })
}

fn strip_conversions<'a>(pass: &Pass<'_>, expr: &'a Expr) -> &'a Expr {
    match expr {
        Expr::ParenExpr(p) => strip_conversions(pass, &p.x),
        Expr::CallExpr(c) if c.args.len() == 1 => {
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return expr;
            };
            let Some(tv) = pass.types_info().and_then(|i| i.types.get(&c.fun.id())) else {
                return expr;
            };
            let under = tv.typ.underlying(&artifacts.types);
            if matches!(artifacts.types.get(under), TypeData::Signature(_)) {
                return expr;
            }
            strip_conversions(pass, &c.args[0])
        }
        _ => expr,
    }
}

fn expr_const_val(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let expr = strip_conversions(pass, expr);
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let info = pass.types_info()?;

    let handle_ident = |ident: &guff::ast::Ident| -> Option<String> {
        let obj = info.uses.get(&ident.id).copied()?;
        let ObjectData::Const(c) = artifacts.objects.get(obj) else {
            return None;
        };
        Some(c.val().exact_string())
    };

    match expr {
        Expr::Ident(id) => handle_ident(id),
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(x) = sel.x.as_ref() else {
                return None;
            };
            let obj = info.uses.get(&x.id).copied()?;
            if !matches!(artifacts.objects.get(obj), ObjectData::PkgName(_)) {
                return None;
            }
            handle_ident(&sel.sel)
        }
        _ => None,
    }
}

fn analyze_clauses(pass: &Pass<'_>, sw: &SwitchStmt) -> (HashSet<String>, bool) {
    let mut found = HashSet::new();
    let mut has_default = false;
    for stmt in &sw.body.list {
        let Stmt::CaseClause(cc) = stmt else {
            continue;
        };
        if cc.list.is_empty() {
            has_default = true;
            continue;
        }
        for expr in &cc.list {
            if let Some(val) = expr_const_val(pass, expr) {
                found.insert(val);
            }
        }
    }
    (found, has_default)
}

fn member_ignored(re: &Option<Regex>, pkg_path: &str, name: &str) -> bool {
    let Some(re) = re else {
        return false;
    };
    re.is_match(&format!("{pkg_path}.{name}"))
}

fn type_ignored(re: &Option<Regex>, pkg_path: &str, type_name: &str) -> bool {
    let Some(re) = re else {
        return false;
    };
    re.is_match(&format!("{pkg_path}.{type_name}"))
}

fn compile_re(pat: &str) -> Option<Regex> {
    if pat.is_empty() {
        return None;
    }
    Regex::new(pat).ok()
}

fn check_switch(
    pass: &Pass<'_>,
    sw: &SwitchStmt,
    local: &HashMap<ObjectId, EnumTypeInfo>,
    options: &ExhaustiveOptions,
    ignore_members: &Option<Regex>,
    ignore_types: &Option<Regex>,
    pending: &mut Vec<(u32, String)>,
) {
    if !options.check_switch {
        return;
    }
    let Some(tag) = sw.tag.as_ref() else {
        return;
    };
    let Some((tag_typ, mode)) = type_of_expr(pass, tag) else {
        return;
    };
    if !is_value_mode(mode) {
        return;
    }
    let Some(enum_info) = enum_for_tag(pass, local, tag_typ) else {
        return;
    };
    if type_ignored(
        ignore_types,
        &enum_info.pkg_path,
        &enum_info.type_name_str,
    ) {
        return;
    }

    let same_pkg = enum_info.pkg_path == pass.pkg().pkg_path;
    let (found_vals, has_default) = analyze_clauses(pass, sw);

    // Required members: exported always; unexported only when same package.
    let mut missing: Vec<String> = Vec::new();
    // Track values already satisfied so same-valued aliases don't all report.
    let mut remaining_by_val: HashMap<String, Vec<String>> = HashMap::new();
    for name in &enum_info.members.names {
        if name == "_" {
            continue;
        }
        if !is_exported(name) && !same_pkg {
            continue;
        }
        if member_ignored(ignore_members, &enum_info.pkg_path, name) {
            continue;
        }
        let Some(val) = enum_info.members.name_to_value.get(name) else {
            continue;
        };
        remaining_by_val
            .entry(val.clone())
            .or_default()
            .push(name.clone());
    }
    for val in &found_vals {
        remaining_by_val.remove(val);
    }
    for names in remaining_by_val.values() {
        // Report one representative per constant value (first declared).
        if let Some(n) = names.first() {
            missing.push(n.clone());
        }
    }
    missing.sort();

    let type_label = format!("{}.{}", enum_info.pkg_name, enum_info.type_name_str);
    let pos = sw.switch.0 as u32;

    if options.default_case_required && !has_default {
        pending.push((
            pos,
            format!("missing default case in switch of type {type_label}"),
        ));
        return;
    }

    if missing.is_empty() {
        return;
    }
    if has_default && options.default_signifies_exhaustive {
        return;
    }

    let missing_labels: Vec<String> = missing
        .iter()
        .map(|n| format!("{}.{}", enum_info.pkg_name, n))
        .collect();
    pending.push((
        pos,
        format!(
            "missing cases in switch of type {type_label}: {}",
            missing_labels.join(", ")
        ),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "exhaustive requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ExhaustiveOptions>("exhaustive")
        .cloned()
        .unwrap_or_default();

    let ignore_members = compile_re(&options.ignore_enum_members);
    let ignore_types = compile_re(&options.ignore_enum_types);

    let enums = find_enums(pass, options.package_scope_only);
    export_enum_facts(pass, &enums);

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(NodeRef::SwitchStmt(sw)) = n {
                check_switch(
                    pass,
                    sw,
                    &enums,
                    &options,
                    &ignore_members,
                    &ignore_types,
                    &mut pending,
                );
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
        name: "exhaustive",
        doc: "Check exhaustiveness of enum switch statements",
        url: "https://github.com/nishanths/exhaustive",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![FactTypeId::of::<EnumMembersFact>()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_mode_predicates() {
        assert!(is_value_mode(OperandMode::Variable));
        assert!(is_value_mode(OperandMode::Constant));
        assert!(!is_value_mode(OperandMode::TypeExpr));
        assert!(!is_value_mode(OperandMode::Builtin));
        assert!(!is_value_mode(OperandMode::NoValue));
    }
}
