//! Port of [`github.com/fzipp/gocyclo`](https://github.com/fzipp/gocyclo)
//! (golangci-lint wrapper in `pkg/golinters/gocyclo`).
//!
//! Default matches golangci-lint: `min-complexity=30`.
//!
//! DEFERRED: `gocyclo:ignore` directive support.

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, Spec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GocycloOptions;

fn recv_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_string(&s.x)),
        Expr::IndexExpr(i) => recv_string(&i.x),
        Expr::IndexListExpr(i) => recv_string(&i.x),
        _ => "BADRECV".into(),
    }
}

fn func_name(fn_: &FuncDecl) -> String {
    if let Some(recv) = &fn_.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_string(ty), fn_.name.name);
            }
        }
    }
    fn_.name.name.clone()
}

fn format_code(code: &str) -> String {
    if code.contains('`') {
        code.to_string()
    } else {
        format!("`{code}`")
    }
}

fn complexity(root: NodeRef<'_>) -> usize {
    let mut complexity = 1usize;
    walk::inspect(root, |n| {
        let Some(n) = n else {
            return true;
        };
        match n {
            NodeRef::IfStmt(_) | NodeRef::ForStmt(_) | NodeRef::RangeStmt(_) => {
                complexity += 1;
            }
            NodeRef::CaseClause(c) if !c.list.is_empty() => {
                complexity += 1;
            }
            NodeRef::CommClause(c) if c.comm.is_some() => {
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

fn report_if_high(
    name: &str,
    pos: u32,
    root: NodeRef<'_>,
    min_complexity: usize,
    pending: &mut Vec<(u32, String)>,
) {
    let c = complexity(root);
    if c > min_complexity {
        pending.push((
            pos,
            format!(
                "cyclomatic complexity {c} of func {} is high (> {min_complexity})",
                format_code(name)
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gocyclo requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GocycloOptions>("gocyclo")
        .copied()
        .unwrap_or_default();
    let min_complexity = options.min_complexity;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(f) => {
                    report_if_high(
                        &func_name(f),
                        f.name.name_pos.0 as u32,
                        NodeRef::FuncDecl(f),
                        min_complexity,
                        &mut pending,
                    );
                }
                Decl::GenDecl(g) => {
                    for spec in &g.specs {
                        let Spec::ValueSpec(vs) = spec else {
                            continue;
                        };
                        for value in &vs.values {
                            let Expr::FuncLit(lit) = value else {
                                continue;
                            };
                            let name = vs
                                .names
                                .first()
                                .map(|n| n.name.as_str())
                                .unwrap_or("<func lit>");
                            let pos = vs
                                .names
                                .first()
                                .map(|n| n.name_pos.0 as u32)
                                .unwrap_or(lit.ty.pos().0 as u32);
                            report_if_high(name, pos, NodeRef::FuncLit(lit), min_complexity, &mut pending);
                        }
                    }
                }
                _ => {}
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
        name: "gocyclo",
        doc: "computes and checks the cyclomatic complexity of functions",
        url: "https://github.com/fzipp/gocyclo",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
