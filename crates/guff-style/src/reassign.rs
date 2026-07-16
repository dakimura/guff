//! Port of [`github.com/curioswitch/go-reassign`](https://github.com/curioswitch/go-reassign)
//! (golangci-lint wrapper in `pkg/golinters/reassign`).
//!
//! Detects reassignment of top-level variables from other packages
//! (e.g. `io.EOF = nil`). Default pattern matches `EOF` and `Err*`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Ident};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use regex::Regex;

use crate::options::ReassignOptions;

/// Upstream / golangci default when `patterns` is empty.
const DEFAULT_PATTERN: &str = r"^(Err.*|EOF)$";

/// Compile settings into a single regexp, matching golangci-lint:
/// non-empty `patterns` → `^(p1|p2|…)$`; empty → [`DEFAULT_PATTERN`].
fn compile_pattern(opts: &ReassignOptions) -> Option<Regex> {
    let source = if opts.patterns.is_empty() {
        DEFAULT_PATTERN.to_string()
    } else {
        format!("^({})$", opts.patterns.join("|"))
    };
    Regex::new(&source).ok()
}

fn report_imported(
    pass: &Pass<'_>,
    expr: &Expr,
    check_re: &Regex,
    pending: &mut Vec<(u32, String)>,
) {
    match expr {
        Expr::SelectorExpr(sel) => {
            let Expr::Ident(pkg_ident) = sel.x.as_ref() else {
                return;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return;
            };
            let pkg_path = match imported_pkg_id(pass, pkg_ident) {
                Some(id) => {
                    // Upstream: not a PkgName, or Imported() == pass.Pkg → skip.
                    if pass.type_pkg() == Some(id) {
                        return;
                    }
                    artifacts.packages.get(id).path().to_string()
                }
                // X is not a package name (e.g. shadowed local struct).
                None => return,
            };

            let matches = check_re.is_match(&sel.sel.name)
                || (!pkg_path.is_empty()
                    && check_re.is_match(&format!("{pkg_path}.{}", sel.sel.name)));
            if !matches {
                return;
            }
            pending.push((
                expr.pos().0 as u32,
                format!(
                    "reassigning variable {} in other package {}",
                    sel.sel.name, pkg_ident.name
                ),
            ));
        }
        Expr::Ident(id) => {
            // Dot-import case: bare `EOF = nil`.
            let Some(info) = pass.types_info() else {
                return;
            };
            let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
                return;
            };
            let Some(&obj) = info.uses.get(&id.id) else {
                return;
            };
            let ObjectData::Var(_) = artifacts.objects.get(obj) else {
                return;
            };
            let Some(pkg) = obj.pkg(&artifacts.objects) else {
                return;
            };
            if pass.type_pkg() == Some(pkg) {
                return;
            }
            if !check_re.is_match(&id.name) {
                return;
            }
            let path = artifacts.packages.get(pkg).path();
            pending.push((
                expr.pos().0 as u32,
                format!(
                    "reassigning variable {} from other package {}",
                    id.name, path
                ),
            ));
        }
        _ => {}
    }
}

fn imported_pkg_id(pass: &Pass<'_>, pkg_ident: &Ident) -> Option<guff_types::PackageId> {
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj_id = info
        .defs
        .get(&pkg_ident.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&pkg_ident.id).copied())?;
    match artifacts.objects.get(obj_id) {
        ObjectData::PkgName(pn) => Some(pn.imported()),
        _ => None,
    }
}

fn check_assign(pass: &Pass<'_>, assign: &AssignStmt, check_re: &Regex, pending: &mut Vec<(u32, String)>) {
    for lhs in &assign.lhs {
        report_imported(pass, lhs, check_re, pending);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "reassign requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<ReassignOptions>("reassign")
        .cloned()
        .unwrap_or_default();
    let Some(check_re) = compile_pattern(&opts) else {
        return Err("reassign: invalid pattern".into());
    };

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::AssignStmt(a) = n {
                check_assign(pass, a, &check_re, &mut pending);
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "reassign",
        doc: "Checks that package variables are not reassigned",
        url: "https://github.com/curioswitch/go-reassign",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pattern_matches_eof_and_err() {
        let re = compile_pattern(&ReassignOptions::default()).unwrap();
        assert!(re.is_match("EOF"));
        assert!(re.is_match("ErrB"));
        assert!(re.is_match("ErrSomething"));
        assert!(!re.is_match("NotErr"));
        assert!(!re.is_match("DefaultClient"));
    }

    #[test]
    fn custom_patterns_joined_like_golangci() {
        let re = compile_pattern(&ReassignOptions {
            patterns: vec![".*".into()],
        })
        .unwrap();
        assert!(re.is_match("NotErr"));
        assert!(re.is_match("DefaultClient"));
    }

    #[test]
    fn package_qualified_pattern() {
        let re = compile_pattern(&ReassignOptions {
            patterns: vec!["net/http\\.Default.*".into()],
        })
        .unwrap();
        assert!(re.is_match("net/http.DefaultClient"));
        assert!(!re.is_match("DefaultClient"));
    }
}
