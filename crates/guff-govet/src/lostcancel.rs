//! `lostcancel` — check for missing calls to context cancel functions.

use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, BranchStmt, Expr, Ident, ReturnStmt, Spec, Stmt, ValueSpec,
};
use guff::token::Token;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::expreq::unparen;
use crate::govet_util::imports_package;

const WITH_FUNCS: &[&str] = &[
    "WithCancel",
    "WithCancelCause",
    "WithTimeout",
    "WithTimeoutCause",
    "WithDeadline",
    "WithDeadlineCause",
];

fn is_context_with_cancel(pass: &Pass<'_>, e: &Expr) -> Option<String> {
    let Expr::SelectorExpr(sel) = unparen(e) else {
        return None;
    };
    if !WITH_FUNCS.contains(&sel.sel.name.as_str()) {
        return None;
    }
    let Expr::Ident(pkg) = sel.x.as_ref() else {
        return None;
    };
    if pkg.name != "context" {
        return None;
    }
    let info = pass.types_info()?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    let obj = info.uses.get(&pkg.id).copied()?;
    match artifacts.objects.get(obj) {
        guff_types::arena::ObjectData::PkgName(pn) => {
            if artifacts.packages.get(pn.imported()).path() != "context" {
                return None;
            }
        }
        _ => {}
    }
    Some(sel.sel.name.clone())
}

struct CancelDef {
    cancel_id: u32,
    cancel_obj: Option<guff_types::ObjectId>,
    stmt_pos: u32,
    with_name: String,
    /// Assigned to `_` — always a leak regardless of later uses.
    discarded: bool,
}

fn collect_cancel_defs(pass: &Pass<'_>, body: &BlockStmt) -> Vec<CancelDef> {
    let mut out = Vec::new();
    // Nested FuncLits are checked as their own scopes via
    // [`collect_func_lit_bodies`] — do not pull their defs into the outer fn.
    for stmt in &body.list {
        if let Some(def) = cancel_from_stmt(pass, stmt) {
            out.push(def);
        }
        match stmt {
            Stmt::BlockStmt(b) => out.extend(collect_cancel_defs(pass, b)),
            Stmt::IfStmt(s) => {
                out.extend(collect_cancel_defs(pass, &s.body));
                if let Some(e) = &s.else_ {
                    collect_cancel_from_else(pass, e, &mut out);
                }
            }
            Stmt::ForStmt(s) => out.extend(collect_cancel_defs(pass, &s.body)),
            Stmt::RangeStmt(s) => out.extend(collect_cancel_defs(pass, &s.body)),
            _ => {}
        }
    }
    out
}

fn collect_cancel_from_else(pass: &Pass<'_>, stmt: &Stmt, out: &mut Vec<CancelDef>) {
    match stmt {
        Stmt::BlockStmt(b) => out.extend(collect_cancel_defs(pass, b)),
        Stmt::IfStmt(s) => {
            out.extend(collect_cancel_defs(pass, &s.body));
            if let Some(e) = &s.else_ {
                collect_cancel_from_else(pass, e, out);
            }
        }
        _ => {}
    }
}

fn collect_func_lit_bodies<'a>(body: &'a BlockStmt, out: &mut Vec<&'a BlockStmt>) {
    fn walk_stmts<'a>(stmts: &'a [Stmt], out: &mut Vec<&'a BlockStmt>) {
        for stmt in stmts {
            walk_stmt(stmt, out);
        }
    }
    fn walk_stmt<'a>(stmt: &'a Stmt, out: &mut Vec<&'a BlockStmt>) {
        match stmt {
            Stmt::BlockStmt(b) => walk_stmts(&b.list, out),
            Stmt::IfStmt(s) => {
                walk_stmts(&s.body.list, out);
                if let Some(e) = &s.else_ {
                    walk_stmt(e, out);
                }
            }
            Stmt::ForStmt(s) => walk_stmts(&s.body.list, out),
            Stmt::RangeStmt(s) => walk_stmts(&s.body.list, out),
            Stmt::ExprStmt(s) => walk_expr(&s.x, out),
            Stmt::AssignStmt(s) => {
                for e in s.lhs.iter().chain(&s.rhs) {
                    walk_expr(e, out);
                }
            }
            Stmt::ReturnStmt(ReturnStmt { results, .. }) => {
                for e in results {
                    walk_expr(e, out);
                }
            }
            Stmt::DeferStmt(s) => walk_call(&s.call, out),
            Stmt::GoStmt(s) => walk_call(&s.call, out),
            _ => {}
        }
    }
    fn walk_call<'a>(call: &'a guff::ast::CallExpr, out: &mut Vec<&'a BlockStmt>) {
        walk_expr(&call.fun, out);
        for a in &call.args {
            walk_expr(a, out);
        }
    }
    fn walk_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a BlockStmt>) {
        match expr {
            Expr::FuncLit(l) => {
                out.push(&l.body);
                walk_stmts(&l.body.list, out);
            }
            Expr::CallExpr(c) => walk_call(c, out),
            Expr::SelectorExpr(s) => walk_expr(&s.x, out),
            Expr::ParenExpr(p) => walk_expr(&p.x, out),
            Expr::UnaryExpr(u) => walk_expr(&u.x, out),
            Expr::BinaryExpr(b) => {
                walk_expr(&b.x, out);
                walk_expr(&b.y, out);
            }
            Expr::IndexExpr(i) => {
                walk_expr(&i.x, out);
                walk_expr(&i.index, out);
            }
            Expr::StarExpr(s) => walk_expr(&s.x, out),
            Expr::KeyValueExpr(kv) => {
                walk_expr(&kv.key, out);
                walk_expr(&kv.value, out);
            }
            Expr::CompositeLit(c) => {
                for elt in &c.elts {
                    walk_expr(elt, out);
                }
            }
            _ => {}
        }
    }
    walk_stmts(&body.list, out);
}

fn cancel_from_stmt(pass: &Pass<'_>, stmt: &Stmt) -> Option<CancelDef> {
    match stmt {
        Stmt::AssignStmt(AssignStmt { lhs, rhs, .. }) if lhs.len() >= 2 && !rhs.is_empty() => {
            cancel_from_rhs(pass, &lhs[1], &rhs[0], stmt.pos().0 as u32)
        }
        Stmt::DeclStmt(ds) => {
            let guff::ast::DeclStmt { decl, .. } = ds;
            let guff::ast::Decl::GenDecl(gd) = &decl else {
                return None;
            };
            let spec = gd.specs.first()?;
            let Spec::ValueSpec(ValueSpec { names, values, .. }) = spec else {
                return None;
            };
            if names.len() < 2 || values.is_empty() {
                return None;
            }
            cancel_from_rhs(pass, &Expr::Ident(names[1].clone()), &values[0], stmt.pos().0 as u32)
        }
        _ => None,
    }
}

fn cancel_from_rhs(pass: &Pass<'_>, cancel: &Expr, rhs: &Expr, stmt_pos: u32) -> Option<CancelDef> {
    let Expr::CallExpr(call) = unparen(rhs) else {
        return None;
    };
    let with_name = is_context_with_cancel(pass, &call.fun)?;
    let Expr::Ident(id) = unparen(cancel) else {
        return None;
    };
    if id.name == "_" {
        return Some(CancelDef {
            cancel_id: id.id,
            cancel_obj: None,
            stmt_pos: id.pos().0 as u32,
            with_name,
            discarded: true,
        });
    }
    let obj = pass
        .types_info()?
        .defs
        .get(&id.id)
        .and_then(|o| *o)
        .or_else(|| pass.types_info()?.uses.get(&id.id).copied());
    Some(CancelDef {
        cancel_id: id.id,
        cancel_obj: obj,
        stmt_pos,
        with_name,
        discarded: false,
    })
}

fn walk_stmts(pass: &Pass<'_>, stmts: &[Stmt], f: &mut dyn FnMut(&Stmt)) {
    for stmt in stmts {
        f(stmt);
        match stmt {
            Stmt::BlockStmt(b) => walk_stmts(pass, &b.list, f),
            Stmt::IfStmt(s) => {
                walk_stmts(pass, &s.body.list, f);
                if let Some(e) = &s.else_ {
                    walk_stmt(pass, e, f);
                }
            }
            Stmt::ForStmt(s) => walk_stmts(pass, &s.body.list, f),
            Stmt::RangeStmt(s) => walk_stmts(pass, &s.body.list, f),
            _ => {}
        }
    }
}

fn walk_stmt(pass: &Pass<'_>, stmt: &Stmt, f: &mut dyn FnMut(&Stmt)) {
    f(stmt);
    if let Stmt::BlockStmt(b) = stmt {
        walk_stmts(pass, &b.list, f);
    }
}

fn cancel_used_in_body(pass: &Pass<'_>, body: &BlockStmt, cancel: &CancelDef) -> bool {
    let mut used = false;
    walk_stmts(pass, &body.list, &mut |stmt| {
        walk_expr_in_stmt(stmt, &mut |id| {
            if id.id == cancel.cancel_id {
                used = true;
            }
            if let Some(obj) = cancel.cancel_obj {
                if let Some(info) = pass.types_info() {
                    if info.uses.get(&id.id) == Some(&obj) {
                        used = true;
                    }
                }
            }
        });
    });
    used
}

fn walk_expr_in_stmt(stmt: &Stmt, f: &mut dyn FnMut(&Ident)) {
    match stmt {
        Stmt::ExprStmt(s) => walk_expr(&s.x, f),
        Stmt::AssignStmt(s) => {
            for e in s.lhs.iter().chain(&s.rhs) {
                walk_expr(e, f);
            }
        }
        Stmt::ReturnStmt(ReturnStmt { results, .. }) => {
            for e in results {
                walk_expr(e, f);
            }
        }
        Stmt::DeferStmt(s) => walk_expr(&Expr::CallExpr(s.call.clone()), f),
        Stmt::GoStmt(s) => walk_expr(&Expr::CallExpr(s.call.clone()), f),
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
        Expr::FuncLit(l) => {
            for s in &l.body.list {
                walk_expr_in_stmt(s, f);
            }
        }
        _ => {}
    }
}

fn has_return_without_use(pass: &Pass<'_>, body: &BlockStmt, cancel: &CancelDef) -> bool {
    if cancel_used_in_body(pass, body, cancel) {
        return false;
    }
    let mut has_return = false;
    walk_stmts(pass, &body.list, &mut |stmt| {
        if matches!(
            stmt,
            Stmt::ReturnStmt(_) | Stmt::BranchStmt(BranchStmt { tok: Token::RETURN, .. })
        ) {
            has_return = true;
        }
    });
    has_return
}

fn check_function(pass: &Pass<'_>, body: &BlockStmt) -> Vec<(u32, String)> {
    let mut pending = Vec::new();
    for def in collect_cancel_defs(pass, body) {
        if def.discarded {
            pending.push((
                def.stmt_pos,
                format!(
                    "the cancel function returned by context.{} should be called, not discarded, to avoid a context leak",
                    def.with_name
                ),
            ));
            continue;
        }
        if has_return_without_use(pass, body, &def) {
            pending.push((
                def.stmt_pos,
                "the cancel function is not used on all paths (possible context leak)".to_string(),
            ));
        }
    }
    pending
}

fn check_function_tree(pass: &Pass<'_>, body: &BlockStmt) -> Vec<(u32, String)> {
    let mut pending = check_function(pass, body);
    let mut lits = Vec::new();
    collect_func_lit_bodies(body, &mut lits);
    for lit in lits {
        pending.extend(check_function(pass, lit));
    }
    pending
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "context") {
        return Ok(None);
    }
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            if let Some(body) = &f.body {
                pending.extend(check_function_tree(pass, body));
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
        name: "lostcancel",
        doc: "check for missing calls to context cancel functions",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/lostcancel",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}
