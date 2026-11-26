//! Analyzer graph validation.
//!
//! Port of `go/analysis/validate.go`.

use std::collections::{HashMap, HashSet};

use crate::analyzer::Analyzer;
use crate::facts::FactTypeId;

/// Reports misconfigured analyzers.
#[derive(Debug, PartialEq, Eq)]
pub enum ValidateError {
    NilAnalyzer,
    InvalidName(&'static str),
    Undocumented(&'static str),
    NilRun(&'static str),
    DuplicateFactType { analyzer: &'static str, fact: String },
    Cycle { names: Vec<String> },
    DuplicateAnalyzer(&'static str),
}

impl std::fmt::Display for ValidateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NilAnalyzer => write!(f, "nil analyzer"),
            Self::InvalidName(name) => write!(f, "invalid analyzer name {name:?}"),
            Self::Undocumented(name) => write!(f, "analyzer {name:?} is undocumented"),
            Self::NilRun(name) => write!(f, "analyzer {name:?} has nil Run"),
            Self::DuplicateFactType { analyzer, fact } => {
                write!(f, "fact type {fact} registered by two analyzers (including {analyzer})")
            }
            Self::Cycle { names } => {
                write!(f, "cycle detected involving analyzers: {}", names.join(" "))
            }
            Self::DuplicateAnalyzer(name) => write!(f, "duplicate analyzer: {name}"),
        }
    }
}

impl std::error::Error for ValidateError {}

/// Validates a set of analyzers (names, documentation, acyclic `requires` graph,
/// unique fact types).
///
/// Port of `analysis.Validate`.
pub fn validate(analyzers: &[&'static Analyzer]) -> Result<(), ValidateError> {
    let mut fact_types: HashMap<FactTypeId, &'static str> = HashMap::new();
    let mut color: HashMap<*const Analyzer, u8> = HashMap::new();

    for &a in analyzers {
        visit(a, &mut fact_types, &mut color)?;
    }

    let mut finished: HashSet<*const Analyzer> = HashSet::new();
    for &a in analyzers {
        let ptr = a as *const Analyzer;
        if finished.contains(&ptr) {
            return Err(ValidateError::DuplicateAnalyzer(a.name));
        }
        finished.insert(ptr);
    }

    Ok(())
}

const WHITE: u8 = 0;
const GREY: u8 = 1;
const BLACK: u8 = 2;

fn visit(
    a: &'static Analyzer,
    fact_types: &mut HashMap<FactTypeId, &'static str>,
    color: &mut HashMap<*const Analyzer, u8>,
) -> Result<(), ValidateError> {
    let ptr = a as *const Analyzer;
    let state = color.get(&ptr).copied().unwrap_or(WHITE);
    if state == BLACK {
        return Ok(());
    }
    if state == GREY {
        return Err(ValidateError::Cycle {
            names: vec![a.name.to_string()],
        });
    }

    color.insert(ptr, GREY);

    if !valid_ident(a.name) {
        return Err(ValidateError::InvalidName(a.name));
    }
    if a.doc.is_empty() {
        return Err(ValidateError::Undocumented(a.name));
    }
    // Run is a function pointer; it is never null in Rust.

    for &fact in &a.fact_types {
        if let Some(prev) = fact_types.get(&fact) {
            return Err(ValidateError::DuplicateFactType {
                analyzer: a.name,
                fact: format!("{fact:?} (also registered by {prev})"),
            });
        }
        fact_types.insert(fact, a.name);
    }

    for req in &a.requires {
        visit(req, fact_types, color)?;
    }

    color.insert(ptr, BLACK);
    Ok(())
}

fn valid_ident(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    for (i, ch) in name.chars().enumerate() {
        if ch == '_' || ch.is_alphabetic() || (i > 0 && ch.is_ascii_digit()) {
            continue;
        }
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::{AnalysisResult, RunError};
    use crate::pass::Pass;

    fn noop_run(_pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
        Ok(None)
    }

    #[test]
    fn valid_analyzer_graph_passes() {
        use std::sync::OnceLock;

        fn child() -> &'static Analyzer {
            static CHILD: OnceLock<Analyzer> = OnceLock::new();
            CHILD.get_or_init(|| Analyzer {
                name: "child",
                doc: "child",
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
                doc: "parent",
                url: "",
                run: noop_run,
                run_despite_errors: false,
                requires: vec![child()],
                fact_types: vec![],
            })
        }

        validate(&[parent(), child()]).expect("valid graph");
    }

    #[test]
    fn cycle_in_requires_graph_is_rejected() {
        let b = Box::new(Analyzer {
            name: "B",
            doc: "b",
            url: "",
            run: noop_run,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        });
        let a = Box::new(Analyzer {
            name: "A",
            doc: "a",
            url: "",
            run: noop_run,
            run_despite_errors: false,
            requires: vec![],
            fact_types: vec![],
        });
        let b_ptr: &'static Analyzer = Box::leak(b);
        let a_ptr: &'static Analyzer = Box::leak(a);
        unsafe {
            let b_mut = &mut *(b_ptr as *const Analyzer as *mut Analyzer);
            b_mut.requires.push(a_ptr);
            let a_mut = &mut *(a_ptr as *const Analyzer as *mut Analyzer);
            a_mut.requires.push(b_ptr);
        }
        let err = validate(&[a_ptr, b_ptr]).unwrap_err();
        assert!(matches!(err, ValidateError::Cycle { .. }));
    }
}
