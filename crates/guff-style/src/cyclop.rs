//! Port of [`github.com/bkielbasa/cyclop`](https://github.com/bkielbasa/cyclop)
//! (golangci-lint wrapper in `pkg/golinters/cyclop`).
//!
//! Default matches cyclop / golangci-lint: `max-complexity=10` (report when
//! complexity is strictly greater than this). Package-average check is off
//! by default (`package-average=0`).

use std::sync::OnceLock;

use guff::ast::Decl;
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::CyclopOptions;

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

    let options = pass
        .settings::<CyclopOptions>("cyclop")
        .cloned()
        .unwrap_or_default();
    let max_complexity = options.max_complexity;
    let package_average = options.package_average;
    let skip_tests = options.skip_tests;

    let mut pending = Vec::new();
    let mut sum = 0f64;
    let mut count = 0f64;
    let mut pkg_name = String::new();
    let mut pkg_pos = 0u32;

    for file in pass.files() {
        if pkg_name.is_empty() {
            pkg_name = file.name.name.clone();
            // `pkgPos = node.Pos()` where `node` is the `*ast.File`, and
            // `File.Pos()` is the `package` keyword — column 1, not the package
            // name nine columns to its right.
            pkg_pos = file.package.0 as u32;
        }
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            if skip_tests && f.name.name.starts_with("Test") {
                continue;
            }
            let c = complexity(NodeRef::FuncDecl(f));
            count += 1.0;
            sum += c as f64;
            if c > max_complexity {
                pending.push((
                    // `pass.Reportf(node.Pos(), …)` — `node` is the FuncDecl,
                    // so this is the `func` keyword, not the name after it.
                    // `Decl::FuncDecl.pos()` is `d.ty.pos()`, as in go/ast.
                    f.ty.pos().0 as u32,
                    format!(
                        "calculated cyclomatic complexity for function {} is {c}, max is {max_complexity}",
                        f.name.name
                    ),
                ));
            }
        }
    }

    if package_average > 0.0 && count > 0.0 {
        let avg = sum / count;
        if avg > package_average {
            pending.push((
                pkg_pos,
                // Upstream renders both numbers with `%f`, which is six
                // decimal places — `12.000000`, not `12`. Rust's default
                // `Display` for f64 prints the shortest round-tripping form,
                // so the two only agree on values that happen to need six.
                format!(
                    "the average complexity for the package {pkg_name} is {avg:.6}, max is {package_average:.6}"
                ),
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
        name: "cyclop",
        doc: "checks function and package cyclomatic complexity",
        url: "https://github.com/bkielbasa/cyclop",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
