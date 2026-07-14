//! Port of [`github.com/polyfloyd/go-errorlint`](https://github.com/polyfloyd/go-errorlint).
//!
//! Default flags match upstream analyzer defaults: comparison + asserts on,
//! errorf off. The full allowed-errors allowlist is reduced to a few common
//! sentinels (`io.EOF`, …); DEFERRED for the complete package table.

use std::sync::OnceLock;

use guff::ast::{
    BinaryExpr, CaseClause, Expr, FuncDecl, Ident, Stmt, SwitchStmt, TypeAssertExpr,
    TypeSwitchStmt,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::util::{expr_string, is_pure_error, type_of, unparen, implements_error};

fn is_nil_ident(e: &Expr) -> bool {
    matches!(unparen(e), Expr::Ident(Ident { name, .. }) if name == "nil")
}

fn is_error_type(pass: &Pass<'_>, e: &Expr) -> bool {
    is_pure_error(pass, e)
}

fn is_allowed_sentinel(pass: &Pass<'_>, e: &Expr) -> bool {
    let Expr::SelectorExpr(sel) = unparen(e) else {
        return false;
    };
    if let Some(n) = code::selector_name(pass, sel) {
        return matches!(
            n.as_str(),
            "io.EOF"
                | "context.Canceled"
                | "context.DeadlineExceeded"
                | "database/sql.ErrNoRows"
        );
    }
    let Expr::Ident(pkg) = unparen(&sel.x) else {
        return false;
    };
    matches!(
        (pkg.name.as_str(), sel.sel.name.as_str()),
        ("io", "EOF")
            | ("context", "Canceled")
            | ("context", "DeadlineExceeded")
            | ("sql", "ErrNoRows")
    )
}

fn in_error_is_method(stack: &[NodeRef<'_>], pass: &Pass<'_>) -> bool {
    for n in stack.iter().rev() {
        let NodeRef::FuncDecl(FuncDecl { name, recv, ty, .. }) = n else {
            continue;
        };
        if name.name != "Is" || recv.is_none() {
            return false;
        }
        let Some(params) = ty.params.as_ref() else {
            return false;
        };
        if params.list.len() != 1 {
            return false;
        }
        let Some(pt) = params.list[0].ty.as_ref() else {
            return false;
        };
        let param_ok = matches!(unparen(pt), Expr::Ident(Ident { name, .. }) if name == "error")
            || is_pure_error(pass, pt);
        if !param_ok {
            return false;
        }
        let Some(results) = ty.results.as_ref() else {
            return false;
        };
        if results.list.len() != 1 {
            return false;
        }
        return matches!(
            results.list[0].ty.as_ref().map(unparen),
            Some(Expr::Ident(Ident { name, .. })) if name == "bool"
        );
    }
    false
}

fn check_comparison(
    pass: &Pass<'_>,
    be: &BinaryExpr,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<Diagnostic>,
) {
    if be.op != Token::EQL && be.op != Token::NEQ {
        return;
    }
    if is_nil_ident(&be.x) || is_nil_ident(&be.y) {
        return;
    }
    if !is_error_type(pass, &be.x) && !is_error_type(pass, &be.y) {
        return;
    }
    if is_allowed_sentinel(pass, &be.x) || is_allowed_sentinel(pass, &be.y) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }

    let (err_var, target) = if is_error_type(pass, &be.y) && !is_error_type(pass, &be.x) {
        (&be.y, &be.x)
    } else {
        (&be.x, &be.y)
    };
    let mut replacement = format!(
        "errors.Is({}, {})",
        expr_string(err_var),
        expr_string(target)
    );
    if be.op == Token::NEQ {
        replacement = format!("!{replacement}");
    }
    let start = be.x.pos().0 as u32;
    let end = be.y.end().0 as u32;
    pending.push(Diagnostic {
        pos: start,
        end,
        message: format!(
            "comparing with {} will fail on wrapped errors. Use errors.Is to check for a specific error",
            be.op
        ),
        suggested_fixes: vec![SuggestedFix {
            message: "Use errors.Is() to compare errors".into(),
            text_edits: vec![TextEdit {
                pos: start,
                end,
                new_text: replacement,
            }],
        }],
        ..Diagnostic::default()
    });
}

fn check_type_assert(
    pass: &Pass<'_>,
    ta: &TypeAssertExpr,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    let Some(ty) = ta.ty.as_deref() else {
        return;
    };
    if !is_error_type(pass, &ta.x) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }
    let Some(target_typ) = type_of(pass, ty) else {
        return;
    };
    if !implements_error(pass, target_typ) {
        return;
    }
    pending.push((
        ta.lparen.0 as u32,
        "type assertion on error will fail on wrapped errors. Use errors.As to check for specific errors"
            .into(),
    ));
}

fn check_type_switch(
    pass: &Pass<'_>,
    ts: &TypeSwitchStmt,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    if in_error_is_method(stack, pass) {
        return;
    }
    // extract assert from assign
    let assert_x = match &*ts.assign {
        Stmt::ExprStmt(es) => match unparen(&es.x) {
            Expr::TypeAssertExpr(ta) => &ta.x,
            _ => return,
        },
        Stmt::AssignStmt(asgn) => match asgn.rhs.first().map(unparen) {
            Some(Expr::TypeAssertExpr(ta)) => &ta.x,
            _ => return,
        },
        _ => return,
    };
    if !is_error_type(pass, assert_x) {
        return;
    }
    // Only report if some case asserts to an error-implementing type
    let mut has_error_case = false;
    for stmt in &ts.body.list {
        let Stmt::CaseClause(CaseClause { list, .. }) = stmt else {
            continue;
        };
        for e in list {
            if is_nil_ident(e) {
                continue;
            }
            if let Some(t) = type_of(pass, e) {
                if implements_error(pass, t) {
                    has_error_case = true;
                    break;
                }
            }
        }
    }
    if !has_error_case {
        return;
    }
    pending.push((
        ts.switch.0 as u32,
        "type switch on error will fail on wrapped errors. Use errors.As to check for specific errors"
            .into(),
    ));
}

fn check_value_switch(
    pass: &Pass<'_>,
    sw: &SwitchStmt,
    stack: &[NodeRef<'_>],
    pending: &mut Vec<(u32, String)>,
) {
    let Some(tag) = sw.tag.as_ref() else {
        return;
    };
    if !is_error_type(pass, tag) {
        return;
    }
    if in_error_is_method(stack, pass) {
        return;
    }
    let mut compares_non_nil = false;
    for stmt in &sw.body.list {
        let Stmt::CaseClause(CaseClause { list, .. }) = stmt else {
            continue;
        };
        for e in list {
            if is_nil_ident(e) {
                continue;
            }
            if is_allowed_sentinel(pass, e) {
                continue;
            }
            compares_non_nil = true;
            break;
        }
    }
    if !compares_non_nil {
        return;
    }
    pending.push((
        sw.switch.0 as u32,
        "switch on an error will fail on wrapped errors. Use errors.Is to check for specific errors"
            .into(),
    ));
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "errorlint requires inspect analyzer".to_string())?;

    let mut diags = Vec::new();
    let mut msgs = Vec::new();
    for file in pass.files() {
        let mut stack = Vec::new();
        walk::preorder_stack(NodeRef::File(file), &mut stack, |n, stack| {
            match n {
                NodeRef::BinaryExpr(be) => check_comparison(pass, be, stack, &mut diags),
                NodeRef::TypeAssertExpr(ta) => check_type_assert(pass, ta, stack, &mut msgs),
                NodeRef::TypeSwitchStmt(ts) => check_type_switch(pass, ts, stack, &mut msgs),
                NodeRef::SwitchStmt(sw) => check_value_switch(pass, sw, stack, &mut msgs),
                _ => {}
            }
            true
        });
    }
    for d in diags {
        pass.report(d);
    }
    for (pos, message) in msgs {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "errorlint",
        doc: "Linter for error wrapping issues (comparisons and type assertions)",
        url: "https://github.com/polyfloyd/go-errorlint",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
