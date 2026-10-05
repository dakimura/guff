//! Port of [`github.com/alecthomas/go-check-sumtype`](https://github.com/alecthomas/go-check-sumtype)
//! at `v0.5.1-0.20260828200218-ae6904d28606`, the version golangci-lint 2.14.0
//! pins. `pkg/golinters/gochecksumtype` now hands golangci the module's own
//! `Analyzer` (renamed `gochecksumtype`, its two flags passed as config)
//! instead of wrapping `Run([]*packages.Package)` in a custom issue reporter.
//!
//! Exhaustiveness checks for interfaces marked `//sumtype:decl`. Variants are
//! the declaring package's named types that implement the sealed interface (at
//! least one unexported method).
//!
//! What the move to an `analysis.Analyzer` changed, each ported from the code
//! (`analyzer.go`, `fact.go`, `decl.go`, `def.go`, `check.go`):
//!
//! - **Facts** (`sumTypeFact`): a package exports its sum types and their
//!   variant names, and every *direct* import's fact is turned back into a
//!   definition (`factToTypeDefs`), so a switch over a sum type declared in
//!   an imported package is checked too. `Run` only ever saw the one package
//!   golangci handed it. guff has the fact when it analysed the import (it
//!   does for same-module imports when contextcheck has them type-checked);
//!   for any other import it reads the declarations from the import's source
//!   ([`source_type_defs`]), lazily — only when a switch is over a sealed
//!   interface no definition matches.
//! - **Aliases are not variants** (`def.go`): a candidate must be a
//!   `*types.Named`; `type Alias = A` used to be listed as a missing case
//!   beside `A`.
//! - **A `//sumtype:decl` on a declaration with no `TypeSpec`** is an analyzer
//!   *error* (`notFoundError` returned from `run`), which golangci turns into
//!   a failure of the whole `goanalysis_metalinter`. It used to be an issue
//!   (`type '' is not defined`). See [`DECL_NOT_FOUND`].
//! - Diagnostics go through `pass.Reportf`, so golangci's per-root dedup and
//!   its `//line` handling apply — guff's generic diagnostic path, which this
//!   analyzer was already on.
//!
//! Defaults match golangci: `default-signifies-exhaustive: true`,
//! `include-shared-interfaces: false`.
//!
//! The analysis AST carries no comments, so declaration docs come from a
//! reparse ([`reparse_with_comments`]) rebased into the analysis `FileSet`.
//! Raw comment text is inspected — `CommentGroup::text` strips
//! `sumtype:decl` as a Go directive.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use guff::ast::{Expr, Spec, Stmt, TypeSwitchStmt};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::walk::{preorder, preorder_prune, NodeRef};
use guff_analysis::comments::{file_source_contains, reparse_with_comments};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Fact, FactTypeId, Pass, RunError, RunFn};
use guff_types::api_predicates::{api_identical, api_implements};
use guff_types::arena::{ObjectData, ObjectId, PackageId, ScopeId, TypeData, TypeId};
use guff_types::interface::{interface_method, interface_num_methods};
use guff_types::object::is_exported;
use guff_types::pointer::{new_pointer, pointer_elem};
use guff_types::predicates::is_interface;

use crate::exhaustruct_v5::{file_stamp, is_goroot_file};
use crate::options::GochecksumtypeOptions;

const DIRECTIVE: &[u8] = b"//sumtype:decl";

/// The analyzer error `run` returns when a `//sumtype:decl` documents a
/// declaration with no `TypeSpec`: upstream's `notFoundError` with an empty
/// `TypeName`, returned (not reported) from `findSumTypeDecls`.
///
/// golangci-lint turns an analyzer error into a failure of the whole
/// `goanalysis_metalinter` — nothing printed but `Running error`, exit 3 — and
/// guff reproduces that for this error (`guff_analysis::run_failure`).
/// It is the only analyzer error guff surfaces: guff's other `RunError`s are
/// skips (ill-typed packages, failed prerequisites) with no upstream
/// counterpart.
///
/// Upstream fails on such a declaration in *any* package it analyses, every
/// transitive dependency included. guff fails on one in a package it analyses,
/// and on one in a direct import only when [`source_type_defs`] reads that
/// import — a broken declaration deeper in the graph, or in an import no
/// switch needed, goes unnoticed.
pub const DECL_NOT_FOUND: &str = "type '' is not defined";

/// `sumTypeFact`: the sum types a package declares and their variant names.
///
/// `location` is guff's addition. Upstream rebuilds it in the importer from
/// the imported object's position (`fset.Position(obj.Pos())`), which guff's
/// dependency objects do not carry; it is the declaring package's own
/// `sumTypeDecl.Location`, the same `file:line:col`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SumTypeFact {
    definitions: Vec<SumTypeDefinitionFact>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SumTypeDefinitionFact {
    type_name: String,
    variants: Vec<String>,
    location: String,
}

impl Fact for SumTypeFact {
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
        "sumTypeFact"
    }

    fn encode_payload(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.definitions
                .iter()
                .map(|d| {
                    serde_json::json!({
                        "type_name": d.type_name,
                        "variants": d.variants,
                        "location": d.location,
                    })
                })
                .collect(),
        )
    }
}

fn decode_sum_type_fact(payload: serde_json::Value) -> Option<Box<dyn Fact>> {
    let mut fact = SumTypeFact::default();
    for d in payload.as_array()? {
        fact.definitions.push(SumTypeDefinitionFact {
            type_name: d.get("type_name")?.as_str()?.to_string(),
            variants: d
                .get("variants")?
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect(),
            location: d.get("location")?.as_str()?.to_string(),
        });
    }
    Some(Box::new(fact))
}

fn ensure_fact_decoder() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        guff_analysis::register_fact_decoder("sumTypeFact", decode_sum_type_fact);
    });
}

#[derive(Clone)]
struct SumTypeDecl {
    type_name: String,
    /// `tspec.Pos()` (the type name) in the analysis `FileSet`. Invalid for a
    /// definition rebuilt from an imported fact, as upstream's `token.NoPos`.
    pos: Pos,
    /// `fset.Position(pos).String()` — the file by its full path; shortened
    /// only when the message is built.
    location: String,
}

#[derive(Clone)]
struct SumTypeDef {
    decl: SumTypeDecl,
    /// Underlying interface type id.
    iface: TypeId,
    /// Variant TypeName objects.
    variants: Vec<ObjectId>,
}

/// golangci-lint's `PathShortener` processor, which rewrites the text of every
/// issue: the working directory (symlinks resolved, `fsutils.Getwd`) followed
/// by `/`, then the bare directory, are removed wherever they appear. A path
/// that does not spell the resolved directory — `/tmp/...` on macOS, where
/// `/tmp` is a symlink — stays absolute, as it does upstream.
fn shorten_paths(text: &str) -> String {
    static WD: OnceLock<Option<String>> = OnceLock::new();
    let wd = WD.get_or_init(|| {
        let wd = std::env::current_dir().ok()?;
        let wd = std::fs::canonicalize(&wd).unwrap_or(wd);
        wd.to_str().map(str::to_string)
    });
    let Some(wd) = wd else {
        return text.to_string();
    };
    text.replace(&format!("{wd}/"), "").replace(wd.as_str(), "")
}

/// `findSumTypeDecls`: every `GenDecl` documented by a `//sumtype:decl` line,
/// at any depth (`ast.Inspect` — a type declared inside a function body is
/// found too, and then reported as not defined since the package scope does
/// not hold it). `Err` is upstream's `retErr`.
fn find_sumtype_decls(pass: &Pass<'_>) -> Result<Vec<SumTypeDecl>, String> {
    let mut decls = Vec::new();
    let mut ret_err = None;
    for file in pass.files() {
        if !file_source_contains(pass, file, DIRECTIVE) {
            continue;
        }
        let Some(re) = reparse_with_comments(pass, file) else {
            continue;
        };
        preorder_prune(NodeRef::File(&re.file), |n| {
            let NodeRef::GenDecl(gen) = n else {
                return true;
            };
            let Some(doc) = gen.doc.as_ref() else {
                return true;
            };
            // Upstream keeps the last TypeSpec of the declaration.
            let mut tspec = None;
            for spec in &gen.specs {
                if let Spec::TypeSpec(ts) = spec {
                    tspec = Some(ts);
                }
            }
            for line in &doc.list {
                if !line.text.starts_with("//sumtype:decl") {
                    continue;
                }
                let Some(ts) = tspec else {
                    ret_err = Some(guff_analysis::run_failure(DECL_NOT_FOUND));
                    return false;
                };
                let pos = re.rebase(ts.name.pos());
                decls.push(SumTypeDecl {
                    type_name: ts.name.name.clone(),
                    pos,
                    location: pass.fset().position(pos).to_string(),
                });
                break;
            }
            true
        });
    }
    match ret_err {
        Some(err) => Err(err),
        None => Ok(decls),
    }
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

fn package_scope(pass: &Pass<'_>, pkg: PackageId) -> Option<ScopeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    Some(artifacts.packages.get(pkg).scope())
}

/// `obj.Type()`.
fn object_type(pass: &Pass<'_>, obj: ObjectId) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    match artifacts.objects.get(obj) {
        ObjectData::TypeName(tn) => tn.typ(),
        ObjectData::Const(c) => Some(c.typ()),
        ObjectData::Var(v) => Some(v.typ()),
        ObjectData::Func(f) => f.typ(),
        _ => None,
    }
}

/// `newSumTypeDef`, against the package scope `scope`. `Ok(None)` is
/// upstream's `nil, nil` (no such object in the package scope), which
/// `findSumTypeDefs` turns into `notFoundError`.
fn new_sum_type_def(
    pass: &Pass<'_>,
    scope: ScopeId,
    decl: &SumTypeDecl,
) -> Result<Option<SumTypeDef>, String> {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Ok(None);
    };
    let Some(obj) = artifacts.scopes.get(scope).lookup_local(&decl.type_name) else {
        return Ok(None);
    };
    let Some(iface) = object_type(pass, obj).and_then(|t| underlying_iface(pass, t)) else {
        return Err(format!("type '{}' is not an interface", decl.type_name));
    };
    if !has_unexported_method(pass, iface) {
        return Err(format!(
            "interface '{}' is not sealed (sealing requires at least one unexported method)",
            decl.type_name
        ));
    }

    let mut variants = Vec::new();
    for name in artifacts.scopes.get(scope).names() {
        let Some(cand) = artifacts.scopes.get(scope).lookup_local(&name) else {
            continue;
        };
        let ObjectData::TypeName(ctn) = artifacts.objects.get(cand) else {
            continue;
        };
        let Some(cty) = ctn.typ() else {
            continue;
        };
        // `ty, ok := obj.Type().(*types.Named)`: an alias's type is a
        // `*types.Alias` (or, unmaterialized, the Named of another object).
        let TypeData::Named(named) = artifacts.types.get(cty) else {
            continue;
        };
        if named.obj() != cand {
            continue;
        }
        if types_identical(pass, cty.underlying(&artifacts.types), iface) {
            continue;
        }
        // Skip generic types.
        if named.type_params().is_some_and(|p| !p.list().is_empty()) {
            continue;
        }
        if type_implements(pass, cty, iface) {
            variants.push(cand);
        }
    }

    Ok(Some(SumTypeDef {
        decl: decl.clone(),
        iface,
        variants,
    }))
}

/// `factToTypeDefs`: the definitions an imported package's fact describes,
/// looked up in that package's scope as this package sees it.
fn fact_to_type_defs(pass: &Pass<'_>, pkg: PackageId, fact: &SumTypeFact) -> Vec<SumTypeDef> {
    let mut defs = Vec::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return defs;
    };
    let Some(scope) = package_scope(pass, pkg) else {
        return defs;
    };
    for definition in &fact.definitions {
        let Some(obj) = artifacts.scopes.get(scope).lookup_local(&definition.type_name) else {
            continue;
        };
        let Some(iface) = object_type(pass, obj).and_then(|t| underlying_iface(pass, t)) else {
            continue;
        };
        let location = if definition.location.is_empty() {
            format!(
                "{}.{}",
                artifacts.packages.get(pkg).path(),
                definition.type_name
            )
        } else {
            definition.location.clone()
        };
        let variants = definition
            .variants
            .iter()
            .filter_map(|name| artifacts.scopes.get(scope).lookup_local(name))
            .collect();
        defs.push(SumTypeDef {
            decl: SumTypeDecl {
                type_name: definition.type_name.clone(),
                pos: guff::position::NO_POS,
                location,
            },
            iface,
            variants,
        });
    }
    defs
}

/// The `//sumtype:decl` declarations of one source file read from disk:
/// `(type name, location)` in source order, and whether one of them has no
/// `TypeSpec` (upstream's `notFoundError`, which fails the run).
#[derive(Default)]
struct FileDecls {
    decls: Vec<(String, String)>,
    broken: bool,
}

/// [`FileDecls`] for `filename`, read once per file version.
fn load_file_decls(filename: &str) -> Arc<FileDecls> {
    type Cache = Mutex<HashMap<String, (Option<(u64, SystemTime)>, Arc<FileDecls>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let stamp = file_stamp(filename);
    if let Some((s, decls)) = cache.lock().unwrap().get(filename) {
        if *s == stamp {
            return decls.clone();
        }
    }
    let mut out = FileDecls::default();
    if let Ok(src) = std::fs::read(filename) {
        if src.windows(DIRECTIVE.len()).any(|w| w == DIRECTIVE) {
            let fset = FileSet::new();
            if let Ok(file) = parse_file(&fset, filename, &src, PARSE_COMMENTS) {
                preorder_prune(NodeRef::File(&file), |n| {
                    let NodeRef::GenDecl(gen) = n else {
                        return true;
                    };
                    let Some(doc) = gen.doc.as_ref() else {
                        return true;
                    };
                    let mut tspec = None;
                    for spec in &gen.specs {
                        if let Spec::TypeSpec(ts) = spec {
                            tspec = Some(ts);
                        }
                    }
                    for line in &doc.list {
                        if !line.text.as_bytes().starts_with(DIRECTIVE) {
                            continue;
                        }
                        let Some(ts) = tspec else {
                            out.broken = true;
                            return false;
                        };
                        out.decls.push((
                            ts.name.name.clone(),
                            fset.position(ts.name.pos()).to_string(),
                        ));
                        break;
                    }
                    true
                });
            }
        }
    }
    let out = Arc::new(out);
    cache
        .lock()
        .unwrap()
        .insert(filename.to_string(), (stamp, out.clone()));
    out
}

/// The sum types of a direct import this pass has no fact for, read from the
/// import's source files.
///
/// Upstream always has the fact: golangci-lint runs a fact-producing analyzer
/// over the syntax of every dependency, module cache included. guff analyses
/// the packages it was asked about (and, for some analyzers, their same-module
/// imports), so an import outside that set arrives without one. Its source is
/// still on disk — the driver lists each import's files — and the declaring
/// half of `run` needs nothing else: the `//sumtype:decl` docs and their
/// positions come from a reparse, and `newSumTypeDef` runs against the
/// import's scope as this package sees it, which is where `factToTypeDefs`
/// looks the fact's names up anyway. GOROOT is skipped: the standard library
/// declares no sum types.
///
/// `Err` when a file of the import has a `//sumtype:decl` with no `TypeSpec`:
/// upstream's analysis of that import fails, and with it the run.
fn source_type_defs(pass: &Pass<'_>, pkg: PackageId) -> Result<Vec<SumTypeDef>, String> {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return Ok(Vec::new());
    };
    let path = artifacts.packages.get(pkg).path();
    let Some(dep) = pass.pkg().imports.values().find(|p| p.pkg_path == path) else {
        return Ok(Vec::new());
    };
    let Some(scope) = package_scope(pass, pkg) else {
        return Ok(Vec::new());
    };
    let files = if dep.compiled_go_files.is_empty() {
        &dep.go_files
    } else {
        &dep.compiled_go_files
    };
    let mut defs = Vec::new();
    for file in files {
        let Some(filename) = file.to_str() else {
            continue;
        };
        if is_goroot_file(filename) || !file.is_absolute() {
            continue;
        }
        let decls = load_file_decls(filename);
        if decls.broken {
            return Err(guff_analysis::run_failure(DECL_NOT_FOUND));
        }
        for (type_name, location) in &decls.decls {
            let decl = SumTypeDecl {
                type_name: type_name.clone(),
                pos: guff::position::NO_POS,
                location: location.clone(),
            };
            // The import's own errors are diagnostics of its own pass, which
            // are never reported for a package that is not a root.
            if let Ok(Some(def)) = new_sum_type_def(pass, scope, &decl) {
                defs.push(def);
            }
        }
    }
    Ok(defs)
}

/// Whether some type switch of this pass asserts on a sealed interface no
/// definition in `defs` matches — the only switches an import's sum types
/// could still apply to, since every definition is a sealed interface.
fn needs_more_defs(pass: &Pass<'_>, defs: &[SumTypeDef]) -> bool {
    // Each distinct interface once: `has_unexported_method` copies the type
    // arena, and a package can hold hundreds of switches over `any`/`error`.
    let mut ifaces: Vec<TypeId> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::TypeSwitchStmt(swtch) = n {
                let ty = find_type_assert_expr(swtch).and_then(|e| type_of(pass, e));
                if let Some((ty, under)) = ty.and_then(|t| Some((t, underlying_iface(pass, t)?))) {
                    if !ifaces.contains(&under) && find_def(defs, pass, ty).is_none() {
                        ifaces.push(under);
                    }
                }
            }
            true
        });
    }
    ifaces.into_iter().any(|iface| has_unexported_method(pass, iface))
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
        shorten_paths(&format!(
            "exhaustiveness check failed for sum type \"{}\" (from {}): missing cases for {}",
            def.decl.type_name,
            def.decl.location,
            names.join(", ")
        )),
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

    let decls = find_sumtype_decls(pass)?;

    // `findSumTypeDefs`, its errors reported at the decl.
    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut defs = Vec::new();
    let own_scope = pass.type_pkg().and_then(|p| package_scope(pass, p));
    for decl in &decls {
        let def = match own_scope {
            Some(scope) => new_sum_type_def(pass, scope, decl),
            None => Ok(None),
        };
        match def {
            Ok(Some(def)) => defs.push(def),
            Ok(None) => pending.push((
                decl.pos.0 as u32,
                format!("type '{}' is not defined", decl.type_name),
            )),
            Err(msg) => pending.push((decl.pos.0 as u32, msg)),
        }
    }

    // Export facts so downstream packages can check exhaustiveness against
    // sum types defined here.
    let mut fact = SumTypeFact::default();
    for def in &defs {
        fact.definitions.push(SumTypeDefinitionFact {
            type_name: def.decl.type_name.clone(),
            variants: def.variants.iter().map(|&v| variant_name(pass, v)).collect(),
            location: def.decl.location.clone(),
        });
    }
    if let (false, Some(pkg)) = (fact.definitions.is_empty(), pass.type_pkg()) {
        pass.export_package_fact(pkg, Box::new(fact));
    }

    // Import sum type facts from (direct) dependencies.
    let imports: Vec<PackageId> = match (pass.pkg().type_artifacts.as_ref(), pass.type_pkg()) {
        (Some(artifacts), Some(pkg)) => artifacts.packages.get(pkg).imports().to_vec(),
        _ => Vec::new(),
    };
    let mut without_fact = Vec::new();
    for pkg in imports {
        let mut fact = SumTypeFact::default();
        if pass.import_package_fact(pkg, &mut fact) {
            defs.extend(fact_to_type_defs(pass, pkg, &fact));
        } else {
            without_fact.push(pkg);
        }
    }
    // guff's stand-in for the facts of imports it did not analyse
    // ([`source_type_defs`]), read only when a switch could use them.
    if !without_fact.is_empty() && needs_more_defs(pass, &defs) {
        for pkg in without_fact {
            defs.extend(source_type_defs(pass, pkg)?);
        }
    }

    // Check exhaustiveness for all type switches in this package.
    if !defs.is_empty() {
        for file in pass.files() {
            preorder(NodeRef::File(file), |n| {
                if let NodeRef::TypeSwitchStmt(swtch) = n {
                    check_switch(pass, &defs, swtch, &options, &mut pending);
                }
                true
            });
        }
    }

    // Upstream reports the definition errors first and the switches after,
    // and golangci's `sort_results` then orders them by position; guff's
    // output keeps report order, so report in position order.
    pending.sort_by_key(|&(pos, _)| pos);
    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| {
        ensure_fact_decoder();
        Analyzer {
            name: "gochecksumtype",
            doc: "check exhaustiveness of sum type switch statements",
            url: "https://github.com/alecthomas/go-check-sumtype",
            run: run as RunFn,
            run_despite_errors: false,
            requires: vec![inspect::analyzer()],
            fact_types: vec![FactTypeId::of::<SumTypeFact>()],
        }
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

    #[test]
    fn fact_round_trips_through_the_cache_codec() {
        ensure_fact_decoder();
        let fact = SumTypeFact {
            definitions: vec![SumTypeDefinitionFact {
                type_name: "First".into(),
                variants: vec!["FirstA".into(), "FirstB".into()],
                location: "/m/types.go:5:6".into(),
            }],
        };
        let back = guff_analysis::decode_fact("sumTypeFact", fact.encode_payload())
            .expect("decodes");
        assert_eq!(back.as_any().downcast_ref::<SumTypeFact>(), Some(&fact));
    }
}
