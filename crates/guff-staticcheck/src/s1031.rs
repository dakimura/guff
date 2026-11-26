//! S1031 — omit redundant nil check around loop.
//!
//! Port of `honnef.co/go/tools/simple/s1031`.
//!
//! **Parentheses.** Upstream states this check as a `pattern` query, and
//! `pattern.match` strips `*ast.ParenExpr` at every recursion (before binding),
//! so `f((x))` matches wherever `f(x)` does. This port descends by hand, so
//! every descent has to `unparen` — `compat/fuzz.py`'s `paren` mutation found
//! nine S-checks going quiet on a parenthesized subexpression at once
//! (COMPAT-HARDENING §4, 2026-08-13).

use std::sync::OnceLock;

use guff::ast::{BinaryExpr, Expr, Ident, IfStmt, RangeStmt, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{is_nil, object_of, unparen};
use guff_analysis::passes::inspect;
use guff_analysis::{match_pos, AnalysisResult, Analyzer, RunError, RunFn, Pass};
use guff_types::TypeData;

fn is_slice_or_map(pass: &Pass<'_>, obj: guff_types::ObjectId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return false;
    };
    let u = typ.underlying(&artifacts.types);
    matches!(
        artifacts.types.get(u),
        TypeData::Slice(_) | TypeData::Map(_)
    )
}

fn check_if(pass: &Pass<'_>, ifs: &IfStmt) -> Option<()> {
    // `(IfStmt nil cond [range] nil)` — the trailing `nil` is the else branch.
    // With one, dropping the check is not the same program: dapr's
    // `default_bulksub.go` runs the "all messages failed" path instead.
    if ifs.init.is_some() || ifs.else_.is_some() {
        return None;
    }
    let Expr::BinaryExpr(BinaryExpr { x, op, y, .. }) = unparen(&ifs.cond) else {
        return None;
    };
    if *op != Token::NEQ || !is_nil(pass, y) {
        return None;
    };
    let Expr::Ident(id) = unparen(x) else {
        return None;
    };
    if ifs.body.list.len() != 1 {
        return None;
    };
    let Stmt::RangeStmt(rs) = &ifs.body.list[0] else {
        return None;
    };
    let Expr::Ident(range_var) = unparen(&rs.x) else {
        return None;
    };
    if object_of(pass, id) != object_of(pass, range_var) && id.name != range_var.name {
        return None;
    }
    Some(())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1031 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |node| {
        let NodeRef::IfStmt(ifs) = node else {
            return;
        };
        if check_if(pass, ifs).is_none() {
            return;
        }
        let Expr::BinaryExpr(BinaryExpr { x, .. }) = unparen(&ifs.cond) else {
            return;
        };
        let Expr::Ident(id) = unparen(x) else {
            return;
        };
        let Some(obj) = object_of(pass, id) else {
            return;
        };
        if !is_slice_or_map(pass, obj) {
            return;
        }
        pending.push((match_pos(node), "unnecessary nil check around range".into()));
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1031_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1031",
        doc: "omit redundant nil check around loop",
        url: "https://staticcheck.dev/docs/checks/#S1031",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1031_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1031_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
