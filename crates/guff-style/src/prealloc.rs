//! Port of [`github.com/alexkohler/prealloc`](https://github.com/alexkohler/prealloc)
//! (golangci-lint wrapper in `pkg/golinters/prealloc`).
//!
//! Defaults match golangci-lint: `simple=true`, `range-loops=true`, `for-loops=false`.

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Spec, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::PreallocOptions;

struct SliceDecl {
    name: String,
    pos: u32,
}

fn is_slice_type(ty: &Expr, array_types: &[String]) -> bool {
    match ty {
        Expr::ArrayType(a) if a.len.is_none() => true,
        Expr::Ident(id) => array_types.iter().any(|n| n == &id.name),
        _ => false,
    }
}

fn collect_array_type_aliases(stmt: &Stmt, array_types: &mut Vec<String>) {
    let Stmt::DeclStmt(ds) = stmt else {
        return;
    };
    let Decl::GenDecl(gen) = &ds.decl else {
        return;
    };
    if gen.tok != Some(Token::TYPE) {
        return;
    }
    for spec in &gen.specs {
        let Spec::TypeSpec(ts) = spec else {
            continue;
        };
        if matches!(&ts.ty, Expr::ArrayType(a) if a.len.is_none()) {
            array_types.push(ts.name.name.clone());
        }
    }
}

fn collect_slice_vars(stmt: &Stmt, array_types: &[String], out: &mut Vec<SliceDecl>) {
    let Stmt::DeclStmt(ds) = stmt else {
        return;
    };
    let Decl::GenDecl(gen) = &ds.decl else {
        return;
    };
    if gen.tok != Some(Token::VAR) {
        return;
    }
    for spec in &gen.specs {
        let Spec::ValueSpec(vs) = spec else {
            continue;
        };
        let Some(ty) = &vs.ty else {
            continue;
        };
        if !is_slice_type(ty, array_types) {
            continue;
        }
        for name in &vs.names {
            out.push(SliceDecl {
                name: name.name.clone(),
                pos: gen.tok_pos.0 as u32,
            });
        }
    }
}

fn loop_has_early_exit(body: &[Stmt]) -> bool {
    for stmt in body {
        if let Stmt::IfStmt(ifs) = stmt {
            for s in &ifs.body.list {
                if matches!(s, Stmt::BranchStmt(_) | Stmt::ReturnStmt(_)) {
                    return true;
                }
            }
        }
    }
    false
}

fn loop_resets_slice(body: &[Stmt], name: &str) -> bool {
    for stmt in body {
        match stmt {
            Stmt::AssignStmt(asgn) => {
                for (lhs, rhs) in asgn.lhs.iter().zip(asgn.rhs.iter()) {
                    let Expr::Ident(id) = lhs else {
                        continue;
                    };
                    if id.name != name {
                        continue;
                    }
                    // `s = nil` / `s = s[:0]` / `s = someOther` — batch is rebuilt.
                    if matches!(rhs, Expr::Ident(r) if r.name == "nil") {
                        return true;
                    }
                    if matches!(rhs, Expr::SliceExpr(_)) {
                        return true;
                    }
                }
            }
            Stmt::IfStmt(ifs) => {
                if loop_resets_slice(&ifs.body.list, name) {
                    return true;
                }
                if let Some(else_) = &ifs.else_ {
                    if let Stmt::BlockStmt(b) = else_.as_ref() {
                        if loop_resets_slice(&b.list, name) {
                            return true;
                        }
                    } else if let Stmt::IfStmt(_) = else_.as_ref() {
                        if loop_resets_slice(std::slice::from_ref(else_.as_ref()), name) {
                            return true;
                        }
                    }
                }
            }
            Stmt::BlockStmt(b) => {
                if loop_resets_slice(&b.list, name) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn hints_from_loop(
    body: &[Stmt],
    decls: &[SliceDecl],
    simple: bool,
    pending: &mut Vec<(u32, String)>,
) {
    if simple && loop_has_early_exit(body) {
        return;
    }
    // One body walk per decl name (not once per append site).
    let resets: Vec<bool> = decls
        .iter()
        .map(|d| loop_resets_slice(body, &d.name))
        .collect();
    for stmt in body {
        let Stmt::AssignStmt(asgn) = stmt else {
            continue;
        };
        for expr in &asgn.rhs {
            let Expr::CallExpr(call) = expr else {
                continue;
            };
            let Expr::Ident(fun) = &*call.fun else {
                continue;
            };
            if fun.name != "append" {
                continue;
            }
            for lhs in &asgn.lhs {
                let Expr::Ident(lhs_id) = lhs else {
                    continue;
                };
                for (decl, &resets_slice) in decls.iter().zip(resets.iter()) {
                    if decl.name != lhs_id.name {
                        continue;
                    }
                    // Upstream alexkohler/prealloc effectively ignores slices that
                    // are cleared mid-loop (`s = nil`); preallocating once at decl
                    // does not help across batch resets (grafana consolidation).
                    if resets_slice {
                        continue;
                    }
                    pending.push((
                        decl.pos,
                        format!("Consider preallocating {}", decl.name),
                    ));
                }
            }
        }
    }
}

fn check_func_body(
    body: &[Stmt],
    array_types: &mut Vec<String>,
    options: PreallocOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let mut slice_decls: Vec<SliceDecl> = Vec::new();
    for stmt in body {
        collect_array_type_aliases(stmt, array_types);
        collect_slice_vars(stmt, array_types, &mut slice_decls);
        match stmt {
            Stmt::RangeStmt(r) if options.range_loops && !slice_decls.is_empty() => {
                hints_from_loop(&r.body.list, &slice_decls, options.simple, pending);
            }
            Stmt::ForStmt(f) if options.for_loops && !slice_decls.is_empty() => {
                hints_from_loop(&f.body.list, &slice_decls, options.simple, pending);
            }
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "prealloc requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<PreallocOptions>("prealloc")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    let mut array_types = Vec::new();
    for file in pass.files() {
        // Type aliases at file scope.
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::TypeSpec(ts) = n {
                if matches!(&ts.ty, Expr::ArrayType(a) if a.len.is_none()) {
                    array_types.push(ts.name.name.clone());
                }
            }
            true
        });
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::FuncDecl(f) = n {
                if let Some(body) = &f.body {
                    check_func_body(&body.list, &mut array_types, options, &mut pending);
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
        name: "prealloc",
        doc: "Find slice declarations that could potentially be pre-allocated",
        url: "https://github.com/alexkohler/prealloc",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
