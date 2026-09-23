//! Port of [`github.com/nishanths/exhaustive`](https://github.com/nishanths/exhaustive)
//! (golangci-lint wrapper in `pkg/golinters/exhaustive`).
//!
//! Checks that switch statements and (when enabled) map literals on enum-like
//! named types list all members.
//! An "enum" here is a named type whose underlying type is integer, float, or
//! string, with same-scope const members.
//!
//! Defaults match golangci / upstream: check switches only;
//! `default` does **not** satisfy exhaustiveness unless configured.
//!
//! DEFERRED: composing type-parameter / union keys; SuggestedFix.
//!
//! `check-generated` needs nothing: golangci-lint pins the flag to `true`
//! regardless of the user's config (generated files are meant to be handled by
//! `linters.exclusions.generated` instead), which is what checking every file
//! amounts to.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{CommentGroup, CompositeLit, Decl, Expr, Spec, Stmt, SwitchStmt};
use guff::commentmap::{new_comment_map, CommentMap};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::comments::file_comments as comments_with_positions;
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

    fn type_name(&self) -> &'static str {
        "EnumMembersFact"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::json!({
            "names": self.names,
            "name_to_value": self.name_to_value,
        })
    }
}

fn decode_enum_members_fact(payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    let names = payload
        .get("names")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let mut name_to_value = HashMap::new();
    for (k, v) in payload.get("name_to_value")?.as_object()? {
        name_to_value.insert(k.clone(), v.as_str()?.to_string());
    }
    Some(Box::new(EnumMembersFact {
        names,
        name_to_value,
    }))
}

fn ensure_enum_members_decoder() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        guff_analysis::register_fact_decoder("EnumMembersFact", decode_enum_members_fact);
    });
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

/// Enum members read straight out of the declaring package's scope, for a type
/// whose package this run never analysed.
///
/// Upstream learns a *foreign* enum's members from an object fact, which exists
/// because golangci-lint runs the analyzer over dependency syntax too. guff
/// analyses the packages it was asked about and imports the rest, so the fact
/// is simply absent — and a `switch` over an imported enum then saw no members
/// and was never non-exhaustive. Every member upstream would count is in that
/// package's scope regardless of how the package arrived: `checklist.add`
/// passes `includeUnexported = pass.Pkg == e.typ.Pkg()`, so for a foreign enum
/// only the exported constants matter, and those are exactly what export data
/// carries.
///
/// The fact's own order is AST order, which the scope does not keep — and
/// which the message depends on, since `groupify` sorts the missing members by
/// declaration position. See the sort key below for what stands in for it.
fn enum_members_from_scope(pass: &Pass<'_>, type_name: ObjectId) -> Option<EnumMembersFact> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    // A type in the package under analysis is covered by `find_enums`; getting
    // here for one means it has no members, not that they are elsewhere.
    if Some(type_name.pkg(&artifacts.objects)?) == pass.type_pkg() {
        return None;
    }
    let scope = artifacts
        .packages
        .get(type_name.pkg(&artifacts.objects)?)
        .scope();
    let mut found: Vec<((u32, u32), String, String)> = Vec::new();
    for name in artifacts.scopes.get(scope).names() {
        let Some(obj) = artifacts.scopes.get(scope).lookup_local(&name) else {
            continue;
        };
        let Some((owner, member, val)) = possible_enum_member(pass, obj) else {
            continue;
        };
        if owner != type_name {
            continue;
        }
        // Declaration order, whichever way the package arrived. A dependency
        // type-checked from source keeps Go's `object.order_` (1-based, in
        // declaration order) but has its positions **erased**: they are
        // FileSet-absolute and the overlay is reused across runs whose bases
        // differ. One decoded from export data is the other way round — no
        // order, but a synthesized position. Zero is the absent value on both
        // sides, so the pair sorts correctly either way.
        found.push((
            (obj.order(&artifacts.objects), obj.pos(&artifacts.objects)),
            member,
            val,
        ));
    }
    if found.is_empty() {
        return None;
    }
    found.sort();
    let mut fact = EnumMembersFact::default();
    for (_, name, val) in found {
        fact.names.push(name.clone());
        fact.name_to_value.insert(name, val);
    }
    Some(fact)
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
    // **No unaliasing.** Upstream's `fromType` switches on the type as
    // recorded, and an alias is a `*types.Alias`, not a `*types.Named` — it
    // matches no case and is not an enum. Go materializes aliases by default
    // since go1.23, so `switch k` on a `type KindAlias = Kind` parameter is
    // silent upstream while guff reported the whole of `Kind`'s membership.
    let TypeData::Named(n) = artifacts.types.get(tag_typ) else {
        return None;
    };
    let type_name = n.obj();
    if let Some(info) = local.get(&type_name) {
        return Some(info.clone());
    }
    let members = match import_enum_members(pass, type_name) {
        Some(m) => m,
        None => enum_members_from_scope(pass, type_name)?,
    };
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

fn missing_members(
    pass: &Pass<'_>,
    enum_info: &EnumTypeInfo,
    found_vals: &HashSet<String>,
    ignore_members: &Option<Regex>,
) -> Vec<String> {
    let same_pkg = enum_info.pkg_path == pass.pkg().pkg_path;
    let mut remaining_by_val: HashMap<String, Vec<String>> = HashMap::new();
    for name in &enum_info.members.names {
        if name == "_" || (!is_exported(name) && !same_pkg) {
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
    for val in found_vals {
        remaining_by_val.remove(val);
    }

    // Upstream groups the remaining members by constant value and sorts both the
    // members inside a group and the groups themselves by **AST position**
    // (`astBefore`), not by name. `names` is already in declaration order, so
    // the group order follows from the first appearance of each value there.
    // Sorting alphabetically instead reads `p.Large, p.Medium` where upstream
    // says `p.Medium, p.Large` — same set, different sentence.
    let mut missing = Vec::new();
    let mut emitted: HashSet<&String> = HashSet::new();
    for name in &enum_info.members.names {
        let Some(val) = enum_info.members.name_to_value.get(name) else {
            continue;
        };
        let Some(names) = remaining_by_val.get(val) else {
            continue;
        };
        // Report one representative per constant value (first declared).
        let Some(first) = names.first() else { continue };
        if emitted.insert(first) {
            missing.push(first.clone());
        }
    }
    missing
}

/// An enum switch that reached the reporting logic, with everything the
/// comment directives can still change folded out.
///
/// Upstream tests the directives *before* resolving the tag type; the split
/// here runs the type resolution first so that a file only pays for the
/// comment map when it actually holds an enum switch or an enum-keyed map
/// literal. Neither order can change what is reported.
struct SwitchFinding {
    pos: u32,
    type_label: String,
    missing_labels: Vec<String>,
    has_default: bool,
}

fn switch_finding(
    pass: &Pass<'_>,
    sw: &SwitchStmt,
    local: &HashMap<ObjectId, EnumTypeInfo>,
    ignore_members: &Option<Regex>,
    ignore_types: &Option<Regex>,
) -> Option<SwitchFinding> {
    let tag = sw.tag.as_ref()?;
    let (tag_typ, mode) = type_of_expr(pass, tag)?;
    if !is_value_mode(mode) {
        return None;
    }
    let enum_info = enum_for_tag(pass, local, tag_typ)?;
    if type_ignored(ignore_types, &enum_info.pkg_path, &enum_info.type_name_str) {
        return None;
    }

    let (found_vals, has_default) = analyze_clauses(pass, sw);
    let missing = missing_members(pass, &enum_info, &found_vals, ignore_members);

    Some(SwitchFinding {
        pos: sw.switch.0 as u32,
        type_label: format!("{}.{}", enum_info.pkg_name, enum_info.type_name_str),
        missing_labels: missing
            .iter()
            .map(|n| format!("{}.{}", enum_info.pkg_name, n))
            .collect(),
        has_default,
    })
}

fn report_switch(
    f: &SwitchFinding,
    options: &ExhaustiveOptions,
    require_default_case: bool,
    pending: &mut Vec<(u32, String)>,
) {
    let type_label = &f.type_label;
    if require_default_case && !f.has_default {
        pending.push((
            f.pos,
            format!("missing default case in switch of type {type_label}"),
        ));
        return;
    }

    if f.missing_labels.is_empty() {
        return;
    }
    if f.has_default && options.default_signifies_exhaustive {
        return;
    }

    pending.push((
        f.pos,
        format!(
            "missing cases in switch of type {type_label}: {}",
            f.missing_labels.join(", ")
        ),
    ));
}

fn map_finding(
    pass: &Pass<'_>,
    lit: &CompositeLit,
    local: &HashMap<ObjectId, EnumTypeInfo>,
    ignore_members: &Option<Regex>,
    ignore_types: &Option<Regex>,
) -> Option<(u32, String)> {
    // Upstream asks `pass.TypesInfo.Types[lit.Type]` for the literal's type,
    // and a literal whose type is elided — the inner `{…}` of
    // `map[Color]map[Color]int{Red: {…}}`, or of `[]holder{{m: …}}` — has no
    // `Type` node at all. `Types[nil]` is the zero `TypeAndValue`, both type
    // assertions fail, and the checker exits with `resultNotMapLiteral`. So an
    // elided map literal is never checked, however many members it is missing.
    // Measured against golangci-lint 2.12.2: of nine such literals it reports
    // only the four that spell their type out.
    if lit.ty.is_none() {
        return None;
    }
    // Upstream intentionally ignores empty map literals: they are commonly
    // used as mutable sets or initialized before being populated.
    if lit.elts.is_empty() {
        return None;
    }
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tv = pass.types_info().and_then(|info| info.types.get(&lit.id))?;
    let typ = unalias_readonly(&artifacts.types, tv.typ);
    let under = typ.underlying(&artifacts.types);
    let TypeData::Map(map) = artifacts.types.get(under) else {
        return None;
    };
    let enum_info = enum_for_tag(pass, local, map.key())?;
    if type_ignored(ignore_types, &enum_info.pkg_path, &enum_info.type_name_str) {
        return None;
    }

    let mut found_vals = HashSet::new();
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        if let Some(val) = expr_const_val(pass, &kv.key) {
            found_vals.insert(val);
        }
    }
    let missing = missing_members(pass, &enum_info, &found_vals, ignore_members);
    if missing.is_empty() {
        return None;
    }

    let type_label = format!("{}.{}", enum_info.pkg_name, enum_info.type_name_str);
    let missing_labels: Vec<String> = missing
        .iter()
        .map(|name| format!("{}.{}", enum_info.pkg_name, name))
        .collect();
    Some((
        lit.ty
            .as_ref()
            .map_or(lit.lbrace.0 as u32, |ty| ty.pos().0 as u32),
        format!(
            "missing keys in map of key type {type_label}: {}",
            missing_labels.join(", ")
        ),
    ))
}

// ============================================================
// `//exhaustive:` comment directives
// ============================================================

const IGNORE_COMMENT: &str = "//exhaustive:ignore";
const ENFORCE_COMMENT: &str = "//exhaustive:enforce";
const IGNORE_DEFAULT_CASE_REQUIRED_COMMENT: &str = "//exhaustive:ignore-default-case-required";
const ENFORCE_DEFAULT_CASE_REQUIRED_COMMENT: &str = "//exhaustive:enforce-default-case-required";

/// Upstream `userDirectives` (switch only): each comment maps to **one**
/// directive, longest text first, so `//exhaustive:enforce-default-case-required`
/// is not also an `//exhaustive:enforce`.
fn user_directives(groups: &[CommentGroup]) -> Vec<&'static str> {
    let mut out = Vec::new();
    for g in groups {
        for c in &g.list {
            for d in [
                ENFORCE_DEFAULT_CASE_REQUIRED_COMMENT,
                IGNORE_DEFAULT_CASE_REQUIRED_COMMENT,
                ENFORCE_COMMENT,
                IGNORE_COMMENT,
            ] {
                if c.text.starts_with(d) {
                    out.push(d);
                    break;
                }
            }
        }
    }
    out
}

/// Upstream `hasCommentPrefix` (map only): a plain prefix test, with no
/// longest-first disambiguation. `//exhaustive:ignore-default-case-required`
/// therefore *does* ignore a map literal, and `//exhaustive:ignoreme` does too
/// — both measured against golangci-lint 2.12.2.
fn has_comment_prefix(groups: &[&CommentGroup], prefix: &str) -> bool {
    groups
        .iter()
        .any(|g| g.list.iter().any(|c| c.text.starts_with(prefix)))
}

/// Node kinds whose comments upstream's map checker folds into the literal's
/// "related comments" — `ast` does not associate a comment group with a
/// `*ast.CompositeLit` itself, so the enclosing declaration or statement
/// carries it.
fn map_related_node(n: NodeRef<'_>) -> bool {
    matches!(
        n,
        NodeRef::CompositeLit(_)
            | NodeRef::ReturnStmt(_)
            | NodeRef::IndexExpr(_)
            | NodeRef::CallExpr(_)
            | NodeRef::UnaryExpr(_)
            | NodeRef::AssignStmt(_)
            | NodeRef::DeclStmt(_)
            | NodeRef::GenDecl(_)
            | NodeRef::ValueSpec(_)
    )
}

/// Comments upstream associates with a map literal, walking the ancestor
/// stack from the literal outwards.
///
/// Upstream's loop reads `default: break`, and its comment says it stops at
/// the first node that is not in the list above — but a `break` inside a
/// `switch` leaves the `switch`, not the `for`, so the walk in fact runs to
/// the top of the stack. Measured: an `//exhaustive:enforce` on a `var`
/// declaration reaches a literal nested inside an immediately-invoked func
/// literal, across the `FuncLit`/`BlockStmt` that are not in the list.
fn map_related_comments<'a>(cm: &'a CommentMap<'a>, stack: &[NodeRef<'a>]) -> Vec<&'a CommentGroup> {
    let mut out = Vec::new();
    for node in stack.iter().rev() {
        if !map_related_node(*node) {
            continue;
        }
        if let Some(groups) = cm.get(*node) {
            out.extend(groups.iter());
        }
    }
    out
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
        // Ancestor stack, maintained the way `inspector.WithStack` does:
        // `walk::inspect` calls back with `None` on the way out of a node.
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        let mut switches: Vec<(&SwitchStmt, SwitchFinding)> = Vec::new();
        let mut maps: Vec<(Vec<NodeRef<'_>>, u32, String)> = Vec::new();
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(node) => {
                    stack.push(node);
                    match node {
                        NodeRef::SwitchStmt(sw) if options.check_switch => {
                            if let Some(f) =
                                switch_finding(pass, sw, &enums, &ignore_members, &ignore_types)
                            {
                                switches.push((sw, f));
                            }
                        }
                        NodeRef::CompositeLit(lit) if options.check_map => {
                            if let Some((pos, msg)) =
                                map_finding(pass, lit, &enums, &ignore_members, &ignore_types)
                            {
                                maps.push((stack.clone(), pos, msg));
                            }
                        }
                        _ => {}
                    }
                }
                None => {
                    stack.pop();
                }
            }
            true
        });
        if switches.is_empty() && maps.is_empty() {
            continue;
        }

        // Only files that hold an enum switch or an enum-keyed map literal pay
        // for the comment map: the analysis AST carries no comments, so it
        // costs a reparse (`s1008` pays the same toll for the same reason).
        let reparsed = comments_with_positions(pass, file);
        let cmap = new_comment_map(pass.fset(), NodeRef::File(file), &reparsed);

        for (sw, f) in &switches {
            let groups = cmap.get(NodeRef::SwitchStmt(sw)).unwrap_or(&[]);
            let directives = user_directives(groups);
            if !options.explicit_exhaustive_switch && directives.contains(&IGNORE_COMMENT) {
                continue;
            }
            if options.explicit_exhaustive_switch && !directives.contains(&ENFORCE_COMMENT) {
                continue;
            }
            let mut require_default_case = options.default_case_required;
            if directives.contains(&IGNORE_DEFAULT_CASE_REQUIRED_COMMENT) {
                require_default_case = false;
            }
            // Upstream uses a second `if` rather than `else if`, so a switch
            // carrying both directives ends up enforcing.
            if directives.contains(&ENFORCE_DEFAULT_CASE_REQUIRED_COMMENT) {
                require_default_case = true;
            }
            report_switch(f, &options, require_default_case, &mut pending);
        }

        for (stack, pos, msg) in &maps {
            let related = map_related_comments(&cmap, stack);
            if !options.explicit_exhaustive_map && has_comment_prefix(&related, IGNORE_COMMENT) {
                continue;
            }
            if options.explicit_exhaustive_map && !has_comment_prefix(&related, ENFORCE_COMMENT) {
                continue;
            }
            pending.push((*pos, msg.clone()));
        }
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| {
        ensure_enum_members_decoder();
        Analyzer {
            name: "exhaustive",
            doc: "Check exhaustiveness of enum switch statements",
            url: "https://github.com/nishanths/exhaustive",
            run: run as RunFn,
            run_despite_errors: false,
            requires: vec![inspect::analyzer()],
            fact_types: vec![FactTypeId::of::<EnumMembersFact>()],
        }
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
