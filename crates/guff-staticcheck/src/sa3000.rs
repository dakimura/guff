//! SA3000 — `TestMain` doesn't call `os.Exit`.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa3000`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, FuncDecl, Ident, Stmt};
use guff_analysis::code::{is_call_to, is_of_type_with_name, object_of, refers_to, stdlib_version, version_compare};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::ObjectId;

fn is_test_main(pass: &Pass<'_>, decl: &FuncDecl) -> bool {
    if decl.name.name != "TestMain" {
        return false;
    }
    let Some(params) = decl.ty.params.as_ref() else {
        return false;
    };
    if params.list.len() != 1 {
        return false;
    }
    let field = &params.list[0];
    if field.names.len() != 1 {
        return false;
    }
    let Some(ty) = field.ty.as_ref() else {
        return false;
    };
    is_of_type_with_name(pass, ty, "*testing.M")
}

fn scan_stmts(
    pass: &Pass<'_>,
    stmts: &[Stmt],
    arg: ObjectId,
    calls_exit: &mut bool,
    calls_run: &mut bool,
) {
    for stmt in stmts {
        match stmt {
            Stmt::ExprStmt(es) => {
                if let Expr::CallExpr(call) = &es.x {
                    check_call(pass, call, arg, calls_exit, calls_run);
                }
            }
            Stmt::BlockStmt(block) => scan_stmts(pass, &block.list, arg, calls_exit, calls_run),
            Stmt::IfStmt(i) => {
                if let Some(init) = &i.init {
                    if let Stmt::ExprStmt(es) = &**init {
                        if let Expr::CallExpr(call) = &es.x {
                            check_call(pass, call, arg, calls_exit, calls_run);
                        }
                    }
                }
                scan_stmts(pass, &i.body.list, arg, calls_exit, calls_run);
                if let Some(else_) = &i.else_ {
                    scan_stmt(pass, else_, arg, calls_exit, calls_run);
                }
            }
            Stmt::ForStmt(f) => {
                if let Some(init) = &f.init {
                    scan_stmt(pass, init, arg, calls_exit, calls_run);
                }
                scan_stmts(pass, &f.body.list, arg, calls_exit, calls_run);
            }
            Stmt::RangeStmt(r) => scan_stmts(pass, &r.body.list, arg, calls_exit, calls_run),
            _ => {}
        }
    }
}

fn scan_stmt(
    pass: &Pass<'_>,
    stmt: &Stmt,
    arg: ObjectId,
    calls_exit: &mut bool,
    calls_run: &mut bool,
) {
    scan_stmts(pass, std::slice::from_ref(stmt), arg, calls_exit, calls_run);
}

fn check_call(
    pass: &Pass<'_>,
    call: &CallExpr,
    arg: ObjectId,
    calls_exit: &mut bool,
    calls_run: &mut bool,
) {
    if is_call_to(pass, call, "os.Exit") {
        *calls_exit = true;
        return;
    }
    let Expr::SelectorExpr(sel) = &*call.fun else {
        return;
    };
    let Expr::Ident(Ident { .. }) = &*sel.x else {
        return;
    };
    if !refers_to(pass, &sel.x, arg) {
        return;
    }
    if sel.sel.name == "Run" {
        *calls_run = true;
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA3000 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let guff::walk::NodeRef::FuncDecl(decl) = node else {
            return;
        };
        if !is_test_main(pass, decl) {
            return;
        }
        if version_compare(&stdlib_version(pass, decl.name.name_pos.0 as u32), "go1.15") >= 0 {
            return;
        }
        let Some(body) = &decl.body else {
            return;
        };
        let Some(arg) = object_of(
            pass,
            &decl.ty.params.as_ref().unwrap().list[0].names[0],
        ) else {
            return;
        };
        let mut calls_exit = false;
        let mut calls_run = false;
        scan_stmts(pass, &body.list, arg, &mut calls_exit, &mut calls_run);
        if !calls_exit && calls_run {
            pending.push((
                decl.name.name_pos.0 as u32,
                "TestMain should call os.Exit to set exit code".into(),
            ));
        }
    });

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

fn sa3000_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA3000",
        doc: "TestMain doesn't call os.Exit",
        url: "https://staticcheck.dev/docs/checks/#SA3000",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA3000 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa3000_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa3000_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
