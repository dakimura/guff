//! `sigchanyzer` — check misuse of unbuffered signal channels with signal.Notify.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, Spec, ValueSpec};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::expreq::unparen;
use crate::govet_util::{imports_package, is_builtin_named};

fn is_signal_notify(pass: &Pass<'_>, call: &CallExpr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return false;
    };
    if sel.sel.name != "Notify" {
        return false;
    }
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info.uses.get(&sel.sel.id).copied() else {
        return false;
    };
    code_pkg_path(&artifacts.objects, &artifacts.packages, obj).as_deref() == Some("os/signal")
}

fn code_pkg_path(
    objects: &guff_types::arena::ObjectArena,
    packages: &guff_types::arena::PackageArena,
    obj: guff_types::ObjectId,
) -> Option<String> {
    let pkg = obj.pkg(objects)?;
    Some(packages.get(pkg).path().to_string())
}

fn find_decl_rhs<'a>(pass: &'a Pass<'_>, id: &Ident) -> Option<&'a Expr> {
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(gd) = decl else {
                continue;
            };
            for spec in &gd.specs {
                if let Spec::ValueSpec(ValueSpec { names, values, .. }) = spec {
                    for (i, name) in names.iter().enumerate() {
                        if name.id == id.id {
                            return values.get(i);
                        }
                    }
                }
            }
            if let guff::ast::Decl::FuncDecl(f) = decl {
                if let Some(body) = &f.body {
                    if let Some(rhs) = find_in_stmts(&body.list, id) {
                        return Some(rhs);
                    }
                }
            }
        }
    }
    None
}

fn find_in_stmts<'a>(stmts: &'a [guff::ast::Stmt], id: &Ident) -> Option<&'a Expr> {
    for stmt in stmts {
        if let guff::ast::Stmt::AssignStmt(AssignStmt { lhs, rhs, .. }) = stmt {
            for (l, r) in lhs.iter().zip(rhs) {
                if let Expr::Ident(li) = unparen(l) {
                    if li.id == id.id {
                        return Some(r);
                    }
                }
            }
        }
        if let guff::ast::Stmt::BlockStmt(b) = stmt {
            if let Some(rhs) = find_in_stmts(&b.list, id) {
                return Some(rhs);
            }
        }
    }
    None
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    if !imports_package(pass, "os/signal") {
        return Ok(None);
    }
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "sigchanyzer requires inspect analyzer".to_string())?
        .clone();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(CallExpr), pass.files(), |n| {
        let NodeRef::CallExpr(call) = n else {
            return;
        };
        if !is_signal_notify(pass, call) {
            return;
        }
        let chan_decl = match call.args.first() {
            Some(Expr::Ident(id)) => find_decl_rhs(pass, id).and_then(|e| match unparen(e) {
                Expr::CallExpr(c) => Some(c),
                _ => None,
            }),
            Some(Expr::CallExpr(c)) => {
                if is_builtin_named(pass, &c.fun, "make") && c.args.len() == 1 {
                    Some(c)
                } else {
                    None
                }
            }
            _ => None,
        };
        let Some(chan_decl) = chan_decl else {
            return;
        };
        if chan_decl.args.len() != 1 {
            return;
        }
        pending.push((
            call.pos().0 as u32,
            "misuse of unbuffered os.Signal channel as argument to signal.Notify".into(),
        ));
    });
    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "sigchanyzer",
        doc: "check for misuse of unbuffered os.Signal channels with signal.Notify",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/sigchanyzer",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
