//! guff-unused — unused package-level declarations.
//!
//! Simplified port of [`honnef.co/go/tools/unused`](https://pkg.go.dev/honnef.co/go/tools/unused)
//! for single-package analysis.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, GenDecl, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff_analysis::code::is_generated_at;
use guff_analysis::passes::facts::generated;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn is_exported(name: &str) -> bool {
    name.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
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
    let mut const_groups: Vec<Vec<guff_types::arena::ObjectId>> = Vec::new();

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
                    if f.recv.is_some() {
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
                            Spec::TypeSpec(TypeSpec { name, .. }) => {
                                let Some(Some(obj)) = info.defs.get(&name.id) else {
                                    continue;
                                };
                                if is_exported(&name.name) {
                                    roots.insert(*obj);
                                } else {
                                    candidates.insert(*obj);
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

    let mut pending = Vec::new();
    for obj in candidates {
        if used.contains(&obj) {
            continue;
        }
        let name = obj.name(&artifacts.objects);
        let pos = obj.pos(&artifacts.objects);
        pending.push((pos, format!("{name} is unused")));
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
        run_despite_errors: false,
        requires: vec![generated::analyzer()],
        fact_types: vec![],
    })
}

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![analyzer()]
}
