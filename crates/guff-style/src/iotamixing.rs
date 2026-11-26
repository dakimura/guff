//! Port of [`github.com/AdminBenni/iota-mixing`](https://github.com/AdminBenni/iota-mixing)
//! (golangci-lint wrapper in `pkg/golinters/iotamixing`).
//!
//! Reports `const` blocks that mix a bare `iota` with other specs that have an
//! explicit right-hand value. Upstream only recognizes a top-level `Ident`
//! named `iota` (not `1 << iota` etc.).

use std::sync::OnceLock;

use guff::ast::{Expr, GenDecl, Spec, ValueSpec};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::IotamixingOptions;

fn is_bare_iota(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(id) if id.name == "iota")
}

fn value_spec_names(spec: &ValueSpec) -> String {
    spec.names
        .iter()
        .map(|n| n.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_const_decl(gd: &GenDecl, report_individual: bool, pending: &mut Vec<(u32, String)>) {
    let mut iota_found = false;
    let mut valued: Vec<&ValueSpec> = Vec::new();

    for spec in &gd.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        if vs.values.iter().any(is_bare_iota) {
            iota_found = true;
            continue;
        }
        if !vs.values.is_empty() {
            valued.push(vs);
        }
    }

    if !iota_found || valued.is_empty() {
        return;
    }

    if report_individual {
        for vs in valued {
            pending.push((
                vs.names
                    .first()
                    .map(|n| n.pos().0 as u32)
                    .unwrap_or(gd.tok_pos.0 as u32),
                format!(
                    "{} is a const with r-val in same const block as iota. keep iotas in separate const blocks",
                    value_spec_names(vs)
                ),
            ));
        }
    } else {
        pending.push((
            gd.tok_pos.0 as u32,
            "iota mixing. keep iotas in separate blocks to consts with r-val".to_string(),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "iotamixing requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<IotamixingOptions>("iotamixing")
        .copied()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            if gd.tok != Some(Token::CONST) {
                continue;
            }
            check_const_decl(gd, opts.report_individual, &mut pending);
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
        name: "iotamixing",
        doc: "checks if iotas are being used in const blocks with other non-iota declarations",
        url: "https://github.com/AdminBenni/iota-mixing",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn graph_ok() {
        validate(&[analyzer()]).expect("iotamixing analyzer graph");
    }

    #[test]
    fn default_is_block_mode() {
        assert!(!IotamixingOptions::default().report_individual);
    }
}
