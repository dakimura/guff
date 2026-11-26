//! Load-mode presets for common linter configurations.

use crate::load_mode::LoadMode;

/// Load mode used by most `go/analysis` linters (golangci `WithLoadForGoAnalysis`
/// plus the type/syntax flags added by the analysis runner).
pub fn load_for_go_analysis() -> LoadMode {
    LoadMode::NEED_NAME
        | LoadMode::NEED_FILES
        | LoadMode::NEED_COMPILED_GO_FILES
        | LoadMode::NEED_IMPORTS
        | LoadMode::NEED_DEPS
        | LoadMode::NEED_EXPORT_FILE
        | LoadMode::NEED_TYPES
        | LoadMode::NEED_TYPES_SIZES
        | LoadMode::NEED_SYNTAX
        | LoadMode::NEED_TYPES_INFO
        // Module.GoVersion is required by modernize / stdlib_version gates
        // (e.g. slicesbackward needs go1.23+). NEED_TYPES implies NEED_MODULE
        // via `implied()`, but keep it explicit so metadata-only loads retain it.
        | LoadMode::NEED_MODULE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load_mode::LoadMode;

    #[test]
    fn load_for_go_analysis_includes_required_bits() {
        let mode = load_for_go_analysis();
        assert!(mode.contains(LoadMode::NEED_NAME));
        assert!(mode.contains(LoadMode::NEED_FILES));
        assert!(mode.contains(LoadMode::NEED_COMPILED_GO_FILES));
        assert!(mode.contains(LoadMode::NEED_IMPORTS));
        assert!(mode.contains(LoadMode::NEED_DEPS));
        assert!(mode.contains(LoadMode::NEED_EXPORT_FILE));
        assert!(mode.contains(LoadMode::NEED_TYPES));
        assert!(mode.contains(LoadMode::NEED_TYPES_SIZES));
        assert!(mode.contains(LoadMode::NEED_SYNTAX));
        assert!(mode.contains(LoadMode::NEED_TYPES_INFO));
        assert!(mode.contains(LoadMode::NEED_MODULE));
    }
}
