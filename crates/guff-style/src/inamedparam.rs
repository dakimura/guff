//! Port of [`github.com/macabu/inamedparam`](https://github.com/macabu/inamedparam)
//! (golangci-lint wrapper in `pkg/golinters/inamedparam`).
//!
//! Reports interface methods whose parameters lack names. Embedded interfaces
//! / type elements (no method names) are skipped, matching upstream.
//!
//! Setting `skip-single-param` (default false) skips methods that have exactly
//! one parameter field in the AST field list.

use std::sync::OnceLock;

use guff::ast::Expr;
use guff::walk::{preorder, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::InamedparamOptions;

/// Format a parameter type for the diagnostic, matching upstream's Ident /
/// SelectorExpr-only pretty-print (other shapes fall back to the generic msg).
fn built_param_type(ty: &Expr) -> Option<String> {
    match ty {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            let mut s = String::new();
            if let Expr::Ident(pkg) = sel.x.as_ref() {
                s.push_str(&pkg.name);
                s.push('.');
            }
            s.push_str(&sel.sel.name);
            Some(s)
        }
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "inamedparam requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<InamedparamOptions>("inamedparam")
        .copied()
        .unwrap_or_default();
    let skip_single_param = opts.skip_single_param;

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| {
            if let NodeRef::InterfaceType(it) = n {
                for method in &it.methods.list {
                    // Embedded interface / constraint element: no method name.
                    if method.names.is_empty() {
                        continue;
                    }
                    let method_name = method.names[0].name.clone();
                    let Some(Expr::FuncType(ft)) = method.ty.as_ref() else {
                        continue;
                    };
                    let Some(params) = ft.params.as_ref() else {
                        continue;
                    };

                    if skip_single_param && params.list.len() == 1 {
                        continue;
                    }

                    for param in &params.list {
                        if !param.names.is_empty() {
                            continue;
                        }
                        let Some(ty) = param.ty.as_ref() else {
                            continue;
                        };
                        let msg = match built_param_type(ty) {
                            Some(t) => format!(
                                "interface method {method_name} must have named param for type {t}"
                            ),
                            None => format!(
                                "interface method {method_name} must have all named params"
                            ),
                        };
                        pending.push((param.pos().0 as u32, msg));
                    }
                }
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
        name: "inamedparam",
        doc: "Reports interfaces with unnamed method parameters.",
        url: "https://github.com/macabu/inamedparam",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
