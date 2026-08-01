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

    let defs_map: HashMap<u32, Option<guff_types::arena::ObjectId>> =
        info.defs.iter().map(|(k, v)| (*k, *v)).collect();
    let uses_map: HashMap<u32, guff_types::arena::ObjectId> =
        info.uses.iter().map(|(k, v)| (*k, *v)).collect();

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
