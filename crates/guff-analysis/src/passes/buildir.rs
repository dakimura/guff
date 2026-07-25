//! The `buildir` analyzer — construct SSA/IR for dependent passes.
//!
//! Port of `honnef.co/go/tools/internal/passes/buildir`.

use std::sync::OnceLock;

use std::sync::Arc;

use guff_ssa::ids::FuncId;
use guff_ssa::member::MemberData;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ids::PackageId;
use guff_ssa::ssautil::build_package_for_analysis;
use guff_types::PackageId as TypePackageId;

use crate::analyzer::{AnalysisResult, Analyzer, RunError, RunFn};
use crate::pass::Pass;
use crate::passes::inspect;

/// SSA intermediate representation for the current package.
///
/// Port of `buildir.IR`.
#[derive(Clone)]
pub struct BuildIrResult {
    pub prog: Arc<Program>,
    pub pkg: PackageId,
    pub type_pkg: TypePackageId,
    pub src_funcs: Vec<FuncId>,
}

// SSA results are immutable after construction. The type-checker arenas behind
// `Program` are not formally proven `Sync`, but analysis only reads them.
unsafe impl Send for BuildIrResult {}
unsafe impl Sync for BuildIrResult {}

fn collect_src_funcs(prog: &Program, pkg: PackageId) -> Vec<FuncId> {
    let mut funcs = Vec::new();
    let ssa_pkg = prog.packages.get(pkg);
    for member in ssa_pkg.members.values() {
        if let MemberData::Function(fid) = member {
            funcs.push(*fid);
            collect_anon_funcs(prog, *fid, &mut funcs);
        }
    }
    funcs
}

fn collect_anon_funcs(prog: &Program, fid: FuncId, out: &mut Vec<FuncId>) {
    let anon = prog.functions.get(fid).anon_funcs.clone();
    for child in anon {
        out.push(child);
        collect_anon_funcs(prog, child, out);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if pass.pkg().ill_typed {
        return Err("buildir: package is ill-typed".into());
    }
    let artifacts = pass
        .pkg()
        .type_artifacts
        .as_ref()
        .ok_or_else(|| "buildir requires type artifacts (load with types mode)".to_string())?
        .snapshot_for_ssa();
    let fset = pass.fset().clone();
    let timing = std::env::var_os("GUFF_DEBUG_CACHE").is_some();
    let t0 = timing.then(std::time::Instant::now);
    // GLOBAL_DEBUG emits DebugRefs needed by ValueForExpr (SA4006/SA4031).
    // When those checks are off, skip the extra IR — settings default to true
    // so unset bags (unit tests) stay conservative.
    let mode = if pass
        .settings::<bool>("buildir_global_debug")
        .copied()
        .unwrap_or(true)
    {
        BuilderMode::GLOBAL_DEBUG
    } else {
        BuilderMode::default()
    };
    let built = build_package_for_analysis(artifacts, pass.files(), fset, mode)
        .map_err(|e| format!("buildir: {e}"))?;
    if let Some(t0) = t0 {
        let el = t0.elapsed().as_secs_f64();
        if el > 1.0 {
            eprintln!(
                "guff: buildir {} {:.2}s ({} files)",
                pass.pkg().pkg_path,
                el,
                pass.files().len(),
            );
        }
    }

    let src_funcs = collect_src_funcs(&built.prog, built.pkg);
    Ok(Some(Box::new(BuildIrResult {
        prog: Arc::new(built.prog),
        pkg: built.pkg,
        type_pkg: built.type_pkg,
        src_funcs,
    })))
}

fn buildir_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "buildir",
        doc: "build SSA IR for later passes",
        url: "https://staticcheck.dev/docs/checks/",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// The `buildir` analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(buildir_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
    use guff_types::default_sizes;
    use guff::position::FileSet;

    use super::*;
    use crate::pass::PassInput;
    use crate::Pass;

    fn typechecked_package() -> Package {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../guff-packages/tests/testdata/typecheck/valid");
        let mut pkg = Package {
            id: "example.com/valid".into(),
            pkg_path: "example.com/valid".into(),
            dir: dir.clone(),
            compiled_go_files: vec![dir.join("main.go")],
            ..Package::default()
        };
        let fset = FileSet::new();
        typecheck_package(
            &mut pkg,
            &fset,
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        pkg
    }

    #[test]
    fn buildir_validates() {
        assert!(crate::validate::validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn buildir_produces_src_funcs() {
        let pkg = typechecked_package();
        assert!(!pkg.ill_typed, "{:?}", pkg.errors);
        let fset = pkg.fset.clone().expect("fset");
        let mut diags = Vec::new();
        let mut facts = crate::facts::FactStore::default();
        let mut pass = PassInput {
            analyzer: analyzer(),
            fset: &fset,
            files: &pkg.syntax,
            pkg: &pkg,
            types_info: pkg.types_info.as_deref(),
            types_sizes: default_sizes(),
            diagnostics: &mut diags,
            result_of: std::collections::HashMap::new(),
            facts: &mut facts,
            settings: std::sync::Arc::new(crate::SettingsBag::default()),
        }
        .build();

        let result = run(&mut pass).expect("buildir run");
        let ir = result
            .unwrap()
            .downcast::<BuildIrResult>()
            .expect("BuildIrResult");
        assert!(
            ir.src_funcs.iter().any(|fid| ir.prog.functions.get(*fid).name == "main"),
            "expected main in src_funcs, got {:?}",
            ir.src_funcs
                .iter()
                .map(|fid| ir.prog.functions.get(*fid).name.as_str())
                .collect::<Vec<_>>()
        );
    }
}
