//! SA1023 — modifying the buffer in an `io.Writer` implementation.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa1023`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, FuncDecl, IndexExpr, Stmt};
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::arena::TypeData;
use guff_types::basic::BasicKind;
use guff_types::signature::{signature_params, signature_recv, signature_results};
use guff_types::slice::slice_elem;
use guff_types::tuple::tuple_at;

const MSG: &str = "io.Writer.Write must not modify the provided buffer, not even temporarily";

fn is_io_writer_write(pass: &Pass<'_>, decl: &FuncDecl) -> Option<guff_types::arena::ObjectId> {
    if decl.name.name != "Write" {
        return None;
    }
    let obj = object_of(pass, &decl.name)?;
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    if signature_recv(&artifacts.types, obj.typ(&artifacts.objects)?).is_none() {
        return None;
    }
    let sig = obj.typ(&artifacts.objects)?;
    let params = signature_params(&artifacts.types, sig)?;
    let param = tuple_at(&artifacts.types, params, 0);
    let ptyp = param.typ(&artifacts.objects)?;
    let elem = slice_elem(&artifacts.types, ptyp);
    let TypeData::Basic(b) = artifacts.types.get(elem) else {
        return None;
    };
    if !matches!(b.kind(), BasicKind::Uint8) {
        return None;
    }
    let results = signature_results(&artifacts.types, sig)?;
    if guff_types::tuple::tuple_len(&artifacts.types, Some(results)) != 2 {
        return None;
    }
    Some(obj)
}

fn modifies_buf_param(pass: &Pass<'_>, body: &[Stmt], buf: guff_types::arena::ObjectId) -> bool {
    for stmt in body {
        match stmt {
            Stmt::AssignStmt(AssignStmt { lhs, rhs, .. }) => {
                for (l, r) in lhs.iter().zip(rhs.iter()) {
                    if modifies_slice(pass, l, buf) {
                        return true;
                    }
                    if let Expr::CallExpr(call) = r {
                        if call_name(pass, &call.fun).as_deref() == Some("append")
                            && call.args.first().is_some_and(|a| refers_to(pass, a, buf))
                        {
                            return true;
                        }
                    }
                }
            }
            Stmt::ExprStmt(es) => {
                if let Expr::CallExpr(call) = &es.x {
                    if call_name(pass, &call.fun).as_deref() == Some("append")
                        && call.args.first().is_some_and(|a| refers_to(pass, a, buf))
                    {
                        return true;
                    }
                }
            }
            Stmt::BlockStmt(b) => {
                if modifies_buf_param(pass, &b.list, buf) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn modifies_slice(pass: &Pass<'_>, expr: &Expr, buf: guff_types::arena::ObjectId) -> bool {
    match expr {
        Expr::IndexExpr(IndexExpr { x, .. }) => refers_to(pass, x, buf),
        _ => false,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA1023 requires inspect analyzer".to_string())?
        .clone();

    let mut pending = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::FuncDecl(decl) = node else {
            return;
        };
        let Some(write_fn) = is_io_writer_write(pass, decl) else {
            return;
        };
        let Some(body) = &decl.body else {
            return;
        };
        let artifacts = pass.pkg().type_artifacts.as_ref().unwrap();
        let sig = write_fn.typ(&artifacts.objects).unwrap();
        let params = signature_params(&artifacts.types, sig).unwrap();
        let buf_param = tuple_at(&artifacts.types, params, 0);
        if !modifies_buf_param(pass, &body.list, buf_param) {
            return;
        }
        pending.push((decl.name.name_pos.0 as u32, MSG.to_string()));
    });

    for (pos, msg) in pending {
        pass.reportf(pos, msg);
    }
    Ok(None)
}

fn sa1023_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA1023",
        doc: "modifying the buffer in an io.Writer implementation",
        url: "https://staticcheck.dev/docs/checks/#SA1023",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// SA1023 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa1023_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa1023_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
