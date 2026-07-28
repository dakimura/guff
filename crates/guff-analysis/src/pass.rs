//! The [`Pass`] type — inputs and outputs for a single analyzer on one package.
//!
//! Port of `go/analysis/analysis.go` (`Pass`).

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use guff::ast::File;
use guff::position::FileSet;
use guff_packages::Package;
use guff_types::api::Info;
use guff_types::arena::{ObjectId, PackageId};
use guff_types::Sizes;

use crate::analyzer::{AnalysisResult, Analyzer};
use crate::diagnostic::Diagnostic;
use crate::facts::{Fact, FactStore, ObjectFact, PackageFact};
use crate::settings::SettingsBag;

/// Information passed to an analyzer's run function.
///
/// Equivalent to `analysis.Pass`.
pub struct Pass<'a> {
    pub analyzer: &'static Analyzer,
    fset: &'a Arc<FileSet>,
    files: &'a [File],
    pkg: &'a Package,
    /// Owning handle to the same package as `pkg`, when the caller has one.
    ///
    /// Only the `inspect` pass uses it: its flat event array holds raw pointers
    /// into `files`, and keeping the `Arc` alongside them makes "the AST
    /// outlives the events" a local fact instead of a property of the runner's
    /// drop order. `None` (tests, ad-hoc passes) simply disables that fast path.
    pkg_arc: Option<Arc<Package>>,
    type_pkg: Option<PackageId>,
    types_info: Option<&'a Info>,
    types_sizes: Sizes,
    diagnostics: &'a mut Vec<Diagnostic>,
    result_of: HashMap<&'static str, Arc<AnalysisResult>>,
    facts: &'a mut FactStore,
    other_files: Vec<String>,
    ignored_files: Vec<String>,
    settings: Arc<SettingsBag>,
}

/// Inputs needed to construct a [`Pass`].
pub struct PassInput<'a> {
    pub analyzer: &'static Analyzer,
    pub fset: &'a Arc<FileSet>,
    pub files: &'a [File],
    pub pkg: &'a Package,
    /// Owning handle to `pkg`. See [`Pass::pkg_arc`]; `None` is always valid.
    pub pkg_arc: Option<Arc<Package>>,
    pub types_info: Option<&'a Info>,
    pub types_sizes: Sizes,
    pub diagnostics: &'a mut Vec<Diagnostic>,
    pub result_of: HashMap<&'static str, Arc<AnalysisResult>>,
    pub facts: &'a mut FactStore,
    /// Per-linter settings for this analysis run (shared across packages).
    pub settings: Arc<SettingsBag>,
}

impl<'a> PassInput<'a> {
    pub fn build(self) -> Pass<'a> {
        let other_files: Vec<String> = self
            .pkg
            .other_files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let ignored_files: Vec<String> = self
            .pkg
            .ignored_files
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        Pass {
            analyzer: self.analyzer,
            fset: self.fset,
            files: self.files,
            pkg: self.pkg,
            pkg_arc: self.pkg_arc,
            type_pkg: self.pkg.types,
            types_info: self.types_info,
            types_sizes: self.types_sizes,
            diagnostics: self.diagnostics,
            result_of: self.result_of,
            facts: self.facts,
            other_files,
            ignored_files,
            settings: self.settings,
        }
    }
}

impl<'a> Pass<'a> {
    pub fn fset(&self) -> &Arc<FileSet> {
        self.fset
    }

    pub fn files(&self) -> &[File] {
        self.files
    }

    pub fn pkg(&self) -> &Package {
        self.pkg
    }

    /// Owning handle to [`pkg`](Self::pkg), when the caller supplied one.
    pub fn pkg_arc(&self) -> Option<&Arc<Package>> {
        self.pkg_arc.as_ref()
    }

    pub fn type_pkg(&self) -> Option<PackageId> {
        self.type_pkg
    }

    pub fn types_info(&self) -> Option<&Info> {
        self.types_info
    }

    pub fn types_sizes(&self) -> Sizes {
        self.types_sizes
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        self.diagnostics
    }

    pub fn other_files(&self) -> &[String] {
        &self.other_files
    }

    pub fn ignored_files(&self) -> &[String] {
        &self.ignored_files
    }

    /// Shared settings bag for this run (see [`SettingsBag`]).
    pub fn settings_bag(&self) -> &SettingsBag {
        &self.settings
    }

    /// Typed settings previously stored under `key` (usually a linter name).
    pub fn settings<T: Any + Send + Sync>(&self, key: &str) -> Option<&T> {
        self.settings.get(key)
    }

    pub fn result_of<T: Any>(&self, analyzer: &'static Analyzer) -> Option<&T> {
        self.result_of
            .get(analyzer.name)
            .and_then(|r| r.downcast_ref::<T>())
    }

    pub fn insert_result(&mut self, analyzer: &'static Analyzer, result: AnalysisResult) {
        self.result_of.insert(analyzer.name, Arc::new(result));
    }

    pub fn report(&mut self, diag: Diagnostic) {
        self.diagnostics.push(diag);
    }

    pub fn reportf(&mut self, pos: u32, message: impl Into<String>) {
        self.report(Diagnostic {
            pos,
            message: message.into(),
            ..Diagnostic::default()
        });
    }

    /// Emit a diagnostic unless `pos` is in a generated file.
    ///
    /// Port of `report.FilterGenerated`.
    pub fn report_unless_generated(&mut self, pos: u32, message: impl Into<String>) {
        if crate::code::is_generated_at(self, pos) {
            return;
        }
        self.reportf(pos, message);
    }

    pub fn import_object_fact<F: Fact + Clone>(&self, object: ObjectId, fact: &mut F) -> bool {
        self.facts.import_object_fact(object, fact)
    }

    pub fn export_object_fact(&mut self, object: ObjectId, fact: Box<dyn Fact>) {
        self.facts.export_object_fact(object, fact);
    }

    pub fn import_package_fact<F: Fact + Clone>(&self, package: PackageId, fact: &mut F) -> bool {
        self.facts.import_package_fact(package, fact)
    }

    pub fn export_package_fact(&mut self, package: PackageId, fact: Box<dyn Fact>) {
        self.facts.export_package_fact(package, fact);
    }

    pub fn all_object_facts(&self) -> Vec<ObjectFact> {
        self.facts.all_object_facts()
    }

    pub fn all_package_facts(&self) -> Vec<PackageFact> {
        self.facts.all_package_facts()
    }

    pub fn string(&self) -> String {
        format!("{}@{}", self.analyzer.name, self.pkg.pkg_path)
    }
}

#[cfg(test)]
mod tests {
    use guff_packages::{typecheck_package, LoadMode, Package, TypecheckEnv};
    use guff_types::default_sizes;
    use guff::position::FileSet;

    use super::*;
    use crate::analyzer::{AnalysisResult, RunError};
    use crate::Analyzer;

    fn noop_run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        Ok(None)
    }

    fn test_analyzer() -> &'static Analyzer {
        use std::sync::OnceLock;
        static A: OnceLock<Analyzer> = OnceLock::new();
        A.get_or_init(|| Analyzer {
            name: "test",
            doc: "test pass",
            url: "",
            run: noop_run,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        })
    }

    fn typechecked_package() -> (Package, std::sync::Arc<FileSet>) {
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
        (pkg, fset)
    }

    #[test]
    fn pass_exposes_package_fields() {
        let (pkg, fset) = typechecked_package();
        let mut diags = Vec::new();
        let mut facts = FactStore::default();
        let pass = PassInput {
            analyzer: test_analyzer(),
            fset: &fset,
            files: &pkg.syntax,
            pkg: &pkg,
            pkg_arc: None,
            types_info: pkg.types_info.as_deref(),
            types_sizes: pkg.types_sizes.unwrap_or_else(default_sizes),
            diagnostics: &mut diags,
            result_of: HashMap::new(),
            facts: &mut facts,
            settings: Arc::new(crate::SettingsBag::default()),
        }
        .build();

        assert_eq!(pass.files().len(), 1);
        assert_eq!(pass.files()[0].name.name, "main");
        assert!(pass.types_info().is_some());
        assert!(pass.type_pkg().is_some());
        assert_eq!(pass.string(), "test@example.com/valid");
    }

    #[test]
    fn pass_fact_export_import() {
        let (pkg, fset) = typechecked_package();
        let type_pkg = pkg.types.expect("types");
        let mut diags = Vec::new();
        let mut facts = FactStore::default();
        let mut pass = PassInput {
            analyzer: test_analyzer(),
            fset: &fset,
            files: &pkg.syntax,
            pkg: &pkg,
            pkg_arc: None,
            types_info: pkg.types_info.as_deref(),
            types_sizes: default_sizes(),
            diagnostics: &mut diags,
            result_of: HashMap::new(),
            facts: &mut facts,
            settings: Arc::new(crate::SettingsBag::default()),
        }
        .build();

        pass.export_package_fact(type_pkg, Box::new(crate::facts::StringFact("ok".into())));
        let mut fact = crate::facts::StringFact(String::new());
        assert!(pass.import_package_fact(type_pkg, &mut fact));
        assert_eq!(fact.0, "ok");
    }
}
