//! Port of [`github.com/bkielbasa/cyclop`](https://github.com/bkielbasa/cyclop)
//! (golangci-lint wrapper in `pkg/golinters/cyclop`).
//!
//! Default matches cyclop / golangci-lint: `max-complexity=10` (report when
//! complexity is strictly greater than this). Package-average check is off
//! by default (`package-average=0`).
//!
//! DEFERRED: `linters.settings.cyclop` wiring (`max-complexity`,
//! `package-average`); `skipTests` flag.

use std::sync::OnceLock;

use guff::ast::Decl;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// cyclop / golangci-lint default for `max-complexity`.
const MAX_COMPLEXITY: usize = 10;

fn complexity(root: NodeRef<'_>) -> usize {
    let mut complexity = 0usize;
    walk::inspect(root, |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::FuncDecl(_)
            | NodeRef::IfStmt(_)
            | NodeRef::ForStmt(_)
            | NodeRef::RangeStmt(_)
            | NodeRef::CaseClause(_)
            | NodeRef::CommClause(_) => {
                complexity += 1;
            }
            NodeRef::BinaryExpr(b) if b.op == Token::LAND || b.op == Token::LOR => {
                complexity += 1;
            }
            _ => {}
        }
        true
    });
    complexity
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "cyclop requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let c = complexity(NodeRef::FuncDecl(f));
            if c > MAX_COMPLEXITY {
                pending.push((
                    f.name.name_pos.0 as u32,
                    format!(
                        "calculated cyclomatic complexity for function {} is {c}, max is {MAX_COMPLEXITY}",
                        f.name.name
                    ),
                ));
            }
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
        name: "cyclop",
        doc: "checks function and package cyclomatic complexity",
        url: "https://github.com/bkielbasa/cyclop",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
