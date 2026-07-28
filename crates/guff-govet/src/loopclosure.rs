//! `loopclosure` — check for loop variable capture in closures (pre-Go 1.22).

use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, CallExpr, CaseClause, CommClause, Expr, FuncLit, GoStmt, Ident,
    IncDecStmt, RangeStmt, Stmt,
};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::expreq::unparen;
use crate::govet_util::{file_go_version_before, is_method_named};

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

fn ident_object(pass: &Pass<'_>, e: &Expr) -> Option<guff_types::ObjectId> {
    let Expr::Ident(id) = unparen(e) else {
        return None;
    };
    let info = pass.types_info()?;
    info.uses
        .get(&id.id)
        .copied()
        .or_else(|| info.defs.get(&id.id).and_then(|o| *o))
}

fn lit_stmts(fun: &Expr) -> Option<&[Stmt]> {
    let Expr::FuncLit(FuncLit { body, .. }) = unparen(fun) else {
        return None;
    };
    Some(&body.list)
}

fn for_each_last_stmt(stmts: &[Stmt], on_last: &mut dyn FnMut(&Stmt)) {
    if stmts.is_empty() {
        return;
    }
    match &stmts[stmts.len() - 1] {
        Stmt::IfStmt(s) => {
            let mut cur = s;
            loop {
                for_each_last_stmt(&cur.body.list, on_last);
                match cur.else_.as_deref() {
                    Some(Stmt::BlockStmt(b)) => for_each_last_stmt(&b.list, on_last),
                    Some(Stmt::IfStmt(inner)) => cur = inner,
                    _ => break,
                }
                if !matches!(cur.else_.as_deref(), Some(Stmt::IfStmt(_))) {
                    break;
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

fn report_captured(pass: &Pass<'_>, vars: &[guff_types::ObjectId], stmt: &Stmt) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    walk_stmt(stmt, &mut |id: &Ident| {
        let Some(obj) = ident_object(pass, &Expr::Ident(id.clone())) else {
            return;
        };
        if vars.contains(&obj) {
            out.push((
                id.pos().0 as u32,
                format!("loop variable {} captured by func literal", id.name),
            ));
        }
    });
    out
}

fn walk_stmt(stmt: &Stmt, f: &mut dyn FnMut(&Ident)) {
    match stmt {
        Stmt::BlockStmt(b) => {
            for s in &b.list {
                walk_stmt(s, f);
            }
        }
        Stmt::IfStmt(s) => {
            walk_stmt(&Stmt::BlockStmt(s.body.clone()), f);
            if let Some(e) = &s.else_ {
                walk_stmt(e, f);
            }
        }
        Stmt::ForStmt(s) => walk_stmt(&Stmt::BlockStmt(s.body.clone()), f),
        Stmt::RangeStmt(s) => walk_stmt(&Stmt::BlockStmt(s.body.clone()), f),
        Stmt::SwitchStmt(s) => {
            for cas in &s.body.list {
                if let Stmt::CaseClause(c) = cas {
                    for inner in &c.body {
                        walk_stmt(inner, f);
                    }
                }
            }
        }
        Stmt::TypeSwitchStmt(s) => {
            for cas in &s.body.list {
                if let Stmt::CaseClause(c) = cas {
                    for inner in &c.body {
                        walk_stmt(inner, f);
                    }
                }
            }
        }
        Stmt::SelectStmt(s) => {
            for comm in &s.body.list {
                if let Stmt::CommClause(c) = comm {
                    for inner in &c.body {
                        walk_stmt(inner, f);
                    }
                }
            }
        }
        Stmt::AssignStmt(s) => {
            for e in s.lhs.iter().chain(&s.rhs) {
                walk_expr(e, f);
            }
        }
        Stmt::ExprStmt(s) => walk_expr(&s.x, f),
        Stmt::GoStmt(s) => walk_expr(&Expr::CallExpr(s.call.clone()), f),
        Stmt::DeferStmt(s) => walk_expr(&Expr::CallExpr(s.call.clone()), f),
        Stmt::ReturnStmt(s) => {
            for e in &s.results {
                walk_expr(e, f);
            }
        }
        _ => {}
    }
}

fn walk_expr(expr: &Expr, f: &mut dyn FnMut(&Ident)) {
    match expr {
        Expr::Ident(id) => f(id),
        Expr::SelectorExpr(s) => walk_expr(&s.x, f),
        Expr::CallExpr(c) => {
            walk_expr(&c.fun, f);
            for a in &c.args {
                walk_expr(a, f);
            }
        }
        Expr::FuncLit(lit) => {
            for s in &lit.body.list {
                walk_stmt(s, f);
            }
        }
        Expr::UnaryExpr(u) => walk_expr(&u.x, f),
        Expr::BinaryExpr(b) => {
            walk_expr(&b.x, f);
            walk_expr(&b.y, f);
        }
        Expr::StarExpr(s) => walk_expr(&s.x, f),
        Expr::IndexExpr(i) => {
            walk_expr(&i.x, f);
            walk_expr(&i.index, f);
        }
        Expr::ParenExpr(p) => walk_expr(&p.x, f),
        _ => {}
    }
}

fn errgroup_go_invoke<'a>(pass: &Pass<'_>, call: &'a CallExpr) -> Option<&'a Expr> {
    if !is_method_named(pass, call, "golang.org/x/sync/errgroup", "Group", "Go") {
        return None;
    }
    call.args.first()
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
    if !file_go_version_before(pass, "go1.22") {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "loopclosure requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    // `loop_vars` returns `None` for every other kind — see its match.
    inspect.preorder_typed(node_mask!(RangeStmt, ForStmt), pass.files(), |n| {
        let Some((vars, body)) = loop_vars(pass, n) else {
            return;
        };
        if vars.is_empty() {
            return;
        }
        for_each_last_stmt(&body.list, &mut |last| {
            pending.extend(check_last_stmt(pass, &vars, last));
        });
    });
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
