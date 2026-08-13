//! S1033 — unnecessary guard around call to delete.
//!
//! Port of `honnef.co/go/tools/simple/s1033`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, Ident, IfStmt, IndexExpr, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, object_of, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn same_expr(pass: &Pass<'_>, a: &Expr, b: &Expr) -> bool {
    match (unparen(a), unparen(b)) {
        (Expr::Ident(ia), Expr::Ident(ib)) => {
            object_of(pass, ia) == object_of(pass, ib) || ia.name == ib.name
        }
        _ => a.id() == b.id(),
    }
}

fn map_index_init(init: &Stmt) -> Option<(&Expr, &Expr)> {
    let Stmt::AssignStmt(AssignStmt { lhs, rhs, tok, .. }) = init else {
        return None;
    };
    if tok != &Some(Token::DEFINE) || lhs.len() != 2 || rhs.len() != 1 {
        return None;
    };
    let Expr::Ident(blank) = unparen(&lhs[0]) else {
        return None;
    };
    if blank.name != "_" {
        return None;
    };
    let Expr::IndexExpr(IndexExpr { x: m, index: key, .. }) = unparen(&rhs[0]) else {
        return None;
    };
    Some((m, key))
}

fn check_if(pass: &Pass<'_>, ifs: &IfStmt) -> bool {
    let Some((m, key)) = ifs.init.as_deref().and_then(map_index_init) else {
        return false;
    };
    let Expr::Ident(ok) = unparen(&ifs.cond) else {
        return false;
    };
    if ok.name == "_" || ifs.else_.is_some() || ifs.body.list.len() != 1 {
        return false;
    }
    let Stmt::ExprStmt(es) = &ifs.body.list[0] else {
        return false;
    };
    let Expr::CallExpr(call) = unparen(&es.x) else {
        return false;
    };
    call.args.len() == 2
        && same_expr(pass, &call.args[0], m)
        && same_expr(pass, &call.args[1], key)
        && is_call_to(pass, call, "delete")
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1033 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        if check_if(pass, ifs) {
            pending.push((match_pos(node), "unnecessary guard around call to delete".into()));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1033_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1033",
        doc: "unnecessary guard around call to delete",
        url: "https://staticcheck.dev/docs/checks/#S1033",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

/// S1033 analyzer singleton.
pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1033_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1033_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
