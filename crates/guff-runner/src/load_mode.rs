//! Load-mode helpers for combining multiple linter configurations.
//!
//! Port of golangci-lint `linter/config.go` load-mode union logic.

use std::collections::HashMap;

use guff_analysis::Analyzer;
use guff_packages::{load_for_go_analysis, LoadMode};

/// Load mode sufficient for AST-only analyzers (e.g. `inspect` without types).
pub fn ast_only_load_mode() -> LoadMode {
    LoadMode::NEED_NAME
        | LoadMode::NEED_FILES
        | LoadMode::NEED_COMPILED_GO_FILES
        | LoadMode::NEED_SYNTAX
}

/// Load mode for analyzers that need type information.
pub fn types_load_mode() -> LoadMode {
    load_for_go_analysis()
}

/// Returns the union of several load modes (multiple enabled linters).
pub fn union_load_modes(modes: &[LoadMode]) -> LoadMode {
    LoadMode::union_all(modes)
}

/// Infers a conservative load mode for a single analyzer and its `requires` chain.
pub fn infer_load_mode(analyzer: &'static Analyzer) -> LoadMode {
    let mut mode = if analyzer.fact_types.is_empty() {
        ast_only_load_mode()
    } else {
        types_load_mode()
    };
    for req in &analyzer.requires {
        mode = mode.union(infer_load_mode(req));
    }
    mode
}

/// Unions load modes for `analyzers`, using `overrides` when an analyzer name is present.
pub fn load_mode_for_analyzers(
    analyzers: &[&'static Analyzer],
    overrides: &HashMap<&'static str, LoadMode>,
) -> LoadMode {
    let modes: Vec<LoadMode> = analyzers
        .iter()
        .map(|a| overrides.get(a.name).copied().unwrap_or_else(|| infer_load_mode(a)))
        .collect();
    union_load_modes(&modes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::{AnalysisResult, Analyzer, RunError};
    use guff_analysis::Pass;
    use std::sync::OnceLock;

    fn noop_run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        Ok(None)
    }

    fn dummy(name: &'static str) -> &'static Analyzer {
        static A: OnceLock<Analyzer> = OnceLock::new();
        static B: OnceLock<Analyzer> = OnceLock::new();
        static C: OnceLock<Analyzer> = OnceLock::new();
        match name {
            "ast" => A.get_or_init(|| Analyzer {
                name: "ast",
                doc: "ast only",
                url: "",
                run: noop_run,
                run_despite_errors: false,
                requires: vec![],
                fact_types: vec![],
            }),
            "types" => B.get_or_init(|| Analyzer {
                name: "types",
                doc: "needs types",
                url: "",
                run: noop_run,
                run_despite_errors: false,
                requires: vec![],
                fact_types: vec![],
            }),
            _ => C.get_or_init(|| Analyzer {
                name: "other",
                doc: "other",
                url: "",
                run: noop_run,
                run_despite_errors: false,
                requires: vec![],
                fact_types: vec![],
            }),
        }
    }

    #[test]
    fn union_ast_only_and_types_modes() {
        let ast = ast_only_load_mode();
        let types = types_load_mode();
        let union = union_load_modes(&[ast, types]);
        assert!(union.contains(LoadMode::NEED_SYNTAX));
        assert!(union.contains(LoadMode::NEED_TYPES_INFO));
        assert!(union.contains(LoadMode::NEED_NAME));
    }

    #[test]
    fn load_mode_for_analyzers_respects_overrides() {
        let analyzers = [dummy("ast"), dummy("types")];
        let overrides = HashMap::from([
            ("ast", ast_only_load_mode()),
            ("types", types_load_mode()),
        ]);
        let mode = load_mode_for_analyzers(&analyzers, &overrides);
        assert!(mode.contains(LoadMode::NEED_TYPES));
        assert!(mode.contains(LoadMode::NEED_SYNTAX));
    }
}
