//! `loopclosure` — check for loop variable capture in closures (pre-Go 1.22).

use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, CallExpr, CaseClause, CommClause, Expr,
    IncDecStmt, Stmt,
};
use guff::node_mask;
use guff::walk::{preorder_prune, stmt_ref, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use guff_analysis::code;

use crate::govet_util::is_method_named;

fn loop_vars<'a>(
    pass: &Pass<'_>,
    n: NodeRef<'a>,
) -> Option<(Vec<guff_types::ObjectId>, &'a BlockStmt)> {
    match n {
        NodeRef::RangeStmt(s) => {
            let mut vars = Vec::new();
            if let Some(key) = &s.key {
                if let Some(obj) = ident_object(pass, key) {
                    vars.push(obj);
                }
            }
            if let Some(val) = &s.value {
                if let Some(obj) = ident_object(pass, val) {
                    vars.push(obj);
                }
            }
            Some((vars, &s.body))
        }
        NodeRef::ForStmt(s) => {
            let mut vars = Vec::new();
            if let Some(Stmt::IncDecStmt(IncDecStmt { x, .. })) = s.post.as_deref() {
                if let Some(obj) = ident_object(pass, x) {
                    vars.push(obj);
                }
            }
            if let Some(Stmt::AssignStmt(AssignStmt { lhs, .. })) = s.post.as_deref() {
                for lhs in lhs {
                    if let Some(obj) = ident_object(pass, lhs) {
                        vars.push(obj);
                    }
                }
            }
            Some((vars, &s.body))
        }
        _ => None,
    }
}

/// `addVar`: `pass.TypesInfo.ObjectOf` of a **bare** identifier. Upstream
/// asserts `expr.(*ast.Ident)` without unparenthesising, so `for (k) = range`
/// contributes no variable.
fn ident_object(pass: &Pass<'_>, e: &Expr) -> Option<guff_types::ObjectId> {
    let Expr::Ident(id) = e else {
        return None;
    };
    let info = pass.types_info()?;
    info.defs
        .get(&id.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&id.id).copied())
}

/// `litStmts`: also a bare type assertion upstream — `go (func() { … })()` is
/// not a function literal to it.
fn lit_stmts(fun: &Expr) -> Option<&[Stmt]> {
    let Expr::FuncLit(lit) = fun else {
        return None;
    };
    Some(&lit.body.list)
}

fn for_each_last_stmt(stmts: &[Stmt], on_last: &mut dyn FnMut(&Stmt)) {
    if stmts.is_empty() {
        return;
    }
    match &stmts[stmts.len() - 1] {
        Stmt::IfStmt(s) => {
            // Upstream's `loop:` — every `else if` body is visited, and a final
            // `else` block ends the chain.
            let mut cur = s;
            loop {
                for_each_last_stmt(&cur.body.list, on_last);
                match cur.else_.as_deref() {
                    Some(Stmt::BlockStmt(b)) => {
                        for_each_last_stmt(&b.list, on_last);
                        break;
                    }
                    Some(Stmt::IfStmt(inner)) => cur = inner,
                    _ => break,
                }
            }
        }
        Stmt::ForStmt(s) => for_each_last_stmt(&s.body.list, on_last),
        Stmt::RangeStmt(s) => for_each_last_stmt(&s.body.list, on_last),
        Stmt::SwitchStmt(s) => {
            for cas in &s.body.list {
                let Stmt::CaseClause(CaseClause { body, .. }) = cas else {
                    continue;
                };
                for_each_last_stmt(body, on_last);
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            for cas in &s.body.list {
                let Stmt::CaseClause(CaseClause { body, .. }) = cas else {
                    continue;
                };
                for_each_last_stmt(body, on_last);
            }
        }
        Stmt::SelectStmt(s) => {
            for comm in &s.body.list {
                let Stmt::CommClause(CommClause { body, .. }) = comm else {
                    continue;
                };
                for_each_last_stmt(body, on_last);
            }
        }
        other => on_last(other),
    }
}

/// `reportCaptured`: `ast.Inspect` over the whole statement, reporting every
/// identifier whose `Uses` entry is one of the loop variables — conditions,
/// declarations, composite literals, labeled statements, sends and `i++`
/// included. (The hand-written walk this replaces descended only into statement
/// bodies and a handful of expression kinds, and cloned each nested body on the
/// way down.)
fn report_captured(pass: &Pass<'_>, vars: &[guff_types::ObjectId], stmt: &Stmt) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let Some(info) = pass.types_info() else {
        return out;
    };
    preorder_prune(stmt_ref(stmt), |n| {
        if let NodeRef::Ident(id) = n {
            if let Some(obj) = info.uses.get(&id.id) {
                if vars.contains(obj) {
                    out.push((
                        id.pos().0 as u32,
                        format!("loop variable {} captured by func literal", id.name),
                    ));
                }
            }
        }
        true
    });
    out
}

fn errgroup_go_invoke<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<&'a Expr> {
    if !is_method_named(pass, call, "golang.org/x/sync/errgroup", "Group", "Go") {
        return None;
    }
    call.args.first()
}

/// `unlabel`: the statement under any number of labels, and whether there was one.
fn unlabel(mut stmt: &Stmt) -> (&Stmt, bool) {
    let mut labeled = false;
    while let Stmt::LabeledStmt(l) = stmt {
        labeled = true;
        stmt = &l.stmt;
    }
    (stmt, labeled)
}

/// `parallelSubtest`: the statements of a `t.Run(name, func(t *testing.T) {…})`
/// literal that follow a `t.Parallel()` on the literal's own `t`, counting only
/// from the last labeled statement (a jump could skip either side of it).
fn parallel_subtest<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Vec<&'a Stmt> {
    let mut stmts = Vec::new();
    if !is_method_named(pass, call, "testing", "T", "Run") {
        return stmts;
    }
    if call.args.len() != 2 {
        return stmts;
    }
    let Expr::FuncLit(lit) = &call.args[1] else {
        return stmts;
    };
    let Some(first) = lit.ty.params.as_ref().and_then(|p| p.list.first()) else {
        return stmts;
    };
    let Some(name) = first.names.first() else {
        return stmts;
    };
    let Some(info) = pass.types_info() else {
        return stmts;
    };
    let Some(t_obj) = info.defs.get(&name.id).and_then(|o| *o) else {
        return stmts;
    };
    let mut after_parallel = false;
    for stmt in &lit.body.list {
        let (stmt, labeled) = unlabel(stmt);
        if labeled {
            stmts.clear();
            after_parallel = false;
        }
        if after_parallel {
            stmts.push(stmt);
            continue;
        }
        let Stmt::ExprStmt(es) = stmt else {
            continue;
        };
        let Expr::CallExpr(pcall) = &es.x else {
            continue;
        };
        if is_method_named(pass, pcall, "testing", "T", "Parallel") {
            let Expr::SelectorExpr(sel) = &*pcall.fun else {
                continue;
            };
            let Expr::Ident(id) = &*sel.x else {
                continue;
            };
            if info.uses.get(&id.id) == Some(&t_obj) {
                after_parallel = true;
            }
        }
    }
    stmts
}

fn check_last_stmt(pass: &Pass<'_>, vars: &[guff_types::ObjectId], last: &Stmt) -> Vec<(u32, String)> {
    let mut pending = Vec::new();
    match last {
        Stmt::GoStmt(s) => {
            if let Some(stmts) = lit_stmts(&s.call.fun) {
                for stmt in stmts {
                    pending.extend(report_captured(pass, vars, stmt));
                }
            }
        }
        Stmt::DeferStmt(s) => {
            if let Some(stmts) = lit_stmts(&s.call.fun) {
                for stmt in stmts {
                    pending.extend(report_captured(pass, vars, stmt));
                }
            }
        }
        Stmt::ExprStmt(s) => {
            if let Expr::CallExpr(call) = &s.x {
                if let Some(arg) = errgroup_go_invoke(pass, call) {
                    if let Some(stmts) = lit_stmts(arg) {
                        for stmt in stmts {
                            pending.extend(report_captured(pass, vars, stmt));
                        }
                    }
                }
            }
        }
        _ => {}
    }
    pending
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "loopclosure requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    // Upstream prunes at each `*ast.File` whose own version is go1.22 or later
    // (`analyzerutil.FileUsesGoVersion`, i.e. `TypesInfo.FileVersions`), so in
    // a package mixing a `//go:build go1.21` file with go1.22+ files only the
    // old file is checked. Walking a one-file subslice of `pass.files()` keeps
    // the inspector's flat-event fast path.
    let files = pass.files();
    for file in files {
        let v = code::effective_file_go_version(pass, file.file_start.0 as u32);
        if code::version_compare(&v, "go1.22") >= 0 {
            continue;
        }
        // `loop_vars` returns `None` for every other kind — see its match.
        inspect.preorder_typed(node_mask!(RangeStmt, ForStmt), std::slice::from_ref(file), |n| {
            let Some((vars, body)) = loop_vars(pass, n) else {
                return;
            };
            if vars.is_empty() {
                return;
            }
            for_each_last_stmt(&body.list, &mut |last| {
                pending.extend(check_last_stmt(pass, &vars, last));
            });
            // Every `t.Run` statement of the body, not just the last one: there is
            // no common way to synchronise parallel subtests.
            for s in &body.list {
                if let Stmt::ExprStmt(es) = s {
                    if let Expr::CallExpr(call) = &es.x {
                        for stmt in parallel_subtest(pass, call) {
                            pending.extend(report_captured(pass, &vars, stmt));
                        }
                    }
                }
            }
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
        name: "loopclosure",
        doc: "check for loop variable capture in closures before Go 1.22",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/loopclosure",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
