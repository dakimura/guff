//! Port of [`github.com/nishanths/exhaustive`](https://github.com/nishanths/exhaustive)
//! v0.15.0 (golangci-lint 2.15.0, `pkg/golinters/exhaustive`): the
//! `passes/exhaustive` analyzer over the `passes/enumerated` result.
//!
//! An enumerated type is a defined (not alias, not generic) type whose
//! underlying type is numeric or string, and its members are the constants
//! declared with exactly that type in the type's own scope.
//!
//! Defaults match golangci: `-check=switch`, `default` does **not** make a
//! switch exhaustive, and generated files are checked (golangci pins `-g`).
//!
//! `-check=typeswitch` stands upstream's `finder` facts in with the package
//! scopes of the package and every transitive import: golangci inherits a
//! `TypeName` fact from every dependency (`exportedFrom` keeps them all), so
//! the candidates are every package-level type name in the import closure.
//!
//! DEFERRED: `invalid value for flag -check` and uncompilable patterns, which
//! fail the analyzer upstream; `-casenil`, which golangci never sets.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{CommentGroup, CompositeLit, Expr, Spec, Stmt, SwitchStmt, TypeSwitchStmt};
use guff::commentmap::{new_comment_map, CommentMap};
use guff::position::Pos;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Fact, FactTypeId, Pass, RunError, RunFn};
use guff_types::alias::unalias_readonly;
use guff_types::api_predicates::api_implements;
use guff_types::arena::{ObjectData, ObjectId, PackageId, TypeArena, TypeData};
use guff_types::basic::{IS_COMPLEX, IS_FLOAT, IS_INTEGER, IS_STRING};
use guff_types::object::is_exported;
use guff_types::pointer::new_pointer;
use guff_types::typeset::compute_interface_type_set;
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


/// One enumerated constant.
#[derive(Clone, Debug)]
struct Member {
    name: String,
    /// `constant.Value.ExactString()`.
    value: String,
    /// `isPkgLevel(c)`.
    pkg_level: bool,
}

/// An enumerated type and its constants, in declaration (AST position) order.
#[derive(Clone)]
struct EnumTypeInfo {
    /// Declared in the package under analysis.
    local: bool,
    pkg_name: String,
    pkg_path: String,
    members: Vec<Member>,
}

/// enumerated `permittedEnumeratedType`: the type is *itself* a
/// `*types.Named` (a constant spelled with an alias of an enum type is not a
/// member), not generic or an instance, over a numeric (complex included) or
/// string basic type.
fn permitted_enum_type(pass: &Pass<'_>, typ: TypeId) -> Option<ObjectId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let TypeData::Named(n) = artifacts.types.get(typ) else {
        return None;
    };
    if n.type_params().is_some() || n.instance().is_some() {
        return None;
    }
    let TypeData::Basic(b) = artifacts.types.get(typ.underlying(&artifacts.types)) else {
        return None;
    };
    let info = b.info();
    if !(info.contains(IS_INTEGER)
        || info.contains(IS_FLOAT)
        || info.contains(IS_COMPLEX)
        || info.contains(IS_STRING))
    {
        return None;
    }
    Some(n.obj())
}

/// enumerated `permittedEnumeratedConstant`: `(type name, member)`.
fn permitted_enum_constant(pass: &Pass<'_>, obj: ObjectId) -> Option<(ObjectId, Member)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::Const(c) = artifacts.objects.get(obj) else {
        return None;
    };
    let type_name = permitted_enum_type(pass, c.typ())?;
    if c.name() == "_" {
        return None;
    }
    if type_name.parent(&artifacts.objects) != obj.parent(&artifacts.objects) {
        return None;
    }
    Some((
        type_name,
        Member {
            name: c.name().to_string(),
            value: c.val().exact_string(),
            pkg_level: is_pkg_level(pass, obj),
        },
    ))
}

/// `isPkgLevel`: the object is what its package scope holds under its name.
fn is_pkg_level(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(pid) = obj.pkg(&artifacts.objects) else {
        return false;
    };
    let scope = artifacts.packages.get(pid).scope();
    let name = obj.name(&artifacts.objects).to_string();
    artifacts.scopes.get(scope).lookup_local(&name) == Some(obj)
}

/// `fullname`: `pkgpath.Name` of a package-level object.
fn full_name(pass: &Pass<'_>, obj: ObjectId) -> Option<String> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let pid = obj.pkg(&artifacts.objects)?;
    Some(format!(
        "{}.{}",
        artifacts.packages.get(pid).path(),
        obj.name(&artifacts.objects)
    ))
}

/// enumerated `find`: every `const` declaration of the package, nested ones
/// included (`inspect.Preorder` visits function bodies too).
fn find_enums(pass: &Pass<'_>, package_scope_only: bool) -> HashMap<ObjectId, EnumTypeInfo> {
    let mut order: Vec<ObjectId> = Vec::new();
    let mut by_type: HashMap<ObjectId, Vec<Member>> = HashMap::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return HashMap::new();
    };
    let defs = |ident: &guff::ast::Ident| {
        pass.types_info()
            .and_then(|info| info.defs.get(&ident.id).copied().flatten())
    };
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::GenDecl(gen)) = n else {
                return true;
            };
            if gen.tok != Some(Token::CONST) {
                return true;
            }
            for spec in &gen.specs {
                let Spec::ValueSpec(vs) = spec else {
                    continue;
                };
                for ident in &vs.names {
                    let Some(obj) = defs(ident) else {
                        continue;
                    };
                    let Some((type_name, member)) = permitted_enum_constant(pass, obj) else {
                        continue;
                    };
                    if package_scope_only && !member.pkg_level {
                        continue;
                    }
                    by_type
                        .entry(type_name)
                        .or_insert_with(|| {
                            order.push(type_name);
                            Vec::new()
                        })
                        .push(member);
                }
            }
            true
        });
    }
    let mut out = HashMap::new();
    for type_name in order {
        let members = by_type.remove(&type_name).unwrap_or_default();
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
                local: true,
                pkg_name,
                pkg_path,
                members,
            },
        );
    }
    out
}

fn export_enum_facts(pass: &mut Pass<'_>, enums: &HashMap<ObjectId, EnumTypeInfo>) {
    for (type_name, info) in enums {
        // Only a package-level type can be named from another package.
        if !info.members.iter().all(|m| m.pkg_level) {
            continue;
        }
        let mut fact = EnumMembersFact::default();
        for m in &info.members {
            fact.names.push(m.name.clone());
            fact.name_to_value.insert(m.name.clone(), m.value.clone());
        }
        pass.export_object_fact(*type_name, Box::new(fact));
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
/// is absent. Every member upstream would require is in that package's scope
/// regardless of how the package arrived: `needValues` drops a foreign
/// unexported constant, and the exported ones are what export data carries.
///
/// The fact's own order is AST order, which the scope does not keep — and
/// which the message depends on, since `formatMissingNames` sorts by
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
        let Some((owner, member)) = permitted_enum_constant(pass, obj) else {
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
            member.name,
            member.value,
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

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

/// `knownEnumeratedType`: through aliases to a `*types.Named` the
/// enumerated result knows. Its underlying type is never itself an enum.
fn known_enum_type(
    pass: &Pass<'_>,
    local: &HashMap<ObjectId, EnumTypeInfo>,
    typ: TypeId,
) -> Option<EnumTypeInfo> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = unalias_readonly(&artifacts.types, typ);
    let TypeData::Named(n) = artifacts.types.get(typ) else {
        return None;
    };
    let type_name = n.obj();
    if let Some(info) = local.get(&type_name) {
        return Some(info.clone());
    }
    let fact = match import_enum_members(pass, type_name) {
        Some(m) => m,
        None => enum_members_from_scope(pass, type_name)?,
    };
    let pid = type_name.pkg(&artifacts.objects)?;
    let p = artifacts.packages.get(pid);
    let members = fact
        .names
        .iter()
        .filter_map(|name| {
            Some(Member {
                name: name.clone(),
                value: fact.name_to_value.get(name)?.clone(),
                pkg_level: true,
            })
        })
        .collect();
    Some(EnumTypeInfo {
        local: Some(pid) == pass.type_pkg(),
        pkg_name: p.name().to_string(),
        pkg_path: p.path().to_string(),
        members,
    })
}

/// `hasTypeName`: the object naming the type as written — an alias names
/// itself, not what it stands for.
fn type_name_of(pass: &Pass<'_>, typ: TypeId) -> Option<ObjectId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.types.get(typ) {
        TypeData::Alias(a) => Some(a.obj()),
        TypeData::Named(n) => Some(n.obj()),
        TypeData::TypeParam(tp) => Some(tp.obj()),
        _ => None,
    }
}

/// The compiled `-typeonly` / `-typeignore` / `-constignore` patterns.
struct Patterns {
    only_types: Vec<Regex>,
    ignore_types: Vec<Regex>,
    ignore_constants: Vec<Regex>,
}

impl Patterns {
    fn new(options: &ExhaustiveOptions) -> Self {
        let compile = |v: &[String]| v.iter().filter_map(|p| Regex::new(p).ok()).collect();
        Self {
            only_types: compile(&options.only_types),
            ignore_types: compile(&options.ignore_types),
            ignore_constants: compile(&options.ignore_constants),
        }
    }
}

fn match_any(rs: &[Regex], s: &str) -> bool {
    rs.iter().any(|r| r.is_match(s))
}

/// `proceedInclExclPatterns`: an include pattern, when any is given, must
/// match (and then wins over the excludes); patterns only ever match a
/// package-level type.
fn proceed_incl_excl(pass: &Pass<'_>, obj: ObjectId, patterns: &Patterns) -> bool {
    let full = || is_pkg_level(pass, obj).then(|| full_name(pass, obj)).flatten();
    if !patterns.only_types.is_empty() {
        return full().is_some_and(|f| match_any(&patterns.only_types, &f));
    }
    !full().is_some_and(|f| match_any(&patterns.ignore_types, &f))
}

/// `needValues`, grouped by value: each group's constants in position order,
/// the groups in the order of their first constant.
fn need_values(enum_info: &EnumTypeInfo, patterns: &Patterns) -> Vec<(String, Vec<String>)> {
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for m in &enum_info.members {
        if !enum_info.local && !is_exported(&m.name) {
            continue;
        }
        if m.pkg_level
            && match_any(
                &patterns.ignore_constants,
                &format!("{}.{}", enum_info.pkg_path, m.name),
            )
        {
            continue;
        }
        match groups.iter_mut().find(|(v, _)| *v == m.value) {
            Some((_, names)) => names.push(m.name.clone()),
            None => groups.push((m.value.clone(), vec![m.name.clone()])),
        }
    }
    groups
}

/// `formatMissingNames`: `nameString` for each constant, `|` within a value
/// group, `, ` between groups.
fn format_missing_names(enum_info: &EnumTypeInfo, groups: &[(String, Vec<String>)]) -> String {
    let name = |n: &str| {
        if enum_info.local {
            n.to_string()
        } else {
            format!("{}.{n}", enum_info.pkg_name)
        }
    };
    groups
        .iter()
        .map(|(_, names)| names.iter().map(|n| name(n)).collect::<Vec<_>>().join("|"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `TypesInfo.Types[expr].Value.ExactString()`: any constant expression
/// counts — a literal, a conversion, `X+1`.
fn const_value(pass: &Pass<'_>, expr: &Expr) -> Option<String> {
    let tv = pass.types_info()?.types.get(&expr.id())?;
    Some(tv.val.as_ref()?.exact_string())
}

// ============================================================
// `//exhaustive:` comment directives
// ============================================================

/// `parseDirectives`' result.
#[derive(Default, Clone, Copy)]
struct Directives {
    ignore: bool,
    enforce: bool,
    /// `defrequire=0` / `defrequire=1` (and their deprecated long names).
    defrequire: Option<bool>,
}

/// `go/ast.ParseDirective` (go1.26): `//tool:name args`, where `tool` and the
/// first byte of `name` are `[a-z0-9]`, and `args` is everything after the
/// name with the surrounding white space trimmed.
fn parse_directive(c: &str) -> Option<(&str, &str, &str)> {
    let b = c.as_bytes();
    let alnum = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !(b.len() >= 3 && b[0] == b'/' && b[1] == b'/' && alnum(b[2])) {
        return None;
    }
    let s = &c[2..];
    let colon = s.find(':')?;
    if colon == 0 || colon + 1 >= s.len() {
        return None;
    }
    let sb = s.as_bytes();
    if (0..=colon + 1).any(|i| i != colon && !alnum(sb[i])) {
        return None;
    }
    let tool = &s[..colon];
    let rest = &s[colon + 1..];
    let name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let name = &rest[..name_end];
    let args = rest[name_end..].trim_matches(char::is_whitespace);
    Some((tool, name, args))
}

/// `parseDirectives`: any error stops the scan and the node is not checked.
fn parse_directives<'a>(texts: impl Iterator<Item = &'a str>) -> Result<Directives, String> {
    const CONFLICT: &str = "conflicting directives";
    let mut d = Directives::default();
    for text in texts {
        let Some((tool, name, args)) = parse_directive(text) else {
            continue;
        };
        if tool != "exhaustive" {
            continue;
        }
        if !args.is_empty() {
            return Err("args not allowed".to_string());
        }
        match name {
            "ignore" => d.ignore = true,
            "enforce" => d.enforce = true,
            "defrequire=0" | "ignore-default-case-required" => {
                if d.defrequire == Some(true) {
                    return Err(CONFLICT.to_string());
                }
                d.defrequire = Some(false);
            }
            "defrequire=1" | "enforce-default-case-required" => {
                if d.defrequire == Some(false) {
                    return Err(CONFLICT.to_string());
                }
                d.defrequire = Some(true);
            }
            _ => return Err(format!("invalid name {}", guff_gostd::strconv::quote(name))),
        }
    }
    if d.ignore && d.enforce {
        return Err(CONFLICT.to_string());
    }
    Ok(d)
}

fn group_texts<'a>(groups: impl IntoIterator<Item = &'a CommentGroup>) -> Vec<&'a str> {
    groups
        .into_iter()
        .flat_map(|g| g.list.iter().map(|c| c.text.as_str()))
        .collect()
}

/// `Doc` / `Comment` groups of the declarations in one file, read from the
/// comment-carrying reparse (the analysis AST has none) and keyed by the
/// analysis position of the `GenDecl` keyword or the `ValueSpec`'s first name.
#[derive(Default)]
struct DeclComments {
    gen_decls: HashMap<Pos, Vec<String>>,
    value_specs: HashMap<Pos, Vec<String>>,
}

impl DeclComments {
    fn of(re: &guff_analysis::comments::Reparsed) -> Self {
        let mut out = Self::default();
        let texts = |groups: &[Option<&CommentGroup>]| -> Vec<String> {
            groups
                .iter()
                .flatten()
                .flat_map(|g| g.list.iter().map(|c| c.text.clone()))
                .collect()
        };
        walk::inspect(NodeRef::File(&re.file), |n| {
            match n {
                Some(NodeRef::GenDecl(gen)) => {
                    out.gen_decls
                        .insert(re.rebase(gen.tok_pos), texts(&[gen.doc.as_ref()]));
                }
                Some(NodeRef::ValueSpec(vs)) => {
                    if let Some(first) = vs.names.first() {
                        out.value_specs.insert(
                            re.rebase(first.name_pos),
                            texts(&[vs.doc.as_ref(), vs.comment.as_ref()]),
                        );
                    }
                }
                _ => {}
            }
            true
        });
        out
    }
}

/// `compositeLitComments`: outwards from the literal, the comments of the
/// first `AssignStmt`, or of every `ValueSpec` (doc, line comment, mapped)
/// up to and including the first `GenDecl` (doc, mapped). Nothing else
/// contributes, but the walk goes on through any other node to reach them.
/// A group may be listed twice, which no directive outcome depends on.
fn map_related_comments<'a>(
    cm: &'a CommentMap<'a>,
    decls: &'a DeclComments,
    stack: &[NodeRef<'a>],
) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    let mapped = |out: &mut Vec<&'a str>, node: NodeRef<'a>| {
        out.extend(group_texts(cm.get(node).unwrap_or(&[]).iter()));
    };
    for node in stack.iter().rev() {
        match node {
            NodeRef::AssignStmt(_) => {
                mapped(&mut out, *node);
                break;
            }
            NodeRef::ValueSpec(vs) => {
                if let Some(t) = vs.names.first().and_then(|n| decls.value_specs.get(&n.name_pos)) {
                    out.extend(t.iter().map(String::as_str));
                }
                mapped(&mut out, *node);
            }
            NodeRef::GenDecl(gen) => {
                if let Some(t) = decls.gen_decls.get(&gen.tok_pos) {
                    out.extend(t.iter().map(String::as_str));
                }
                mapped(&mut out, *node);
                break;
            }
            _ => {}
        }
    }
    out
}

/// `tp`: a type name `T`, or `*T` when `pointer`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Tp {
    obj: ObjectId,
    pointer: bool,
}

/// `typeSwitchState` plus the `finder` result it reads, built on the first
/// type switch that gets that far.
#[derive(Default)]
struct TypeSwitchState {
    /// `finder.Result.TypeDecls`: the package-level type names (defined or
    /// alias) of the package and of every package it transitively imports.
    decls: Option<Vec<ObjectId>>,
    /// A copy of the type arena: `types.NewPointer` and the lazily computed
    /// interface type sets need somewhere to go.
    scratch: Option<TypeArena>,
    /// `impl` (each entry once), keyed by the switched interface's
    /// underlying type.
    implements: HashMap<TypeId, Vec<Tp>>,
    /// `identical`.
    identical: Option<HashMap<Tp, Vec<Tp>>>,
}

struct Checker<'p, 'a> {
    pass: &'p Pass<'a>,
    options: &'p ExhaustiveOptions,
    patterns: &'p Patterns,
    enums: &'p HashMap<ObjectId, EnumTypeInfo>,
    type_switches: TypeSwitchState,
    reports: Vec<(u32, String)>,
}

impl Checker<'_, '_> {
    /// `checkSwitch`.
    fn check_switch(&mut self, sw: &SwitchStmt, comments: &[&str]) {
        let pos = sw.switch.0 as u32;
        let directives = match parse_directives(comments.iter().copied()) {
            Ok(d) => d,
            Err(err) => {
                self.reports
                    .push((pos, format!("error parsing comment directives: {err}")));
                return;
            }
        };
        if self.options.explicit_exhaustive && !directives.enforce {
            return;
        }
        if directives.ignore {
            return;
        }
        let skip = |reports: &mut Vec<(u32, String)>, reason: &str| {
            if directives.enforce {
                reports.push((
                    pos,
                    format!("enforce directive present and expression switch not checked: {reason}"),
                ));
            }
        };
        let Some(tag) = sw.tag.as_ref() else {
            skip(&mut self.reports, "no switch expression");
            return;
        };
        let tag_typ = type_of_expr(self.pass, tag);
        let Some(enum_info) = tag_typ.and_then(|t| known_enum_type(self.pass, self.enums, t)) else {
            skip(&mut self.reports, "switch expression type is not an enumerated type");
            return;
        };
        if !directives.enforce {
            if let Some(tn) = tag_typ.and_then(|t| type_name_of(self.pass, t)) {
                if !proceed_incl_excl(self.pass, tn, self.patterns) {
                    return;
                }
            }
        }

        let mut report_missing_default = directives
            .defrequire
            .unwrap_or(self.options.default_case_required);
        let mut need = need_values(&enum_info, self.patterns);
        for stmt in &sw.body.list {
            let Stmt::CaseClause(cc) = stmt else {
                continue;
            };
            if cc.list.is_empty() {
                report_missing_default = false;
                if self.options.default_signifies_exhaustive {
                    need.clear();
                    break;
                }
            }
            for expr in &cc.list {
                if let Some(v) = const_value(self.pass, expr) {
                    need.retain(|(value, _)| *value != v);
                }
            }
        }
        if report_missing_default {
            self.reports.push((pos, "missing default case".to_string()));
        }
        if !need.is_empty() {
            self.reports.push((
                pos,
                format!(
                    "missing cases in expression switch: {}",
                    format_missing_names(&enum_info, &need)
                ),
            ));
        }
    }

    /// `checkMapLiteral`, for a literal whose (pointer-stripped) type is a map
    /// — `map_key` gives its key type.
    fn check_map_literal(&mut self, lit: &CompositeLit, key: TypeId, comments: &[&str]) {
        // `compLit.Pos()`: the type, or `{` when it is elided.
        let pos = lit.ty.as_ref().map_or(lit.lbrace, |t| t.pos()).0 as u32;
        let directives = match parse_directives(comments.iter().copied()) {
            Ok(d) => d,
            Err(err) => {
                self.reports
                    .push((pos, format!("error parsing comment directives: {err}")));
                return;
            }
        };
        if self.options.explicit_exhaustive && !directives.enforce {
            return;
        }
        if directives.ignore {
            return;
        }
        let Some(enum_info) = known_enum_type(self.pass, self.enums, key) else {
            if directives.enforce {
                self.reports.push((
                    pos,
                    "enforce directive present and map literal not checked: key type is not an enumerated type"
                        .to_string(),
                ));
            }
            return;
        };
        if !directives.enforce {
            if let Some(tn) = type_name_of(self.pass, key) {
                if !proceed_incl_excl(self.pass, tn, self.patterns) {
                    return;
                }
            }
        }
        let mut need = need_values(&enum_info, self.patterns);
        for elt in &lit.elts {
            let Expr::KeyValueExpr(kv) = elt else {
                continue;
            };
            if let Some(v) = const_value(self.pass, &kv.key) {
                need.retain(|(value, _)| *value != v);
            }
        }
        if !need.is_empty() {
            self.reports.push((
                pos,
                format!(
                    "missing keys in map literal: {}",
                    format_missing_names(&enum_info, &need)
                ),
            ));
        }
    }

    /// `checkTypeSwitch`.
    fn check_type_switch(&mut self, sw: &TypeSwitchStmt, comments: &[&str]) {
        let pass = self.pass;
        let pos = sw.switch.0 as u32;
        let directives = match parse_directives(comments.iter().copied()) {
            Ok(d) => d,
            Err(err) => {
                self.reports
                    .push((pos, format!("error parsing comment directives: {err}")));
                return;
            }
        };
        if self.options.explicit_exhaustive && !directives.enforce {
            return;
        }
        if directives.ignore {
            return;
        }
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return;
        };
        let sw_expr = match sw.assign.as_ref() {
            Stmt::ExprStmt(e) => type_assert_operand(&e.x),
            Stmt::AssignStmt(a) => a.rhs.first().and_then(type_assert_operand),
            _ => None,
        };
        let Some(sw_type) = sw_expr.and_then(|e| type_of_expr(pass, e)) else {
            return;
        };

        let scratch = self
            .type_switches
            .scratch
            .get_or_insert_with(|| artifacts.types.clone());
        let iface = sw_type.underlying(scratch);
        let (is_method_set, empty) = match interface_type_set(pass, scratch, iface) {
            Some(v) => v,
            None => (false, false),
        };
        if !is_method_set {
            if directives.enforce {
                self.reports.push((
                    pos,
                    "enforce directive present and type switch not checked: expression in type assertion is not basic interface"
                        .to_string(),
                ));
            }
            return;
        }

        if !directives.enforce {
            let tn = type_name_of(pass, sw_type);
            if let Some(tn) = tn {
                if !proceed_incl_excl(pass, tn, self.patterns) {
                    return;
                }
            }
            // The empty interface and the predeclared `error` are not checked.
            if empty {
                return;
            }
            if tn.is_some_and(|tn| {
                tn.name(&artifacts.objects) == "error" && tn.pkg(&artifacts.objects).is_none()
            }) {
                return;
            }
        }

        let need = self.need_types(iface);
        let mut need: HashMap<Tp, Vec<Tp>> = need;
        // `-casenil` is never set by golangci.
        let mut report_missing_nil = false;
        for stmt in &sw.body.list {
            let Stmt::CaseClause(cc) = stmt else {
                continue;
            };
            if cc.list.is_empty() && self.options.default_signifies_exhaustive {
                report_missing_nil = false;
                need.clear();
                break;
            }
            for expr in &cc.list {
                let Some(t) = type_of_expr(pass, expr) else {
                    continue;
                };
                let t = unalias_readonly(&artifacts.types, t);
                match artifacts.types.get(t) {
                    TypeData::Named(n) => {
                        need.remove(&Tp { obj: n.obj(), pointer: false });
                    }
                    TypeData::Pointer(p) => {
                        let elem = unalias_readonly(&artifacts.types, p.elem());
                        if let TypeData::Named(n) = artifacts.types.get(elem) {
                            need.remove(&Tp { obj: n.obj(), pointer: true });
                        }
                    }
                    TypeData::Basic(_) if is_untyped_nil(pass, t) => report_missing_nil = false,
                    _ => {}
                }
            }
        }
        if report_missing_nil || !need.is_empty() {
            self.reports.push((
                pos,
                format!(
                    "missing cases in type switch: {}",
                    format_missing_types(pass, need, report_missing_nil)
                ),
            ));
        }
    }

    /// `computeImplements` + `computeTypeRelations` + `needTypes`.
    fn need_types(&mut self, iface: TypeId) -> HashMap<Tp, Vec<Tp>> {
        let pass = self.pass;
        let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
            return HashMap::new();
        };
        let state = &mut self.type_switches;
        let decls = state.decls.get_or_insert_with(|| type_decls(pass));
        let scratch = state
            .scratch
            .get_or_insert_with(|| artifacts.types.clone());

        if !state.implements.contains_key(&iface) {
            let mut found = Vec::new();
            for &tn in decls.iter() {
                let Some(t) = tn.typ(&artifacts.objects) else {
                    continue;
                };
                let t = unalias_readonly(scratch, t);
                match scratch.get(t) {
                    TypeData::Named(n) => {
                        let obj = n.obj();
                        if underlying_is_interface(scratch, t) {
                            continue;
                        }
                        let ptr = new_pointer(scratch, t);
                        if api_implements(scratch, &artifacts.objects, &artifacts.packages, ptr, iface) {
                            found.push(Tp { obj, pointer: true });
                        }
                        if api_implements(scratch, &artifacts.objects, &artifacts.packages, t, iface) {
                            found.push(Tp { obj, pointer: false });
                        }
                    }
                    TypeData::Pointer(p) => {
                        let elem = unalias_readonly(scratch, p.elem());
                        let TypeData::Named(n) = scratch.get(elem) else {
                            continue;
                        };
                        let obj = n.obj();
                        if underlying_is_interface(scratch, elem) {
                            continue;
                        }
                        let ptr = new_pointer(scratch, elem);
                        if api_implements(scratch, &artifacts.objects, &artifacts.packages, ptr, iface) {
                            found.push(Tp { obj, pointer: true });
                        }
                    }
                    _ => {}
                }
            }
            // A set upstream: an alias declaration reaches its target's
            // entry again.
            let mut seen = HashSet::new();
            found.retain(|t| seen.insert(*t));
            state.implements.insert(iface, found);
        }

        let identical = state.identical.get_or_insert_with(|| {
            let mut m: HashMap<Tp, Vec<Tp>> = HashMap::new();
            let mut add = |root: Tp, t: Tp| m.entry(root).or_default().push(t);
            for &tn in decls.iter() {
                let Some(t) = tn.typ(&artifacts.objects) else {
                    continue;
                };
                match artifacts.types.get(t) {
                    TypeData::Alias(_) => {
                        let u = unalias_readonly(&artifacts.types, t);
                        match artifacts.types.get(u) {
                            TypeData::Named(n) => {
                                add(Tp { obj: n.obj(), pointer: false }, Tp { obj: tn, pointer: false });
                                add(Tp { obj: n.obj(), pointer: true }, Tp { obj: tn, pointer: true });
                            }
                            TypeData::Pointer(p) => {
                                let elem = unalias_readonly(&artifacts.types, p.elem());
                                if let TypeData::Named(n) = artifacts.types.get(elem) {
                                    add(Tp { obj: n.obj(), pointer: true }, Tp { obj: tn, pointer: false });
                                }
                            }
                            _ => {}
                        }
                    }
                    TypeData::Named(n) => {
                        add(Tp { obj: n.obj(), pointer: false }, Tp { obj: tn, pointer: false });
                        add(Tp { obj: n.obj(), pointer: true }, Tp { obj: tn, pointer: true });
                    }
                    _ => {}
                }
            }
            m
        });

        let current = pass.type_pkg();
        let mut need: HashMap<Tp, Vec<Tp>> = HashMap::new();
        for t in &state.implements[&iface] {
            for name in identical.get(t).map(Vec::as_slice).unwrap_or(&[]) {
                if name.obj.pkg(&artifacts.objects) != current && !name.obj.exported(&artifacts.objects) {
                    continue;
                }
                need.entry(*t).or_default().push(*name);
            }
        }
        need
    }
}

/// The operand of the `x.(type)` a type switch's guard asserts on.
fn type_assert_operand(e: &Expr) -> Option<&Expr> {
    match unparen_ref(e) {
        Expr::TypeAssertExpr(ta) => Some(unparen_ref(&ta.x)),
        _ => None,
    }
}

fn unparen_ref(mut e: &Expr) -> &Expr {
    while let Expr::ParenExpr(p) = e {
        e = &p.x;
    }
    e
}

/// `(IsMethodSet, Empty)` of the interface `iface`, or `None` when it is not
/// an interface.
fn interface_type_set(pass: &Pass<'_>, scratch: &mut TypeArena, iface: TypeId) -> Option<(bool, bool)> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if !matches!(scratch.get(iface), TypeData::Interface(_)) {
        return None;
    }
    compute_interface_type_set(scratch, &artifacts.objects, &artifacts.packages, iface);
    let TypeData::Interface(i) = scratch.get(iface) else {
        return None;
    };
    let ts = i.cached_typeset()?;
    Some((ts.is_method_set(), ts.is_all()))
}

fn underlying_is_interface(types: &TypeArena, t: TypeId) -> bool {
    matches!(types.get(t.underlying(types)), TypeData::Interface(_))
}

fn is_untyped_nil(pass: &Pass<'_>, t: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    matches!(artifacts.types.get(t), TypeData::Basic(b) if b.kind() == guff_types::basic::BasicKind::UntypedNil)
}

/// `finder.Result.TypeDecls`: every package-level type name whose type is a
/// defined type or an alias, in the package and its import closure.
fn type_decls(pass: &Pass<'_>) -> Vec<ObjectId> {
    let (Some(artifacts), Some(root)) = (pass.pkg().type_artifacts.as_ref(), pass.type_pkg()) else {
        return Vec::new();
    };
    let mut seen: HashSet<PackageId> = HashSet::new();
    let mut queue = vec![root];
    // A set upstream (map keys).
    let mut found: HashSet<ObjectId> = HashSet::new();
    let mut out = Vec::new();
    while let Some(pkg) = queue.pop() {
        if !seen.insert(pkg) {
            continue;
        }
        let p = artifacts.packages.get(pkg);
        queue.extend(p.imports().iter().copied());
        let scope = artifacts.scopes.get(p.scope());
        for name in scope.names() {
            let Some(obj) = scope.lookup_local(&name) else {
                continue;
            };
            let ObjectData::TypeName(tn) = artifacts.objects.get(obj) else {
                continue;
            };
            let Some(t) = tn.typ() else {
                continue;
            };
            if matches!(artifacts.types.get(t), TypeData::Alias(_) | TypeData::Named(_)) && found.insert(obj) {
                out.push(obj);
            }
        }
    }
    out
}

/// `TypeName.IsAlias` for a declared type name.
fn type_name_is_alias(pass: &Pass<'_>, obj: ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    match obj.typ(&artifacts.objects).map(|t| artifacts.types.get(t)) {
        Some(TypeData::Alias(_)) => true,
        Some(TypeData::Named(n)) => n.obj() != obj,
        _ => false,
    }
}

/// `formatMissingTypes`.
fn format_missing_types(pass: &Pass<'_>, missing: HashMap<Tp, Vec<Tp>>, missing_nil: bool) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    let current = pass.type_pkg();
    let objects = &artifacts.objects;
    let cmpfn = |a: &Tp, b: &Tp| -> Ordering {
        let (aa, ba) = (type_name_is_alias(pass, a.obj), type_name_is_alias(pass, b.obj));
        if aa != ba {
            return if !aa { Ordering::Less } else { Ordering::Greater };
        }
        let (ap, bp) = (a.obj.pkg(objects), b.obj.pkg(objects));
        match (ap, bp) {
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            _ => {}
        }
        if ap == current && bp != current {
            return Ordering::Less;
        }
        if ap != current && bp == current {
            return Ordering::Greater;
        }
        if ap != bp {
            let path = |p: Option<PackageId>| p.map(|p| artifacts.packages.get(p).path().to_string()).unwrap_or_default();
            return path(ap).cmp(&path(bp));
        }
        let (an, bn) = (a.obj.name(objects), b.obj.name(objects));
        if an != bn {
            return an.cmp(bn);
        }
        a.pointer.cmp(&b.pointer)
    };
    let tp_string = |t: &Tp| {
        let pkg = t.obj.pkg(objects);
        let name = t.obj.name(objects);
        let base = match pkg {
            Some(p) if Some(p) != current => format!("{}.{name}", artifacts.packages.get(p).name()),
            _ => name.to_string(),
        };
        if t.pointer {
            format!("*{base}")
        } else {
            base
        }
    };
    let mut groups: Vec<(Tp, Vec<Tp>)> = missing
        .into_iter()
        .map(|(t, mut names)| {
            names.sort_by(&cmpfn);
            (t, names)
        })
        .collect();
    groups.sort_by(|a, b| cmpfn(&a.0, &b.0));
    let mut parts: Vec<String> = Vec::new();
    if missing_nil {
        parts.push("nil".to_string());
    }
    for (_, names) in &groups {
        parts.push(names.iter().map(&tp_string).collect::<Vec<_>>().join(" or "));
    }
    parts.join(", ")
}

/// The key type of a literal `checkMapLiteral` gets past its first test for:
/// `unpointer(Types[compLit].Type.Underlying())` is a map. Elided inner
/// literals (`[]*map[K]V{{…}}`) and empty ones are checked too.
fn map_literal_key(pass: &Pass<'_>, lit: &CompositeLit) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let tv = pass.types_info()?.types.get(&lit.id)?;
    let mut t = unalias_readonly(&artifacts.types, tv.typ).underlying(&artifacts.types);
    while let TypeData::Pointer(p) = artifacts.types.get(t) {
        t = p.elem();
    }
    match artifacts.types.get(t) {
        TypeData::Map(m) => Some(m.key()),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "exhaustive requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<ExhaustiveOptions>("exhaustive")
        .cloned()
        .unwrap_or_default();
    let patterns = Patterns::new(&options);

    let enums = find_enums(pass, options.package_scope_only);
    export_enum_facts(pass, &enums);

    let mut checker = Checker {
        pass,
        options: &options,
        patterns: &patterns,
        enums: &enums,
        type_switches: TypeSwitchState::default(),
        reports: Vec::new(),
    };
    for file in checker.pass.files() {
        // Ancestor stack, maintained the way `Cursor.Enclosing` sees it:
        // `walk::inspect` calls back with `None` on the way out of a node.
        let mut stack: Vec<NodeRef<'_>> = Vec::new();
        let mut switches: Vec<&SwitchStmt> = Vec::new();
        let mut type_switches: Vec<&TypeSwitchStmt> = Vec::new();
        let mut maps: Vec<(Vec<NodeRef<'_>>, &CompositeLit, TypeId)> = Vec::new();
        walk::inspect(NodeRef::File(file), |n| {
            match n {
                Some(node) => {
                    stack.push(node);
                    match node {
                        NodeRef::SwitchStmt(sw) if options.check_switch => switches.push(sw),
                        NodeRef::TypeSwitchStmt(sw) if options.check_type_switch => {
                            type_switches.push(sw)
                        }
                        NodeRef::CompositeLit(lit) if options.check_map => {
                            if let Some(key) = map_literal_key(checker.pass, lit) {
                                maps.push((stack.clone(), lit, key));
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
        if switches.is_empty() && maps.is_empty() && type_switches.is_empty() {
            continue;
        }

        // The analysis AST carries no comments, so the comment map costs a
        // reparse. Skipped when the file holds no `//exhaustive:` text at
        // all: then every directive set is empty.
        let reparsed = if guff_analysis::comments::file_source_contains(
            checker.pass,
            file,
            b"//exhaustive:",
        ) {
            guff_analysis::comments::reparse_with_comments(checker.pass, file)
        } else {
            None
        };
        let groups: Vec<CommentGroup> = reparsed
            .as_ref()
            .map(|re| re.file.comments.iter().map(|g| re.rebase_group(g)).collect())
            .unwrap_or_default();
        let decls = reparsed.as_ref().map(DeclComments::of).unwrap_or_default();
        let cmap = new_comment_map(checker.pass.fset(), NodeRef::File(file), &groups);

        for sw in switches {
            let texts = group_texts(cmap.get(NodeRef::SwitchStmt(sw)).unwrap_or(&[]).iter());
            checker.check_switch(sw, &texts);
        }
        for (stack, lit, key) in &maps {
            let texts = map_related_comments(&cmap, &decls, stack);
            checker.check_map_literal(lit, *key, &texts);
        }
        for sw in type_switches {
            let texts = group_texts(cmap.get(NodeRef::TypeSwitchStmt(sw)).unwrap_or(&[]).iter());
            checker.check_type_switch(sw, &texts);
        }
    }

    let reports = std::mem::take(&mut checker.reports);
    for (pos, msg) in reports {
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
            doc: "check that expression switches and type switches are exhaustive",
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
    fn parse_directive_matches_go_ast() {
        assert_eq!(parse_directive("//exhaustive:ignore"), Some(("exhaustive", "ignore", "")));
        assert_eq!(
            parse_directive("//exhaustive:ignore // why"),
            Some(("exhaustive", "ignore", "// why"))
        );
        assert_eq!(parse_directive("//exhaustive:"), None);
        assert_eq!(parse_directive("// exhaustive:ignore"), None);
        assert_eq!(parse_directive("//exhaustive:-x"), None);
        assert_eq!(parse_directive("//Exhaustive:ignore"), None);
        assert_eq!(parse_directive("/*exhaustive:ignore*/"), None);
        assert_eq!(parse_directive("//go:build x"), Some(("go", "build", "x")));
    }

    #[test]
    fn parse_directives_errors() {
        let p = |v: &[&str]| parse_directives(v.iter().copied()).map(|d| (d.ignore, d.enforce, d.defrequire));
        assert_eq!(p(&["//exhaustive:ignore"]), Ok((true, false, None)));
        assert_eq!(p(&["//exhaustive:ignore x"]), Err("args not allowed".to_string()));
        assert_eq!(p(&["//exhaustive:ignoreme"]), Err("invalid name \"ignoreme\"".to_string()));
        assert_eq!(
            p(&["//exhaustive:ignore", "//exhaustive:enforce"]),
            Err("conflicting directives".to_string())
        );
        assert_eq!(
            p(&["//exhaustive:defrequire=1", "//exhaustive:ignore-default-case-required"]),
            Err("conflicting directives".to_string())
        );
        assert_eq!(p(&["//exhaustive:defrequire=0"]), Ok((false, false, Some(false))));
        assert_eq!(p(&["//exhaustive:", "//nolint:x"]), Ok((false, false, None)));
    }
}
