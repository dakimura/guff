//! `composites` — check for unkeyed composite literals of imported struct types.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/composite`.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{CompositeLit, Expr};
use guff::commentmap;
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;
use guff_types::lookup::deref;
use guff_types::named::named_obj;
use guff_types::typestring::type_string;

use crate::expreq::unparen;

fn trim_test_suffix(path: &str) -> &str {
    path.strip_suffix("_test")
        .or_else(|| path.strip_suffix(".test"))
        .unwrap_or(path)
}

fn is_whitelisted_type(name: &str) -> bool {
    static WHITELIST: OnceLock<HashSet<&'static str>> = OnceLock::new();
    WHITELIST
        .get_or_init(|| {
            [
                "image/color.Alpha16",
                "image/color.Alpha",
                "image/color.CMYK",
                "image/color.Gray16",
                "image/color.Gray",
                "image/color.NRGBA64",
                "image/color.NRGBA",
                "image/color.NYCbCrA",
                "image/color.RGBA64",
                "image/color.RGBA",
                "image/color.YCbCr",
                "image.Point",
                "image.Rectangle",
                "image.Uniform",
                "unicode.Range16",
                "unicode.Range32",
                "testing.InternalBenchmark",
                "testing.InternalExample",
                "testing.InternalTest",
                "testing.InternalFuzzTarget",
            ]
            .into_iter()
            .collect()
        })
        .contains(name)
}

fn type_name_same_package(type_name: &str, pass_path: &str) -> bool {
    let name = type_name.strip_prefix('*').unwrap_or(type_name);
    let name = name.split('[').next().unwrap_or(name);
    match name.rfind('.') {
        Some(dot) => trim_test_suffix(&name[..dot]) == pass_path,
        None => !name.contains('/'),
    }
}

fn is_same_package_type(pass: &Pass<'_>, typ: guff_types::TypeId) -> bool {
    let artifacts = match pass.pkg().type_artifacts.as_ref() {
        Some(a) => a,
        None => return false,
    };
    let types = &artifacts.types;
    // Prefer go/types package path; fall back to loader pkg_path when the
    // type-checker package path is missing/empty (common for `.test` mains
    // and some hybrid imports).
    let pass_path = {
        let from_types = pass
            .type_pkg()
            .map(|pid| artifacts.packages.get(pid).path().to_string())
            .filter(|p| !p.is_empty());
        let raw = from_types.unwrap_or_else(|| pass.pkg().pkg_path.clone());
        // `promql_test` / `promql.test` → `promql` (upstream composite local-type rule).
        trim_test_suffix(raw.as_str()).to_string()
    };
    // Upstream's `isLocalType` opens with `types.Unalias(typ)` — the comment on
    // its `Obj()` arm reads "aliases were removed already". Without it a local
    // alias (`type basic = BasicAuth`, which is how a table-driven test keeps
    // its literals short) is neither a Struct, a Pointer nor a Named, so it
    // fell through to "not local" and every `basic{...}` in the file was a
    // finding. gitea `modules/auth/httpauth/httpauth_test.go` writes six.
    let typ = guff_types::alias::unalias_readonly(types, typ);
    match types.get(typ) {
        TypeData::Struct(_) => true,
        TypeData::Pointer(p) => is_same_package_type(pass, p.elem()),
        TypeData::Named(_) => {
            let obj = named_obj(types, typ);
            if let Some(obj_pkg) = obj.pkg(&artifacts.objects) {
                let obj_path = trim_test_suffix(artifacts.packages.get(obj_pkg).path());
                if obj_path == pass_path {
                    return true;
                }
            }
            let type_name = type_string(
                types,
                &artifacts.objects,
                &artifacts.packages,
                typ,
                None,
            );
            type_name_same_package(&type_name, &pass_path)
        }
        TypeData::TypeParam(_) => true,
        _ => false,
    }
}

fn struct_type(types: &guff_types::arena::TypeArena, typ: guff_types::TypeId) -> Option<guff_types::TypeId> {
    let (elem, _) = deref(types, typ);
    let u = elem.underlying(types);
    match types.get(u) {
        TypeData::Struct(_) => Some(u),
        _ => None,
    }
}

fn has_keyed_element(lit: &CompositeLit) -> bool {
    lit.elts.iter().any(|e| matches!(unparen(e), Expr::KeyValueExpr(_)))
}

fn check_literal(pass: &Pass<'_>, lit: &CompositeLit) -> Option<String> {
    if lit.elts.is_empty() || has_keyed_element(lit) {
        return None;
    }
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let typ = info.types.get(&lit.id)?.typ;
    if !guff_types::predicates::is_valid(&artifacts.types, typ) {
        return None;
    }
    if is_same_package_type(pass, typ) {
        return None;
    }
    let type_name = type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    if is_whitelisted_type(&type_name) {
        return None;
    }
    if struct_type(&artifacts.types, typ).is_none() {
        return None;
    }
    Some(format!("{type_name} struct literal uses unkeyed fields"))
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "composites requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder_typed(node_mask!(CompositeLit), pass.files(), |n| {
        let NodeRef::CompositeLit(lit) = n else {
            return;
        };
        if let Some(message) = check_literal(pass, lit) {
            // Upstream reports at cl.Pos() (the literal's type, when it has
            // one), not at its opening brace.
            pending.push((commentmap::node_pos(NodeRef::CompositeLit(lit)).0 as u32, message));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "composites",
        doc: "check for unkeyed composite literals of struct types from other packages",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/composite",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
