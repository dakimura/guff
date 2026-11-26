//! Port of [`github.com/blizzy78/varnamelen`](https://github.com/blizzy78/varnamelen)
//! (golangci-lint wrapper in `pkg/golinters/varnamelen`).
//!
//! Checks that the length of a variable / constant / parameter name matches
//! its usage scope (line distance). Defaults match upstream:
//! `max-distance=5`, `min-name-length=3`; receivers / named returns / type
//! parameters are off unless enabled via settings.
//!
//! DEFERRED: import-alias-aware `shortTypeName` for `ignore-decls` / conventional
//! decls (we use `typestring::type_string`); type-switch assign special-case
//! typ `"<type-switched>"` for ignore matching.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, CompositeLit, Expr, FieldList, FuncType, Ident, Spec,
};
use guff::position::FileSet;
use guff::token::Token;
use guff::walk::{preorder, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectId, TypeId};
use guff_types::typestring::type_string;

use crate::options::VarnamelenOptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeclKind {
    Variable,
    Constant,
    Parameter,
    NamedReturn,
    Receiver,
    TypeParam,
}

#[derive(Debug, Clone)]
struct DeclInfo {
    name: String,
    typ: String,
    kind: DeclKind,
    report_pos: u32,
    decl_line: i64,
    /// Present when declared via `:=` / `=` AssignStmt (for `ok` heuristics).
    assign_ok: Option<AssignOkKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignOkKind {
    TypeAssert,
    MapIndex,
    ChanRecv,
}

#[derive(Debug, Clone)]
struct IdentDecl {
    name: String,
    constant: bool,
    typ: String,
}

fn parse_ident_declaration(decl: &str) -> Option<IdentDecl> {
    let decl = decl.trim();
    if let Some(name) = decl.strip_prefix("const ") {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        return Some(IdentDecl {
            name: name.to_string(),
            constant: true,
            typ: String::new(),
        });
    }
    let mut parts = decl.splitn(2, ' ');
    let name = parts.next()?.trim();
    let typ = parts.next()?.trim();
    if name.is_empty() || typ.is_empty() {
        return None;
    }
    Some(IdentDecl {
        name: name.to_string(),
        constant: false,
        typ: typ.to_string(),
    })
}

fn conventional_decls() -> &'static [IdentDecl] {
    static D: OnceLock<Vec<IdentDecl>> = OnceLock::new();
    D.get_or_init(|| {
        [
            "ctx context.Context",
            "b *testing.B",
            "f *testing.F",
            "m *testing.M",
            "pb *testing.PB",
            "t *testing.T",
            "tb testing.TB",
        ]
        .into_iter()
        .filter_map(parse_ident_declaration)
        .collect()
    })
}

fn name_len(name: &str) -> usize {
    name.chars().count()
}

fn check_name_and_distance(name: &str, dist: i64, opts: &VarnamelenOptions) -> bool {
    if name_len(name) >= opts.min_name_length {
        return true;
    }
    dist <= opts.max_distance as i64
}

fn type_str(pass: &Pass<'_>, typ: TypeId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    )
}

fn obj_type_str(pass: &Pass<'_>, obj: ObjectId) -> String {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return String::new();
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return String::new();
    };
    type_str(pass, typ)
}

fn line_of(fset: &FileSet, pos: guff::position::Pos) -> i64 {
    fset.position(pos).line
}

fn assign_ok_kind(assign: &AssignStmt, name: &str) -> Option<AssignOkKind> {
    if name != "ok" {
        return None;
    }
    if assign.lhs.len() != 2 {
        return None;
    }
    let Expr::Ident(ok_ident) = &assign.lhs[1] else {
        return None;
    };
    if ok_ident.name != "ok" {
        return None;
    }
    if assign.rhs.len() != 1 {
        return None;
    }
    match &assign.rhs[0] {
        Expr::TypeAssertExpr(_) => Some(AssignOkKind::TypeAssert),
        Expr::IndexExpr(_) | Expr::IndexListExpr(_) => Some(AssignOkKind::MapIndex),
        Expr::UnaryExpr(u) if u.op == Token::ARROW => Some(AssignOkKind::ChanRecv),
        _ => None,
    }
}

fn is_composite_lit_key(ident: &Ident, composite_lits: &[&CompositeLit]) -> bool {
    for cl in composite_lits {
        if matches!(cl.ty.as_deref(), Some(Expr::MapType(_))) {
            continue;
        }
        for elt in &cl.elts {
            let Expr::KeyValueExpr(kv) = elt else {
                continue;
            };
            if let Expr::Ident(key) = kv.key.as_ref() {
                if std::ptr::eq(key as *const Ident, ident as *const Ident) {
                    return true;
                }
                // Fallback: same node id when pointers differ across walks.
                if key.id != 0 && key.id == ident.id {
                    return true;
                }
            }
        }
    }
    false
}

fn match_decl(name: &str, typ: &str, constant: bool, decl: &IdentDecl) -> bool {
    if name != decl.name {
        return false;
    }
    if constant {
        return decl.constant;
    }
    if decl.constant {
        return false;
    }
    if typ.is_empty() {
        return false;
    }
    typ == decl.typ
}

fn matches_any(name: &str, typ: &str, constant: bool, decls: &[IdentDecl]) -> bool {
    decls
        .iter()
        .any(|d| match_decl(name, typ, constant, d))
}

fn register_field_names(
    pass: &Pass<'_>,
    fset: &FileSet,
    fields: &FieldList,
    kind: DeclKind,
    decls: &mut HashMap<ObjectId, DeclInfo>,
) {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return,
    };
    for field in &fields.list {
        let typ = field
            .ty
            .as_ref()
            .and_then(|ty| {
                let tid = info.types.get(&ty.id())?.typ;
                Some(type_str(pass, tid))
            })
            .unwrap_or_default();
        for name in &field.names {
            if name.name == "_" {
                continue;
            }
            let Some(obj) = info.defs.get(&name.id).and_then(|o| o.as_ref()).copied() else {
                continue;
            };
            decls.entry(obj).or_insert(DeclInfo {
                name: name.name.clone(),
                typ: typ.clone(),
                kind,
                report_pos: field.pos().0 as u32,
                decl_line: line_of(fset, field.pos()),
                assign_ok: None,
            });
        }
    }
}

fn register_func_type(
    pass: &Pass<'_>,
    fset: &FileSet,
    ft: &FuncType,
    opts: &VarnamelenOptions,
    decls: &mut HashMap<ObjectId, DeclInfo>,
) {
    if let Some(params) = &ft.params {
        register_field_names(pass, fset, params, DeclKind::Parameter, decls);
    }
    if opts.check_return {
        if let Some(results) = &ft.results {
            register_field_names(pass, fset, results, DeclKind::NamedReturn, decls);
        }
    }
    if opts.check_type_param {
        if let Some(tparams) = &ft.type_params {
            register_field_names(pass, fset, tparams, DeclKind::TypeParam, decls);
        }
    }
}

fn collect_decls(
    pass: &Pass<'_>,
    fset: &FileSet,
    opts: &VarnamelenOptions,
) -> HashMap<ObjectId, DeclInfo> {
    let mut decls = HashMap::new();
    let Some(info) = pass.types_info() else {
        return decls;
    };

    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::FuncDecl(fd) => {
                    if opts.check_receiver {
                        if let Some(recv) = &fd.recv {
                            register_field_names(
                                pass,
                                fset,
                                recv,
                                DeclKind::Receiver,
                                &mut decls,
                            );
                        }
                    }
                    register_func_type(pass, fset, &fd.ty, opts, &mut decls);
                }
                NodeRef::FuncLit(fl) => {
                    register_func_type(pass, fset, &fl.ty, opts, &mut decls);
                }
                NodeRef::GenDecl(gd) => {
                    let is_const = gd.tok == Some(Token::CONST);
                    let is_var = gd.tok == Some(Token::VAR);
                    if !is_const && !is_var {
                        // TypeSpec type parameters.
                        if opts.check_type_param {
                            for spec in &gd.specs {
                                if let Spec::TypeSpec(ts) = spec {
                                    if let Some(tp) = &ts.type_params {
                                        register_field_names(
                                            pass,
                                            fset,
                                            tp,
                                            DeclKind::TypeParam,
                                            &mut decls,
                                        );
                                    }
                                }
                            }
                        }
                        return true;
                    }
                    for spec in &gd.specs {
                        let Spec::ValueSpec(vs) = spec else {
                            continue;
                        };
                        for name in &vs.names {
                            if name.name == "_" {
                                continue;
                            }
                            let Some(obj) =
                                info.defs.get(&name.id).and_then(|o| o.as_ref()).copied()
                            else {
                                continue;
                            };
                            let kind = if is_const {
                                DeclKind::Constant
                            } else {
                                DeclKind::Variable
                            };
                            let typ = if is_const {
                                String::new()
                            } else {
                                obj_type_str(pass, obj)
                            };
                            decls.entry(obj).or_insert(DeclInfo {
                                name: name.name.clone(),
                                typ,
                                kind,
                                report_pos: name.pos().0 as u32,
                                decl_line: line_of(fset, name.pos()),
                                assign_ok: None,
                            });
                        }
                    }
                }
                NodeRef::AssignStmt(assign) => {
                    // Only short declarations introduce defs (:= / range :=).
                    for lhs in &assign.lhs {
                        let Expr::Ident(ident) = lhs else {
                            continue;
                        };
                        if ident.name == "_" {
                            continue;
                        }
                        let Some(obj) =
                            info.defs.get(&ident.id).and_then(|o| o.as_ref()).copied()
                        else {
                            continue;
                        };
                        let typ = obj_type_str(pass, obj);
                        let ok = assign_ok_kind(assign, &ident.name);
                        decls.entry(obj).or_insert(DeclInfo {
                            name: ident.name.clone(),
                            typ,
                            kind: DeclKind::Variable,
                            // `pass.Reportf(variable.assign.Pos(), …)` — the
                            // AssignStmt's first LHS operand, i.e. the name
                            // being complained about, not the `:=` after it.
                            report_pos: assign
                                .lhs
                                .first()
                                .map(|e| e.pos().0 as u32)
                                .unwrap_or(assign.tok_pos.0 as u32),
                            decl_line: line_of(fset, assign.tok_pos),
                            assign_ok: ok,
                        });
                    }
                }
                _ => {}
            }
            true
        });
    }
    decls
}

fn collect_composite_lits<'a>(pass: &'a Pass<'_>) -> Vec<&'a CompositeLit> {
    let mut out = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::CompositeLit(cl) = n {
                out.push(cl);
            }
            true
        });
    }
    out
}

fn compute_distances(
    pass: &Pass<'_>,
    fset: &FileSet,
    decls: &HashMap<ObjectId, DeclInfo>,
    composite_lits: &[&CompositeLit],
) -> HashMap<ObjectId, i64> {
    let mut dist: HashMap<ObjectId, i64> = HashMap::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            let NodeRef::Ident(ident) = n else {
                return true;
            };
            if is_composite_lit_key(ident, composite_lits) {
                return true;
            }
            let Some(obj) = object_of(pass, ident) else {
                return true;
            };
            let Some(decl) = decls.get(&obj) else {
                return true;
            };
            let use_line = line_of(fset, ident.pos());
            let d = (use_line - decl.decl_line).max(0);
            let e = dist.entry(obj).or_insert(0);
            if d > *e {
                *e = d;
            }
            true
        });
    }
    dist
}

fn should_ignore_ok(decl: &DeclInfo, opts: &VarnamelenOptions) -> bool {
    match decl.assign_ok {
        Some(AssignOkKind::TypeAssert) => opts.ignore_type_assert_ok,
        Some(AssignOkKind::MapIndex) => opts.ignore_map_index_ok,
        Some(AssignOkKind::ChanRecv) => opts.ignore_chan_recv_ok,
        None => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "varnamelen requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<VarnamelenOptions>("varnamelen")
        .cloned()
        .unwrap_or_default();

    let ignore_names: HashSet<String> = opts.ignore_names.iter().cloned().collect();
    let ignore_decls: Vec<IdentDecl> = opts
        .ignore_decls
        .iter()
        .filter_map(|s| parse_ident_declaration(s))
        .collect();

    let fset = pass.fset().clone();
    let decls = collect_decls(pass, &fset, &opts);
    let composite_lits = collect_composite_lits(pass);
    let distances = compute_distances(pass, &fset, &decls, &composite_lits);

    let mut pending: Vec<(u32, String)> = Vec::new();
    for (obj, decl) in &decls {
        let dist = distances.get(obj).copied().unwrap_or(0);
        if ignore_names.contains(&decl.name) {
            continue;
        }
        let is_const = decl.kind == DeclKind::Constant;
        if matches_any(&decl.name, &decl.typ, is_const, &ignore_decls) {
            continue;
        }
        if matches!(
            decl.kind,
            DeclKind::Variable | DeclKind::Parameter
        ) && matches_any(&decl.name, &decl.typ, false, conventional_decls())
        {
            continue;
        }
        if should_ignore_ok(decl, &opts) {
            continue;
        }
        if check_name_and_distance(&decl.name, dist, &opts) {
            continue;
        }
        let msg = match decl.kind {
            DeclKind::Variable => {
                format!(
                    "variable name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
            DeclKind::Constant => {
                format!(
                    "constant name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
            DeclKind::Parameter => {
                format!(
                    "parameter name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
            DeclKind::NamedReturn => {
                format!(
                    "return value name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
            DeclKind::Receiver => {
                format!(
                    "method receiver name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
            DeclKind::TypeParam => {
                format!(
                    "type parameter name '{}' is too short for the scope of its usage",
                    decl.name
                )
            }
        };
        pending.push((decl.report_pos, msg));
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "varnamelen",
        doc: "checks that the length of a variable's name matches its scope",
        url: "https://github.com/blizzy78/varnamelen",
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
    fn parse_decl_forms() {
        let v = parse_ident_declaration("ctx context.Context").unwrap();
        assert_eq!(v.name, "ctx");
        assert_eq!(v.typ, "context.Context");
        assert!(!v.constant);

        let c = parse_ident_declaration("const C").unwrap();
        assert_eq!(c.name, "C");
        assert!(c.constant);
    }

    #[test]
    fn name_and_distance_defaults() {
        let opts = VarnamelenOptions::default();
        assert_eq!(opts.max_distance, 5);
        assert_eq!(opts.min_name_length, 3);
        assert!(check_name_and_distance("x", 5, &opts));
        assert!(!check_name_and_distance("x", 6, &opts));
        assert!(check_name_and_distance("foo", 100, &opts));
    }
}
