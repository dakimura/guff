//! Analyzer definitions.
//!
//! Port of `go/analysis/analysis.go` (`Analyzer`).

use std::any::Any;
use std::fmt;

use crate::facts::FactTypeId;
use crate::pass::Pass;

/// Error returned from an analyzer [`Run`](Analyzer::run) function.
pub type RunError = String;

/// Optional result produced by an analyzer for its dependents.
pub type AnalysisResult = Box<dyn Any + Send>;

/// Function that applies an analyzer to a single package.
pub type RunFn = fn(&mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError>;

/// Describes an analysis function and its options.
///
/// Equivalent to `analysis.Analyzer`.
pub struct Analyzer {
    /// Valid Go identifier used in flags and URLs.
    pub name: &'static str,
    /// Documentation (title is the part before the first blank line).
    pub doc: &'static str,
    /// Optional link to extended documentation.
    pub url: &'static str,
    /// Applies the analyzer to a package.
    pub run: RunFn,
    /// Run even when the package has parse or type errors.
    pub run_despite_errors: bool,
    /// Analyzers that must run successfully before this one on the same package.
    pub requires: Vec<&'static Analyzer>,
    /// Fact types this analyzer may import and export.
    pub fact_types: Vec<FactTypeId>,
}

impl fmt::Display for Analyzer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name)
    }
}

impl fmt::Debug for Analyzer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Analyzer")
            .field("name", &self.name)
            .field(
                "requires",
                &self.requires.iter().map(|a| a.name).collect::<Vec<_>>(),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;
    use crate::pass::Pass;

    fn noop_run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        Ok(None)
    }

    fn child() -> &'static Analyzer {
        static CHILD: OnceLock<Analyzer> = OnceLock::new();
        CHILD.get_or_init(|| Analyzer {
            name: "child",
            doc: "child analyzer",
            url: "",
            run: noop_run,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        })
    }

    fn parent() -> &'static Analyzer {
        static PARENT: OnceLock<Analyzer> = OnceLock::new();
        PARENT.get_or_init(|| Analyzer {
            name: "parent",
            doc: "parent analyzer",
            url: "",
            run: noop_run,
            run_despite_errors: false,
            requires: vec![child()],
            fact_types: vec![],
        })
    }

    #[test]
    fn analyzer_exposes_name_and_requires() {
        let p = parent();
        assert_eq!(p.name, "parent");
        assert_eq!(p.requires.len(), 1);
        assert_eq!(p.requires[0].name, "child");
        assert_eq!(p.to_string(), "parent");
    }
}
