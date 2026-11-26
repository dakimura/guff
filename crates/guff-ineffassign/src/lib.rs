//! guff-ineffassign — detect ineffectual assignments.

mod cfg;

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::code::is_generated_at;
use guff_analysis::passes::facts::generated;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let info = pass
        .types_info()
        .ok_or_else(|| "ineffassign requires types info".to_string())?;

    // Upstream keys its variable table on `*ast.Object`, which the *parser*
    // fills in — so an identifier it cannot resolve within the file has a nil
    // `Obj` and is never tracked at all. The two that matter are a name that
    // arrives through a dot import and anything from the universe scope: guff
    // resolves both through the type checker, which does know them, and then
    // reported an assignment to another package's variable as an ineffectual
    // assignment to a local (velero's `ReportData`, reached through
    // `. "github.com/vmware-tanzu/velero/test"`).
    let own_pkg = pass.type_pkg();
    let objects = pass
        .pkg()
        .type_artifacts
        .as_ref()
        .map(|a| &a.objects);
    let mine = |obj: guff_types::arena::ObjectId| -> bool {
        match (own_pkg, objects) {
            (Some(own), Some(arena)) => obj.pkg(arena) == Some(own),
            // Without the arena there is nothing to filter on; keeping every
            // object is what guff did before and stays the safer default.
            _ => true,
        }
    };

    let defs_map: HashMap<u32, Option<guff_types::arena::ObjectId>> = info
        .defs
        .iter()
        .map(|(k, v)| (*k, v.filter(|o| mine(*o))))
        .collect();
    let uses_map: HashMap<u32, guff_types::arena::ObjectId> = info
        .uses
        .iter()
        .filter(|(_, v)| mine(**v))
        .map(|(k, v)| (*k, *v))
        .collect();

    // Package-level vars are declared in one file and assigned/read in others
    // (e.g. gin `codec/json` `var API` + `init` in build-tagged files). Mark
    // them as escaping once for the whole package before per-file CFGs.
    let package_escape_objs = cfg::package_level_var_objs(pass.files(), &defs_map, &uses_map);

    let mut pending = Vec::new();
    for file in pass.files() {
        if is_generated_at(pass, file.file_start.0 as u32) {
            continue;
        }
        pending.extend(cfg::analyze_file(
            &file.decls,
            &defs_map,
            &uses_map,
            &package_escape_objs,
        ));
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "ineffassign",
        doc: "detects when assignments to existing variables are not used",
        url: "https://github.com/gordonklaus/ineffassign",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![generated::analyzer()],
        fact_types: vec![],
    })
}

pub fn analyzers() -> Vec<&'static Analyzer> {
    vec![analyzer()]
}
