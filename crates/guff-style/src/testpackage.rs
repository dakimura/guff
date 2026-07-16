//! Port of [`github.com/maratori/testpackage`](https://github.com/maratori/testpackage)
//! (golangci-lint wrapper in `pkg/golinters/testpackage`).
//!
//! Reports `*_test.go` files whose package name does not end with `_test`
//! (unless the package is in `allow-packages`, default `main`).
//!
//! Files whose names match `skip-regexp` (default `(export|internal)_test\.go`)
//! are skipped.
//!
//! Settings (`linters.settings.testpackage`):
//! - `skip-regexp` (default `(export|internal)_test\.go`)
//! - `allow-packages` (default `["main"]`)

use std::sync::OnceLock;

use regex::Regex;

use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::TestpackageOptions;

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let opts = pass
        .settings::<TestpackageOptions>("testpackage")
        .cloned()
        .unwrap_or_default();

    let skip_re = Regex::new(&opts.skip_regexp).map_err(|e| {
        format!(
            "testpackage: invalid skip-regexp {:?}: {e}",
            opts.skip_regexp
        )
    })?;

    let mut pending: Vec<(u32, String)> = Vec::new();
    let pkg = pass.pkg();
    let fset = pass.fset();

    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| fallback.clone());

        if !filename.ends_with("_test.go") || skip_re.is_match(&filename) {
            continue;
        }

        let package_name = &file.name.name;
        if opts.allow_packages.iter().any(|p| p == package_name) {
            continue;
        }

        if !package_name.ends_with("_test") {
            pending.push((
                file.name.pos().0 as u32,
                format!("package should be `{package_name}_test` instead of `{package_name}`"),
            ));
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "testpackage",
        doc: "linter that makes you use a separate _test package",
        url: "https://github.com/maratori/testpackage",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn analyzer_graph_is_valid() {
        validate(&[analyzer()]).expect("valid analyzer graph");
    }

    #[test]
    fn default_skip_regexp_compiles() {
        Regex::new(r"(export|internal)_test\.go").expect("default skip-regexp");
    }
}
