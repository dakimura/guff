//! `httpresponse` — check for mistakes using HTTP responses.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BlockStmt, CallExpr, DeferStmt, Expr, Stmt};
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::{ObjectData, TypeData};

use crate::expreq::unparen;
use crate::govet_util::{
    imports_package, is_type_named, root_ident, static_callee, tuple_len_of, tuple_type_at,
};

fn is_http_response_error_call(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Some(obj) = static_callee(pass, call) else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let ObjectData::Func(_) = artifacts.objects.get(obj) else {
        return false;
    };
    let pkg_path = obj
        .pkg(&artifacts.objects)
        .map(|p| artifacts.packages.get(p).path().to_string());
    if pkg_path.as_deref() == Some("net/http") {
        return true;
    }
    let Some(sig) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let results = guff_types::signature::signature_results(&artifacts.types, sig);
    if tuple_len_of(pass, results) != 2 {
        return false;
    }
    let Some(r0) = tuple_type_at(pass, results, 0) else {
        return false;
    };
    let r0_ok = if is_type_named(pass, r0, "net/http", "Response") {
        true
    } else {
        let u = r0.underlying(&artifacts.types);
        if let TypeData::Pointer(p) = artifacts.types.get(u) {
            is_type_named(pass, p.elem(), "net/http", "Response")
        } else {
            false
        }
    };
    if !r0_ok {
        return false;
    }
    let Some(r1) = tuple_type_at(pass, results, 1) else {
        return false;
    };
    let r1u = r1.underlying(&artifacts.types);
    if !matches!(artifacts.types.get(r1u), TypeData::Interface(_)) {
        return false;
    }
    true
}

fn same_ident(pass: &Pass<'_>, a: &guff::ast::Ident, b: &guff::ast::Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return a.name == b.name;
    };
    let oa = info
        .defs
        .get(&a.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&a.id).copied());
    let ob = info
        .defs
        .get(&b.id)
        .and_then(|o| *o)
        .or_else(|| info.uses.get(&b.id).copied());
    oa == ob
}

fn walk_block(pass: &Pass<'_>, stmts: &[Stmt]) -> Vec<(u32, String)> {
    let mut pending = Vec::new();
    for i in 0..stmts.len().saturating_sub(1) {
        let Stmt::AssignStmt(assign) = &stmts[i] else {
            continue;
        };
        let Stmt::DeferStmt(defer) = &stmts[i + 1] else {
            continue;
        };
        let call = match assign.rhs.first() {
            Some(Expr::CallExpr(c)) => c,
            _ => continue,
        };
        if !is_http_response_error_call(pass, call) {
            continue;
        }
        let Some(resp) = root_ident(&assign.lhs[0]) else {
            continue;
        };
        let Some(root) = root_ident(&defer.call.fun) else {
            continue;
        };
        if same_ident(pass, resp, root) {
            pending.push((
                root.pos().0 as u32,
                format!("using {} before checking for errors", resp.name),
            ));
        }
    }
    for stmt in stmts {
        match stmt {
            Stmt::BlockStmt(BlockStmt { list, .. }) => pending.extend(walk_block(pass, list)),
            Stmt::IfStmt(s) => {
                pending.extend(walk_block(pass, &s.body.list));
                if let Some(Stmt::BlockStmt(b)) = s.else_.as_deref() {
                    pending.extend(walk_block(pass, &b.list));
                }
            }
            _ => {}
        }
    }
    pending
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "net/http") {
        return Ok(None);
    }
    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            if let Some(body) = &f.body {
                pending.extend(walk_block(pass, &body.list));
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
        name: "httpresponse",
        doc: "check for mistakes using HTTP responses",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/httpresponse",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![],
        fact_types: vec![],
    })
}
