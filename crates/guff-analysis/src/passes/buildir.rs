//! The `buildir` analyzer — construct SSA/IR for dependent passes.
//!
//! Port of `honnef.co/go/tools/internal/passes/buildir`.

use std::collections::HashSet;
use std::sync::OnceLock;

use std::sync::Arc;

use guff_ssa::ids::FuncId;
use guff_ssa::mode::BuilderMode;
use guff_ssa::program::Program;
use guff_ssa::ids::PackageId;
use guff_ssa::source::ExprValueIndex;
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
    /// Built on first use; only SA4006 / SA4031 resolve source expressions.
    expr_values: OnceLock<ExprValueIndex>,
    /// Built on first use — see [`Self::src_funcs_with_methods`].
    src_funcs_all: OnceLock<Vec<FuncId>>,
    /// Built on first use — see [`Self::call_target_names`].
    call_target_names: OnceLock<HashSet<String>>,
}

impl BuildIrResult {
    pub fn new(
        prog: Arc<Program>,
        pkg: PackageId,
        type_pkg: TypePackageId,
        src_funcs: Vec<FuncId>,
    ) -> Self {
        Self {
            prog,
            pkg,
            type_pkg,
            src_funcs,
            expr_values: OnceLock::new(),
            src_funcs_all: OnceLock::new(),
            call_target_names: OnceLock::new(),
        }
    }

    /// `SrcFuncs` as go/ssa's `buildssa` and honnef's `buildir` define it: every
    /// named function declared in the package, **methods included**.
    ///
    /// [`Self::src_funcs`] honours the `buildir_src_methods` setting, which
    /// guff-lint turns off outside contextcheck runs — not for correctness but
    /// because SA5011 over-reports once method bodies are visible (guff-ssa is a
    /// go/ssa port and has no σ-nodes, so SA5011's value-identity test matches
    /// across branches where honnef's SSI form would not; see
    /// `docs/COMPAT-HARDENING.md` §7). A check that needs upstream's SrcFuncs
    /// and does not have that precision gap should use this instead.
    ///
    /// No SSA is rebuilt: `prog` already holds every function in the package,
    /// so this is a filter over the arena the pass already owns.
    pub fn src_funcs_with_methods(&self) -> &[FuncId] {
        self.src_funcs_all
            .get_or_init(|| collect_src_funcs_with_methods(&self.prog, self.pkg))
    }

    /// Expression → SSA value index over [`Self::src_funcs`]. Shared by every
    /// analyzer that runs on this package, since the runner hands them all the
    /// same `Arc<BuildIrResult>`.
    pub fn expr_values(&self) -> &ExprValueIndex {
        self.expr_values
            .get_or_init(|| ExprValueIndex::build(&self.prog, &self.src_funcs))
    }

    /// The set of call-target names this package uses, built once on first ask.
    ///
    /// Twenty-five staticcheck analyzers hand [`crate::callcheck::run`] a rule
    /// table keyed by call-target name, and each of them walked every
    /// instruction of every function to find out that this package calls none
    /// of them. Most packages call none: SA1030's rules are all
    /// `strconv.Quote`-shaped, SA6000's are `regexp.Match`-shaped, and an
    /// average package touches neither. That walk was 0.32s of self CPU on
    /// prometheus `./...`, twenty-five times over the same instructions.
    ///
    /// The answer is the same for all of them, so the first analyzer to ask
    /// pays for it and the other twenty-four get a hash lookup per rule.
    /// Whoever does match still walks, and reports in the order it always did.
    ///
    /// `build` supplies the names — collecting them needs the type arenas and
    /// the call-target memo, which live above this pass. Same lifetime as the
    /// other lazy indices here: the runner hands every analyzer for a package
    /// the same `Arc<BuildIrResult>` and drops it when the last one is done.
    pub fn call_target_names(
        &self,
        build: impl FnOnce() -> HashSet<String>,
    ) -> &HashSet<String> {
        self.call_target_names.get_or_init(build)
    }
}

// SSA results are immutable after construction. The type-checker arenas behind
// `Program` are not formally proven `Sync`, but analysis only reads them.
unsafe impl Send for BuildIrResult {}
unsafe impl Sync for BuildIrResult {}

fn collect_src_funcs(prog: &Program, pkg: PackageId, include_methods: bool) -> Vec<FuncId> {
    if include_methods {
        collect_src_funcs_with_methods(prog, pkg)
    } else {
        collect_src_funcs_members_only(prog, pkg)
    }
}

/// Package-level functions only (`Package.members`). Cheaper RSS/walk; enough
/// when no consumer needs method bodies in `SrcFuncs` (prometheus without
/// contextcheck).
fn collect_src_funcs_members_only(prog: &Program, pkg: PackageId) -> Vec<FuncId> {
    use guff_ssa::member::MemberData;

    let mut funcs = Vec::new();
    let ssa_pkg = prog.packages.get(pkg);
    // Sort by member name so FxHash map order cannot reorder analyzer walks
    // (PERF_TASKS_V2 §0-12 / §A-1).
    let mut top: Vec<(&str, FuncId)> = ssa_pkg
        .members
        .iter()
        .filter_map(|(name, m)| match m {
            MemberData::Function(fid) => Some((name.as_str(), *fid)),
            _ => None,
        })
        .collect();
    top.sort_by(|(a, _), (b, _)| a.cmp(b));
    for (_, fid) in top {
        funcs.push(fid);
        collect_anon_funcs(prog, fid, &mut funcs);
    }
    funcs
}

/// Match `golang.org/x/tools/go/analysis/passes/buildssa`: SrcFuncs is every
/// named function declared in the package's AST (package-level *and* methods).
/// Methods are absent from `Package.members`, so this is required for
/// contextcheck on receivers like `(*ReadyChecker).IsReady` (helm).
fn collect_src_funcs_with_methods(prog: &Program, pkg: PackageId) -> Vec<FuncId> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut named: Vec<(String, FuncId)> = Vec::new();
    for (fid, f) in prog.functions.iter() {
        if f.pkg != Some(pkg) {
            continue;
        }
        if f.object.is_none() {
            continue;
        }
        if f.blocks.is_empty() {
            continue;
        }
        if matches!(
            f.synthetic.as_deref(),
            Some("from type information (on demand)" | "missing generic origin")
        ) {
            continue;
        }
        if !seen.insert(fid) {
            continue;
        }
        named.push((f.name.clone(), fid));
    }
    named.sort_by(|(a, _), (b, _)| a.cmp(b));

    let mut funcs = Vec::new();
    for (_, fid) in named {
        funcs.push(fid);
        collect_anon_funcs(prog, fid, &mut funcs);
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
    // Still build SSA for ill-typed packages when requested (golangci / go/ssa
    // do the same for contextcheck on helm). Default skip keeps prometheus-sized
    // peak RSS down; guff-lint sets `buildir_despite_errors` when contextcheck
    // is enabled. Analyzer `run_despite_errors` stays true so the runner reaches
    // this gate instead of skipping the action entirely.
    if pass.pkg().ill_typed
        && !pass
            .settings::<bool>("buildir_despite_errors")
            .copied()
            .unwrap_or(false)
    {
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

    if std::env::var_os("GUFF_DEBUG_RSS").is_some() {
        sample_ssa_rss(pass.pkg().pkg_path.as_str(), &built.prog);
    }

    // Default true (go/ssa SrcFuncs includes methods) for direct analyzer use /
    // unit tests. guff-lint sets this false when contextcheck is off to avoid
    // ~70MiB peak RSS on large corpora that do not need method SrcFuncs.
    let include_methods = pass
        .settings::<bool>("buildir_src_methods")
        .copied()
        .unwrap_or(true);
    let src_funcs = collect_src_funcs(&built.prog, built.pkg, include_methods);
    Ok(Some(Box::new(BuildIrResult::new(
        Arc::new(built.prog),
        built.pkg,
        built.type_pkg,
        src_funcs,
    ))))
}

/// One-shot SSA incremental size sample (PERF_TASKS_V2 C-8). Shared type-arena
/// bases are already counted in the post-typecheck report; here we charge the
/// owned overlays + SSA function arena so concurrent peak ≈ this × workers.
fn sample_ssa_rss(pkg_path: &str, prog: &Program) {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    static SAMPLED: AtomicBool = AtomicBool::new(false);
    static PEAK_INCR: AtomicU64 = AtomicU64::new(0);
    static SAMPLES: AtomicU64 = AtomicU64::new(0);

    let mut acct = guff_types::RetainedBytes::default();
    prog.type_arena.account_overlay_only(&mut acct);
    prog.object_arena.account_overlay_only(&mut acct);
    prog.package_arena.account_overlay_only(&mut acct);
    // ScopeArena is emptied in snapshot_for_ssa.
    let fn_bytes = prog.functions.len().saturating_mul(512); // rough Function envelope
    let incr = acct.types_total().saturating_add(fn_bytes);
    let n = SAMPLES.fetch_add(1, Ordering::Relaxed) + 1;
    let peak = {
        let mut cur = PEAK_INCR.load(Ordering::Relaxed);
        while incr as u64 > cur {
            match PEAK_INCR.compare_exchange_weak(
                cur,
                incr as u64,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(v) => cur = v,
            }
        }
        PEAK_INCR.load(Ordering::Relaxed)
    };

    // Print the first sample in detail; always refresh a running peak line
    // every 16th package so the final peak is visible without flooding.
    if !SAMPLED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "guff:   ssa sample pkg={pkg_path} incremental≈{:.1}MiB \
             (type/obj/pkg overlays + ~SSA funcs; shared seed base excluded)",
            incr as f64 / (1024.0 * 1024.0),
        );
    }
    if n % 16 == 0 || n == 1 {
        eprintln!(
            "guff:   ssa incremental peak≈{:.1}MiB across {n} buildir pkgs \
             (× concurrency ≈ additive RSS during analyze)",
            peak as f64 / (1024.0 * 1024.0),
        );
    }
}

fn buildir_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "buildir",
        doc: "build SSA IR for later passes",
        url: "https://staticcheck.dev/docs/checks/",
        run: run as RunFn,
        // Allow the runner to invoke buildir on ill-typed packages; the run()
        // body then honors `buildir_despite_errors` (set by guff-lint when
        // contextcheck is enabled). Without this, helm's ill-typed pkgs are
        // skipped before contextcheck can see SSA.
        run_despite_errors: true,
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
        // Let inference pick the driver's hasher rather than naming it here.
        let export_paths = Default::default();
        let dep_graph = Default::default();
        typecheck_package(
            &mut pkg,
            &fset,
            &export_paths,
            &dep_graph,
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
            pkg_arc: None,
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
