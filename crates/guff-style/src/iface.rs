//! Port of [`github.com/uudashr/iface`](https://github.com/uudashr/iface)
//! (golangci-lint wrapper in `pkg/golinters/iface`).
//!
//! Default enables only `identical` (golangci-lint compat). Additional checkers
//! (`unused`, …) via `linters.settings.iface.enable`.
//!
//! DEFERRED: `opaque` / `unexported` / `unusedmethod`; `//iface:ignore` directives;
//! unused SuggestedFix text edits; unused `settings.unused.exclude` package globs
//! beyond exact path match.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use guff_types::predicates::identical as types_identical;
use guff_types::{ObjectId, TypeData, TypeId};

use crate::options::IfaceOptions;

const CHECK_IDENTICAL: &str = "identical";
const CHECK_UNUSED: &str = "unused";

fn enabled_checks(opts: &IfaceOptions) -> HashSet<String> {
    if opts.enable.is_empty() {
        return HashSet::from([CHECK_IDENTICAL.to_string()]);
    }
    let mut out = HashSet::new();
    for name in &opts.enable {
        match name.as_str() {
            CHECK_IDENTICAL | CHECK_UNUSED => {
                out.insert(name.clone());
            }
            // Unknown / not-yet-ported checkers are ignored (golangci skips them).
            _ => {}
        }
    }
    out
}

struct IfaceDecl {
    name: String,
    pos: u32,
    /// Underlying interface type (for identical).
    iface_ty: TypeId,
    /// TypeName object (for unused).
    type_name: ObjectId,
}

fn collect_iface_decls(pass: &Pass<'_>) -> Vec<IfaceDecl> {
    let mut out = Vec::new();
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return out;
    };
    let Some(info) = pass.types_info() else {
        return out;
    };

    for file in pass.files() {
        for decl in &file.decls {
            let Decl::GenDecl(gen) = decl else {
                continue;
            };
            if gen.tok != Some(Token::TYPE) {
                continue;
            }
            for spec in &gen.specs {
                let Spec::TypeSpec(ts) = spec else {
                    continue;
                };
                if !matches!(ts.ty, Expr::InterfaceType(_)) {
                    continue;
                }
                let Some(obj) = info.defs.get(&ts.name.id).copied().flatten() else {
                    continue;
                };
                let ObjectData::TypeName(tn) = artifacts.objects.get(obj) else {
                    continue;
                };
                let Some(named_ty) = tn.typ() else {
                    continue;
                };
                let under = named_ty.underlying(&artifacts.types);
                if !matches!(artifacts.types.get(under), TypeData::Interface(_)) {
                    continue;
                }
                out.push(IfaceDecl {
                    name: ts.name.name.clone(),
                    pos: ts.name.pos().0 as u32,
                    iface_ty: under,
                    type_name: obj,
                });
            }
        }
    }
    out
}

fn check_identical(pass: &Pass<'_>, decls: &[IfaceDecl], pending: &mut Vec<(u32, String)>) {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return;
    };

    // name -> other names with identical method sets
    let mut identicals: HashMap<String, Vec<String>> = HashMap::new();
    for (i, a) in decls.iter().enumerate() {
        for b in decls.iter().skip(i + 1) {
            let mut types = artifacts.types.clone();
            if !types_identical(
                &mut types,
                &artifacts.objects,
                &artifacts.packages,
                a.iface_ty,
                b.iface_ty,
            ) {
                continue;
            }
            identicals
                .entry(a.name.clone())
                .or_default()
                .push(b.name.clone());
            identicals
                .entry(b.name.clone())
                .or_default()
                .push(a.name.clone());
        }
    }

    let pos_by_name: HashMap<&str, u32> = decls.iter().map(|d| (d.name.as_str(), d.pos)).collect();
    for (name, mut others) in identicals {
        others.sort();
        others.dedup();
        let other_names = others.join(", ");
        let Some(&pos) = pos_by_name.get(name.as_str()) else {
            continue;
        };
        pending.push((
            pos,
            format!(
                "identical: interface '{name}' contains identical methods or type constraints with another interface, causing redundancy (see: {other_names})"
            ),
        ));
    }
}

fn check_unused(
    pass: &Pass<'_>,
    decls: &[IfaceDecl],
    exclude_pkgs: &[String],
    pending: &mut Vec<(u32, String)>,
) {
    if !exclude_pkgs.is_empty() {
        let path = &pass.pkg().pkg_path;
        if exclude_pkgs.iter().any(|p| p == path) {
            return;
        }
    }

    let Some(info) = pass.types_info() else {
        return;
    };

    let mut unused: HashMap<ObjectId, &IfaceDecl> =
        decls.iter().map(|d| (d.type_name, d)).collect();

    for file in pass.files() {
        guff::walk::inspect(guff::walk::NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            let guff::walk::NodeRef::Ident(ident) = n else {
                return true;
            };
            let Some(&obj) = info.uses.get(&ident.id) else {
                return true;
            };
            unused.remove(&obj);
            true
        });
    }

    for decl in unused.values() {
        pending.push((
            decl.pos,
            format!(
                "unused: interface '{}' is declared but not used within the package",
                decl.name
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "iface requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<IfaceOptions>("iface")
        .cloned()
        .unwrap_or_default();
    let checks = enabled_checks(&opts);
    if checks.is_empty() {
        return Ok(None);
    }

    let decls = collect_iface_decls(pass);
    let mut pending = Vec::new();

    if checks.contains(CHECK_IDENTICAL) {
        check_identical(pass, &decls, &mut pending);
    }
    if checks.contains(CHECK_UNUSED) {
        check_unused(pass, &decls, &opts.unused_exclude, &mut pending);
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "iface",
        doc: "detects incorrect use of interfaces (interface pollution)",
        url: "https://github.com/uudashr/iface",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
