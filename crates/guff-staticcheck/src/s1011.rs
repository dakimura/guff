//! S1011 — use a single append to concatenate two slices.
//!
//! Port of `honnef.co/go/tools/simple/s1011`.

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, IndexExpr, RangeStmt, Stmt};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, object_of, refers_to};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::TypeId;

fn expr_type(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    pass.types_info()?.types.get(&expr.id()).map(|tv| tv.typ)
}

fn render_type(pass: &Pass<'_>, typ: TypeId) -> Option<String> {
    let a = pass.pkg().type_artifacts.as_ref()?;
    Some(guff_types::typestring::type_string(
        &a.types,
        &a.objects,
        &a.packages,
        typ,
        None,
    ))
}

fn same_key(pass: &Pass<'_>, a: &Ident, b: &Ident) -> bool {
    match (object_of(pass, a), object_of(pass, b)) {
        (Some(oa), Some(ob)) => oa == ob,
        _ => a.name == b.name,
    }
}

fn same_expr(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(ia), Expr::Ident(ib)) => same_key(pass, ia, ib),
        _ => a.id() == b.id(),
    }
}

fn is_append_to_lhs(pass: &Pass<'_>, call: &CallExpr, lhs: &Expr) -> bool {
    is_call_to(pass, call, "append") && call.args.len() == 2 && same_expr(pass, &call.args[0], lhs)
}

fn check_append_loop(pass: &Pass<'_>, rs: &RangeStmt) -> Option<()> {
    let key = rs.key.as_ref().and_then(|e| match e {
        Expr::Ident(id) => Some(id),
        _ => None,
    })?;
    let x = &rs.x;
    let body = &rs.body.list;

    let (lhs, val_obj, idx_obj) = if let Some(val) = rs.value.as_ref().and_then(|e| match e {
        Expr::Ident(id) => Some(id),
        _ => None,
    }) {
        if key.name != "_" || body.len() != 1 {
            return None;
        }
        let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = &body[0] else {
            return None;
        };
        if tok != &Some(Token::ASSIGN) || lhs.len() != 1 || rhs.len() != 1 {
            return None;
        }
        let Expr::CallExpr(call) = &rhs[0] else {
            return None;
        };
        if !is_append_to_lhs(pass, call, &lhs[0]) {
            return None;
        }
        let Expr::Ident(arg_val) = &call.args[1] else {
            return None;
        };
        if !same_key(pass, arg_val, val) {
            return None;
        }
        let val_obj = object_of(pass, val)?;
        if refers_to(pass, &lhs[0], val_obj) {
            return None;
        }
        (&lhs[0], Some(val_obj), None)
    } else if body.len() == 1 {
        let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = &body[0] else {
            return None;
        };
        if tok != &Some(Token::ASSIGN) || lhs.len() != 1 || rhs.len() != 1 {
            return None;
        };
        let Expr::CallExpr(call) = &rhs[0] else {
            return None;
        };
        let Expr::IndexExpr(IndexExpr { x: ix, index, .. }) = &call.args[1] else {
            return None;
        };
        if !same_expr(pass, ix, x) {
            return None;
        }
        let Expr::Ident(idx) = &**index else {
            return None;
        };
        if !same_key(pass, idx, key) || !is_append_to_lhs(pass, call, &lhs[0]) {
            return None;
        }
        let idx_obj = object_of(pass, idx)?;
        if refers_to(pass, &lhs[0], idx_obj) {
            return None;
        }
        (&lhs[0], None, Some(idx_obj))
    } else if body.len() == 2 {
        let Stmt::AssignStmt(first) = &body[0] else {
            return None;
        };
        let Stmt::AssignStmt(second) = &body[1] else {
            return None;
        };
        if !matches!(first.tok, Some(Token::DEFINE)) || first.lhs.len() != 1 || first.rhs.len() != 1 {
            return None;
        };
        let val = match &first.lhs[0] {
            Expr::Ident(id) => id,
            _ => return None,
        };
        let Expr::IndexExpr(IndexExpr { x: ix, index, .. }) = &first.rhs[0] else {
            return None;
        };
        if !same_expr(pass, ix, x) {
            return None;
        }
        let Expr::Ident(idx) = &**index else {
            return None;
        };
        if !same_key(pass, idx, key) {
            return None;
        }
        if !matches!(second.tok, Some(Token::ASSIGN)) || second.lhs.len() != 1 || second.rhs.len() != 1 {
            return None;
        }
        let Expr::CallExpr(call) = &second.rhs[0] else {
            return None;
        };
        if !is_append_to_lhs(pass, call, &second.lhs[0]) || !same_key(pass, match &call.args[1] {
            Expr::Ident(v) => v,
            _ => return None,
        }, val) {
            return None;
        }
        let val_obj = object_of(pass, val)?;
        if refers_to(pass, &second.lhs[0], val_obj) {
            return None;
        }
        (&second.lhs[0], Some(val_obj), Some(object_of(pass, idx)?))
    } else {
        return None;
    };

    if let Some(idx_obj) = idx_obj {
        if refers_to(pass, lhs, idx_obj) {
            return None;
        }
    }

    let src = expr_type(pass, x)?;
    let dst = expr_type(pass, lhs)?;
    if render_type(pass, src) != render_type(pass, dst) {
        return None;
    }
    Some(())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1011 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder(pass.files(), |node| {
        let NodeRef::RangeStmt(rs) = node else {
            return;
        };
        if check_append_loop(pass, rs).is_some() {
            pending.push((
                match_pos(node),
                "should replace loop with append(lhs, x...)".into(),
            ));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1011_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1011",
        doc: "use a single append to concatenate two slices",
        url: "https://staticcheck.dev/docs/checks/#S1011",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1011_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1011_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
