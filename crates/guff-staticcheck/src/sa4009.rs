//! SA4009 — function argument overwritten before first use.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4009` (simplified).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::{object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn assigns_to_obj(pass: &Pass<'_>, stmt: &Stmt, obj: guff_types::ObjectId) -> bool {
    matches!(stmt, Stmt::AssignStmt(AssignStmt { lhs, .. }) if lhs.iter().any(|e| refers_to(pass, e, obj)))
}

fn reads_obj(pass: &Pass<'_>, stmt: &Stmt, obj: guff_types::ObjectId) -> bool {
    match stmt {
        Stmt::AssignStmt(a) => a.rhs.iter().any(|e| refers_to(pass, e, obj)),
        Stmt::ExprStmt(es) => refers_to(pass, &es.x, obj),
        Stmt::ReturnStmt(r) => r.results.iter().any(|e| refers_to(pass, e, obj)),
        _ => false,
    }
}

fn walk_body(pass: &Pass<'_>, body: &[Stmt], obj: guff_types::ObjectId) -> Option<bool> {
    for stmt in body {
        if assigns_to_obj(pass, stmt, obj) {
            return Some(false);
        }
        if reads_obj(pass, stmt, obj) {
            return Some(true);
        }
        let nested = match stmt {
            Stmt::BlockStmt(b) => walk_body(pass, &b.list, obj),
            Stmt::IfStmt(i) => {
                walk_body(pass, &i.body.list, obj).or_else(|| {
                    i.else_.as_ref().and_then(|e| match &**e {
                        Stmt::BlockStmt(b) => walk_body(pass, &b.list, obj),
                        other => walk_body(pass, std::slice::from_ref(other), obj),
                    })
                })
            }
            Stmt::ForStmt(f) => walk_body(pass, &f.body.list, obj),
            Stmt::RangeStmt(r) => walk_body(pass, &r.body.list, obj),
            _ => None,
        };
        if let Some(used) = nested {
            if used {
                return Some(true);
            }
            return Some(false);
        }
    }
    None
}

fn overwritten_before_use(pass: &Pass<'_>, body: &[Stmt], obj: guff_types::ObjectId) -> bool {
    match walk_body(pass, body, obj) {
        Some(true) => false,
        Some(false) => true,
        None => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4009 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let (params, body) = match node {
            NodeRef::FuncDecl(fd) => (
                fd.ty.params.as_ref().map(|p| &p.list),
                fd.body.as_ref().map(|b| b.list.as_slice()),
            ),
            NodeRef::FuncLit(fl) => (
                fl.ty.params.as_ref().map(|p| &p.list),
                Some(fl.body.list.as_slice()),
            ),
            _ => return,
        };
        let (Some(params), Some(body)) = (params, body) else {
            return;
        };
        for field in params {
            for arg in &field.names {
                if matches!(arg.name.as_str(), "_" | "") {
                    continue;
                }
                let Some(obj) = object_of(pass, arg) else {
                    continue;
                };
                if overwritten_before_use(pass, body, obj) {
                    pending.push((
                        arg.name_pos.0 as u32,
                        format!("argument {} is overwritten before first use", arg.name),
                    ));
                }
            }
        }
    });
    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa4009_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4009",
        doc: "a function argument is overwritten before its first use",
        url: "https://staticcheck.dev/docs/checks/#SA4009",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4009_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4009_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
