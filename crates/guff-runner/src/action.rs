//! Action graph construction and execution.
//!
//! Port of `golang.org/x/tools/go/analysis/checker` (`Action`, `Analyze`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use guff::position::FileSet;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, FactStore, PassInput, validate, ValidateError,
};
use guff_packages::Package;
use guff_types::default_sizes;

/// One unit of analysis work: one analyzer applied to one package.
///
/// Equivalent to `checker.Action`.
pub struct Action {
    pub analyzer: &'static Analyzer,
    pub package: Arc<Package>,
    pub is_root: std::sync::atomic::AtomicBool,
    pub deps: Vec<Arc<Action>>,
    state: Mutex<ActionState>,
}

#[derive(Default)]
struct ActionState {
    result: Option<AnalysisResult>,
    error: Option<String>,
    diagnostics: Vec<Diagnostic>,
    facts: FactStore,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Action")
            .field("id", &self.string_id())
            .field("is_root", &self.is_root.load(Ordering::Relaxed))
            .field("deps", &self.deps.len())
            .finish()
    }
}

impl Action {
    pub fn string_id(&self) -> String {
        format!("{}@{}", self.analyzer.name, self.package.pkg_path)
    }

    pub fn result(&self) -> Option<AnalysisResult> {
        self.state.lock().unwrap().result.as_ref().map(clone_result)
    }

    fn result_arc(&self) -> Option<Arc<AnalysisResult>> {
        self.state
            .lock()
            .unwrap()
            .result
            .as_ref()
            .map(|r| Arc::new(clone_result(r)))
    }

    pub fn error(&self) -> Option<String> {
        self.state.lock().unwrap().error.clone()
    }

    pub fn diagnostics(&self) -> Vec<Diagnostic> {
        self.state.lock().unwrap().diagnostics.clone()
    }

    fn execute(&self) {
        for dep in &self.deps {
            if let Some(err) = dep.error() {
                let mut state = self.state.lock().unwrap();
                state.error = Some(format!("failed prerequisites: {err}"));
                return;
            }
        }

        let mut result_of = HashMap::new();
        let mut facts = FactStore::default();

        for dep in &self.deps {
            let dep_state = dep.state.lock().unwrap();
            if Arc::ptr_eq(&dep.package, &self.package) {
                if let Some(result) = dep_state.result.as_ref() {
                    result_of.insert(dep.analyzer.name, Arc::new(clone_result(result)));
                }
            } else if std::ptr::eq(dep.analyzer, self.analyzer) {
                merge_facts(&mut facts, &dep_state.facts);
            }
        }

        if self.package.ill_typed && !self.analyzer.run_despite_errors {
            let mut state = self.state.lock().unwrap();
            state.error = Some(format!(
                "analysis skipped: package {} is ill-typed",
                self.package.pkg_path
            ));
            return;
        }

        let fset = self
            .package
            .fset
            .clone()
            .unwrap_or_else(FileSet::new);
        let types_sizes = self.package.types_sizes.unwrap_or_else(default_sizes);
        let mut diagnostics = Vec::new();

        let mut pass = PassInput {
            analyzer: self.analyzer,
            fset: &fset,
            files: &self.package.syntax,
            pkg: &self.package,
            types_info: self.package.types_info.as_ref(),
            types_sizes,
            diagnostics: &mut diagnostics,
            result_of,
            facts: &mut facts,
        }
        .build();

        let run_result = (self.analyzer.run)(&mut pass);
        let mut state = self.state.lock().unwrap();
        match run_result {
            Ok(Some(result)) => {
                state.result = Some(result);
                state.diagnostics = diagnostics;
                state.facts = facts;
            }
            Ok(None) => {
                state.diagnostics = diagnostics;
                state.facts = facts;
            }
            Err(err) => {
                state.error = Some(err);
            }
        }
    }
}

fn merge_facts(dst: &mut FactStore, src: &FactStore) {
    for fact in src.all_object_facts() {
        dst.export_object_fact(fact.object, fact.fact);
    }
    for fact in src.all_package_facts() {
        dst.export_package_fact(fact.package, fact.fact);
    }
}

/// Result graph from a round of analysis.
#[derive(Debug)]
pub struct Graph {
    pub roots: Vec<Arc<Action>>,
    all: Vec<Arc<Action>>,
}

impl Graph {
    pub fn all_actions(&self) -> &[Arc<Action>] {
        &self.all
    }

    pub fn root_diagnostics(&self) -> Vec<(String, Diagnostic)> {
        let mut out = Vec::new();
        for root in &self.roots {
            for diag in root.diagnostics() {
                out.push((root.string_id(), diag));
            }
        }
        out
    }
}

/// Builds and executes the action graph.
///
/// Port of `checker.Analyze`.
pub fn analyze(
    analyzers: &[&'static Analyzer],
    packages: &[Arc<Package>],
    sequential: bool,
) -> Result<Graph, ValidateError> {
    validate(analyzers)?;

    let mut actions: HashMap<(*const Analyzer, String), Arc<Action>> = HashMap::new();
    let mut all: Vec<Arc<Action>> = Vec::new();

    fn mk_action(
        analyzer: &'static Analyzer,
        package: Arc<Package>,
        actions: &mut HashMap<(*const Analyzer, String), Arc<Action>>,
        all: &mut Vec<Arc<Action>>,
    ) -> Arc<Action> {
        let key = (analyzer as *const Analyzer, package.id.clone());
        if let Some(act) = actions.get(&key) {
            return Arc::clone(act);
        }

        let mut deps = Vec::new();
        for req in &analyzer.requires {
            deps.push(mk_action(req, Arc::clone(&package), actions, all));
            if !req.fact_types.is_empty() {
                let mut paths: Vec<String> = package.imports.keys().cloned().collect();
                paths.sort();
                for path in paths {
                    if let Some(dep_pkg) = package.imports.get(&path) {
                        deps.push(mk_action(req, Arc::clone(dep_pkg), actions, all));
                    }
                }
            }
        }

        if !analyzer.fact_types.is_empty() {
            let mut paths: Vec<String> = package.imports.keys().cloned().collect();
            paths.sort();
            for path in paths {
                if let Some(dep_pkg) = package.imports.get(&path) {
                    deps.push(mk_action(analyzer, Arc::clone(dep_pkg), actions, all));
                }
            }
        }

        let act = Arc::new(Action {
            analyzer,
            package,
            is_root: std::sync::atomic::AtomicBool::new(false),
            deps,
            state: Mutex::new(ActionState::default()),
        });
        actions.insert(key, Arc::clone(&act));
        all.push(Arc::clone(&act));
        act
    }

    let mut roots = Vec::new();
    for &analyzer in analyzers {
        for pkg in packages {
            let act = mk_action(analyzer, Arc::clone(pkg), &mut actions, &mut all);
            act.is_root.store(true, Ordering::Relaxed);
            roots.push(act);
        }
    }

    exec_all(&roots, sequential);

    for act in &all {
        if !act.is_root.load(Ordering::Relaxed) {
            let mut state = act.state.lock().unwrap();
            state.result = None;
        }
    }

    Ok(Graph { roots, all })
}

fn topo_postorder(roots: &[Arc<Action>]) -> Vec<Arc<Action>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    fn visit(act: &Arc<Action>, seen: &mut HashSet<usize>, out: &mut Vec<Arc<Action>>) {
        let ptr = Arc::as_ptr(act) as usize;
        if seen.contains(&ptr) {
            return;
        }
        seen.insert(ptr);
        for dep in &act.deps {
            visit(dep, seen, out);
        }
        out.push(Arc::clone(act));
    }

    for root in roots {
        visit(root, &mut seen, &mut out);
    }
    out
}

pub(crate) fn exec_all(roots: &[Arc<Action>], sequential: bool) {
    let order = topo_postorder(roots);
    if sequential {
        for act in order {
            act.execute();
        }
        return;
    }

    // `Package` is not `Sync` today (AST uses `RefCell`), so cross-thread package
    // sharing is deferred (PL11). Independent roots still run through the same
    // topological schedule until AST storage is shareable across workers.
    for act in order {
        act.execute();
    }
}

fn clone_result(result: &AnalysisResult) -> AnalysisResult {
    if let Some(inspect) = result.downcast_ref::<guff_analysis::passes::inspect::InspectResult>() {
        return Box::new(inspect.clone());
    }
    if let Some(ir) = result.downcast_ref::<guff_analysis::passes::buildir::BuildIrResult>() {
        return Box::new(ir.clone());
    }
    if let Some(depr) = result.downcast_ref::<guff_analysis::DeprecatedResult>() {
        return Box::new(depr.clone());
    }
    if let Some(gen) = result.downcast_ref::<guff_analysis::GeneratedResult>() {
        return Box::new(gen.clone());
    }
    panic!("unsupported AnalysisResult clone; add a clone path for this result type");
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{AnalysisResult, Pass, RunError};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::OnceLock;

    static ORDER: AtomicUsize = AtomicUsize::new(0);
    static LOG: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

    fn record_run(
        name: &'static str,
    ) -> impl Fn(&mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> + Copy {
        move |_pass: &mut Pass<'_>| {
            LOG.lock().unwrap().push(name);
            ORDER.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
    }

    fn c_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("c")(pass)
    }

    fn b_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("b")(pass)
    }

    fn a_run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        record_run("a")(pass)
    }

    fn analyzer(
        name: &'static str,
        requires: Vec<&'static Analyzer>,
        run: fn(&mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError>,
    ) -> &'static Analyzer {
        static A: OnceLock<Analyzer> = OnceLock::new();
        static B: OnceLock<Analyzer> = OnceLock::new();
        static C: OnceLock<Analyzer> = OnceLock::new();
        match name {
            "a" => A.get_or_init(|| Analyzer {
                name: "a",
                doc: "a",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            "b" => B.get_or_init(|| Analyzer {
                name: "b",
                doc: "b",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            "c" => C.get_or_init(|| Analyzer {
                name: "c",
                doc: "c",
                url: "",
                run,
                run_despite_errors: false,
                requires,
                fact_types: vec![],
            }),
            _ => panic!("unknown test analyzer {name}"),
        }
    }

    fn typechecked_pkg() -> Arc<Package> {
        use guff::position::FileSet;
        use guff_packages::{typecheck_package, LoadMode, TypecheckEnv};

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
            &HashMap::new(),
            &HashMap::new(),
            default_sizes(),
            &TypecheckEnv::default(),
            LoadMode::LOAD_SYNTAX,
        );
        Arc::new(pkg)
    }

    #[test]
    fn requires_chain_runs_in_dependency_order() {
        let c = analyzer("c", vec![], c_run);
        let b = analyzer("b", vec![c], b_run);
        let a = analyzer("a", vec![b], a_run);

        *LOG.lock().unwrap() = Vec::new();
        ORDER.store(0, Ordering::SeqCst);

        let pkg = typechecked_pkg();
        let graph = analyze(&[a], std::slice::from_ref(&pkg), true).expect("analyze");
        assert_eq!(graph.roots.len(), 1);
        assert!(graph.roots[0].error().is_none());

        let log = LOG.lock().unwrap().clone();
        assert_eq!(log, vec!["c", "b", "a"]);
    }
}
