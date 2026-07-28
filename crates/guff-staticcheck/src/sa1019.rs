//! SA1019 — using a deprecated function, variable, constant or field.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1019`.

use std::sync::OnceLock;

use guff::ast::{CompositeLit, Expr, ImportSpec, SelectorExpr};
use guff::node_mask;
use guff::walk::{NodeMask, NodeRef};
use guff_analysis::code::{
    knowledge_selector_name, object_pkg_path, stdlib_version, version_compare,
};
use guff_analysis::passes::facts::deprecated;
use guff_analysis::passes::facts::generated;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, DeprecatedResult, IsDeprecated, RunError, RunFn, Pass};
use guff_types::arena::{ObjectData, TypeData};

use crate::stdlib_deprecations::{
    stdlib_deprecations, Deprecation, DEPRECATED_NEVER_USE, DEPRECATED_USE_NO_LONGER,
};

fn related_pkg_path(pass: &Pass<'_>, path: &str) -> bool {
    let cur = pass.pkg().pkg_path.as_str();
    path == cur
        || cur.strip_suffix("_test") == Some(path)
        || cur.strip_suffix(".test") == Some(path)
        || cur.strip_suffix(".test") == Some(path.strip_suffix("_test").unwrap_or(path))
}

fn is_stdlib_path(path: &str) -> bool {
    !path.contains('.')
}

fn format_go_version(s: &str) -> String {
    format!("Go {}", s.strip_prefix("go").unwrap_or(s))
}

fn deprecation_message(name: &str, depr: &IsDeprecated, std: Option<&Deprecation>) -> Option<String> {
    let std = std?;
    Some(match std.alternative_available_since {
        DEPRECATED_NEVER_USE => format!(
            "{name} has been deprecated since {} because it shouldn't be used: {}",
            format_go_version(std.deprecated_since),
            depr.msg
        ),
        v if v == std.deprecated_since || v == DEPRECATED_USE_NO_LONGER => format!(
            "{name} has been deprecated since {}: {}",
            format_go_version(std.deprecated_since),
            depr.msg
        ),
        alt => format!(
            "{name} has been deprecated since {} and an alternative has been available since {}: {}",
            format_go_version(std.deprecated_since),
            format_go_version(alt),
            depr.msg
        ),
    })
}

fn handle_deprecation(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    depr: &IsDeprecated,
    deprecated_name: &str,
    pkg_path: &str,
    pos: u32,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Option<String> {
    let table = stdlib_deprecations();
    let std = table.get(deprecated_name);
    if std.is_none() && is_stdlib_path(pkg_path) {
        return None;
    }
    if let Some(std) = std {
        if version_compare(&stdlib_version(pass, pos), std.deprecated_since) < 0 {
            return None;
        }
    }
    if current_fn.is_some_and(|f| {
        // Deprecated functions may use deprecated symbols.
        deprs.objects.contains_key(&f)
    }) {
        return None;
    }
    if let Some(std) = std {
        deprecation_message(deprecated_name, depr, Some(std))
    } else {
        Some(format!("{deprecated_name} is deprecated: {}", depr.msg))
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let mut deprs = pass
        .result_of::<DeprecatedResult>(deprecated::analyzer())
        .cloned()
        .unwrap_or_default();

    for fact in pass.all_object_facts() {
        if let Some(d) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            deprs.objects.insert(fact.object, d.clone());
        }
    }
    for fact in pass.all_package_facts() {
        if let Some(d) = fact.fact.as_any().downcast_ref::<IsDeprecated>() {
            deprs.packages.insert(fact.package, d.clone());
        }
    }

    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1019 requires inspect analyzer".to_string())?
        .clone();
    let _generated = pass.result_of::<generated::GeneratedResult>(generated::analyzer());

    let mut pending: Vec<(u32, String)> = Vec::new();
    let mut current_fn: Option<guff_types::arena::ObjectId> = None;

    const WANTED: NodeMask = node_mask!(
        CompositeLit,
        FuncDecl,
        ImportSpec,
        SelectorExpr,
    );
    inspect.preorder_typed(WANTED, pass.files(), |node| {
        match node {
            NodeRef::FuncDecl(f) => {
                current_fn = pass
                    .types_info()
                    .and_then(|info| info.defs.get(&f.name.id).and_then(|o| *o));
            }
            NodeRef::SelectorExpr(sel) => {
                if let Some((pos, msg)) = selector_diagnostic(pass, &deprs, sel, current_fn) {
                    pending.push((pos, msg));
                }
            }
            NodeRef::CompositeLit(lit) => {
                for (pos, msg) in struct_lit_diagnostics(pass, &deprs, lit, current_fn) {
                    pending.push((pos, msg));
                }
            }
            NodeRef::ImportSpec(spec) => {
                if let Some((pos, msg)) = import_diagnostic(pass, &deprs, spec) {
                    pending.push((pos, msg));
                }
            }
            _ => {}
        }
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn selector_diagnostic(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    sel: &SelectorExpr,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Option<(u32, String)> {
    let info = pass.types_info()?;
    let obj = info.uses.get(&sel.sel.id).copied()?;
    let pkg_path = object_pkg_path(pass, obj)?;
    if related_pkg_path(pass, &pkg_path) {
        return None;
    }
    let depr = deprs.objects.get(&obj)?;
    let name = knowledge_selector_name(pass, sel);
    let pos = sel.sel.name_pos.0 as u32;
    handle_deprecation(pass, deprs, depr, &name, &pkg_path, pos, current_fn).map(|msg| (pos, msg))
}

fn struct_lit_diagnostics(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    lit: &CompositeLit,
    current_fn: Option<guff_types::arena::ObjectId>,
) -> Vec<(u32, String)> {
    let Some(typ_expr) = lit.ty.as_ref() else {
        return Vec::new();
    };
    let info = match pass.types_info() {
        Some(i) => i,
        None => return Vec::new(),
    };
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return Vec::new(),
    };
    let Some(tv) = info.types.get(&typ_expr.id()) else {
        return Vec::new();
    };
    if !matches!(
        artifacts.types.get(tv.typ.underlying(&artifacts.types)),
        TypeData::Struct(_)
    ) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for elt in &lit.elts {
        let Expr::KeyValueExpr(kv) = elt else {
            continue;
        };
        let Expr::Ident(key) = &*kv.key else {
            continue;
        };
        let sel = SelectorExpr {
            x: typ_expr.clone(),
            sel: key.clone(),
            id: 0,
        };
        if let Some(d) = selector_diagnostic(pass, deprs, &sel, current_fn) {
            out.push(d);
        }
    }
    out
}

fn import_diagnostic(
    pass: &Pass<'_>,
    deprs: &DeprecatedResult,
    spec: &ImportSpec,
) -> Option<(u32, String)> {
    let info = pass.types_info()?;
    let imp_obj = if let Some(name) = &spec.name {
        info.defs.get(&name.id).and_then(|o| *o)
    } else {
        info.implicits.get(&spec.path.id).copied()
    }?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let ObjectData::PkgName(pn) = artifacts.objects.get(imp_obj) else {
        return None;
    };
    let imported = artifacts.packages.get(pn.imported());
    let path = imported.path();
    if related_pkg_path(pass, path) {
        return None;
    }
    let depr = deprs.packages.get(&pn.imported())?;
    let p = spec.path.value.trim_matches('"');
    let pos = spec.path.value_pos.0 as u32;
    handle_deprecation(pass, deprs, depr, p, path, pos, None).map(|msg| (pos, msg))
}

fn sa1019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1019",
        doc: "using a deprecated function, variable, constant or field",
        url: "https://staticcheck.dev/docs/checks/#SA1019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![
            inspect::analyzer(),
            deprecated::analyzer(),
            generated::analyzer(),
        ],
        fact_types: vec![],
    }
}

/// SA1019 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
