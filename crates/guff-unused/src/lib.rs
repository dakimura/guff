//! guff-unused — unused package-level declarations.
//!
//! Simplified port of [`honnef.co/go/tools/unused`](https://pkg.go.dev/honnef.co/go/tools/unused)
//! for single-package analysis.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, GenDecl, Ident, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::facts::generated;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectId;

fn is_exported(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
}

/// Receiver type name for `T`, `*T`, or indexed `T[...]` / `*T[...]`.
fn recv_type_ident(ty: &Expr) -> Option<&Ident> {
    match ty {
        Expr::Ident(id) => Some(id),
        Expr::StarExpr(s) => recv_type_ident(&s.x),
        Expr::IndexExpr(i) => recv_type_ident(&i.x),
        Expr::IndexListExpr(i) => recv_type_ident(&i.x),
        Expr::ParenExpr(p) => recv_type_ident(&p.x),
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let info = match pass.types_info() {
        Some(i) => i,
        None => return Ok(None),
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return Ok(None),
    };

    let pkg_name = pass.pkg().name.as_str();
    let mut candidates = HashSet::new();
    let mut roots = HashSet::new();
    let mut const_groups: Vec<Vec<ObjectId>> = Vec::new();
    let mut method_recv_type: HashMap<ObjectId, ObjectId> = HashMap::new();
    let mut method_display: HashMap<ObjectId, String> = HashMap::new();
    let mut iface_method_names: HashSet<String> = HashSet::new();

    for file in pass.files() {
        if is_generated_at(pass, file.file_start.0 as u32) {
            continue;
        }
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(f) => {
                    let Some(Some(obj)) = info.defs.get(&f.name.id) else {
                        continue;
                    };
                    if let Some(recv) = &f.recv {
                        if let Some(field) = recv.list.first() {
                            if let Some(ty) = field.ty.as_ref() {
                                if let Some(type_ident) = recv_type_ident(ty) {
                                    // Receiver type Idents are usually uses, not defs.
                                    let type_obj = info
                                        .uses
                                        .get(&type_ident.id)
                                        .copied()
                                        .or_else(|| {
                                            info.defs.get(&type_ident.id).and_then(|d| *d)
                                        });
                                    if let Some(type_obj) = type_obj {
                                        method_recv_type.insert(*obj, type_obj);
                                    }
                                    let ptr = matches!(ty, Expr::StarExpr(_));
                                    let qual = if ptr {
                                        format!("(*{}).", type_ident.name)
                                    } else {
                                        format!("({}).", type_ident.name)
                                    };
                                    method_display
                                        .insert(*obj, format!("{qual}{}", f.name.name));
                                }
                            }
                        }
                        if is_exported(&f.name.name) {
                            roots.insert(*obj);
                        } else {
                            candidates.insert(*obj);
                        }
                        continue;
                    }
                    if f.name.name == "init" || is_exported(&f.name.name) {
                        roots.insert(*obj);
                        continue;
                    }
                    if pkg_name == "main" && f.name.name == "main" {
                        roots.insert(*obj);
                        continue;
                    }
                    candidates.insert(*obj);
                }
                Decl::GenDecl(GenDecl { tok, specs, .. }) => {
                    let kind = matches!(tok, Some(Token::VAR | Token::CONST | Token::TYPE));
                    if !kind {
                        continue;
                    }
                    let mut decl_group = Vec::new();
                    for spec in specs {
                        match spec {
                            Spec::TypeSpec(TypeSpec { name, ty, .. }) => {
                                let Some(Some(obj)) = info.defs.get(&name.id) else {
                                    continue;
                                };
                                if is_exported(&name.name) {
                                    roots.insert(*obj);
                                } else {
                                    candidates.insert(*obj);
                                }
                                if let Expr::InterfaceType(iface) = ty {
                                    for field in &iface.methods.list {
                                        for n in &field.names {
                                            iface_method_names.insert(n.name.clone());
                                        }
                                    }
                                }
                            }
                            Spec::ValueSpec(ValueSpec { names, .. }) => {
                                for id in names {
                                    if id.name == "_" {
                                        continue;
                                    }
                                    let Some(Some(obj)) = info.defs.get(&id.id) else {
                                        continue;
                                    };
                                    if is_exported(&id.name) {
                                        roots.insert(*obj);
                                    } else {
                                        candidates.insert(*obj);
                                        decl_group.push(*obj);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if *tok == Some(Token::CONST) && decl_group.len() > 1 {
                        const_groups.push(decl_group);
                    }
                }
                _ => {}
            }
        }
    }

    let mut used = roots;
    for obj in info.uses.values() {
        used.insert(*obj);
    }

    for group in const_groups {
        if group.iter().any(|obj| used.contains(obj)) {
            for obj in group {
                used.insert(obj);
            }
        }
    }

    // Methods that implement a package interface are used when their receiver
    // type is used (even if never called by name). Compare by type *name* so
    // hybrid typecheck ObjectId identity mismatches don't miss the link.
    let used_type_names: HashSet<String> = used
        .iter()
        .map(|obj| obj.name(&artifacts.objects).to_string())
        .collect();
    for (method, recv_ty) in &method_recv_type {
        if !candidates.contains(method) {
            continue;
        }
        let recv_name = recv_ty.name(&artifacts.objects);
        if !used_type_names.contains(recv_name) {
            continue;
        }
        let name = method.name(&artifacts.objects);
        if iface_method_names.contains(name) {
            used.insert(*method);
        }
    }

    let mut pending = Vec::new();
    for obj in candidates {
        if used.contains(&obj) {
            continue;
        }
        let name = obj.name(&artifacts.objects);
        let pos = obj.pos(&artifacts.objects);
        let message = method_display
            .get(&obj)
            .cloned()
            .map(|d| format!("{d} is unused"))
            .unwrap_or_else(|| format!("{name} is unused"));
        pending.push((pos, message));
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "unused",
        doc: "check for unused package-level declarations",
        url: "https://pkg.go.dev/honnef.co/go/tools/unused",
        run: run as RunFn,
        // Partial types still let unused see live refs; skipping on
        // ill_typed packages drops real nolintlint hits (restic `sys` field).
        run_despite_errors: true,
        requires: vec![generated::analyzer()],
        fact_types: vec![],
    })
}

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![analyzer()]
}
