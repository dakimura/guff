//! Port of [`github.com/charithe/durationcheck`](https://github.com/charithe/durationcheck).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, BinaryExpr, CallExpr, Expr, Ident, SelectorExpr};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::code;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::TypeId;

use crate::util::{type_of, unparen};

fn has_time_import(pass: &Pass<'_>) -> bool {
    if pass.pkg().imports.contains_key("time") {
        return true;
    }
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::GenDecl(g) = decl else {
                continue;
            };
            if g.tok != Some(Token::IMPORT) {
                continue;
            }
            for spec in &g.specs {
                let guff::ast::Spec::ImportSpec(is) = spec else {
                    continue;
                };
                if is.path.value.trim_matches('"') == "time" {
                    return true;
                }
            }
        }
    }
    false
}

fn is_duration(pass: &Pass<'_>, typ: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let s = guff_types::typestring::type_string(
        &artifacts.types,
        &artifacts.objects,
        &artifacts.packages,
        typ,
        None,
    );
    s == "time.Duration" || s == "*time.Duration"
}

fn is_duration_cast(fun: &Expr) -> bool {
    let Expr::SelectorExpr(SelectorExpr { x, sel, .. }) = unparen(fun) else {
        return false;
    };
    matches!(unparen(x), Expr::Ident(Ident { name, .. }) if name == "time")
        && sel.name == "Duration"
}

fn is_acceptable_ident(pass: &Pass<'_>, ident: &Ident) -> bool {
    let Some(info) = pass.types_info() else {
        return false;
    };
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let Some(obj) = info
        .uses
        .get(&ident.id)
        .copied()
        .or_else(|| info.defs.get(&ident.id).copied().flatten())
    else {
        return false;
    };
    let Some(typ) = obj.typ(&artifacts.objects) else {
        return true;
    };
    !is_duration(pass, typ)
}

fn is_acceptable_cast(pass: &Pass<'_>, call: &CallExpr) -> bool {
    if call.args.len() != 1 {
        return false;
    }
    if !is_acceptable_nested(pass, &call.args[0]) {
        return false;
    }
    is_duration_cast(&call.fun)
}

fn is_acceptable_nested(pass: &Pass<'_>, n: &Expr) -> bool {
    match unparen(n) {
        Expr::BasicLit(_) => true,
        Expr::BinaryExpr(b) => {
            is_acceptable_nested(pass, &b.x) && is_acceptable_nested(pass, &b.y)
        }
        Expr::UnaryExpr(u) => is_acceptable_nested(pass, &u.x),
        Expr::Ident(id) => is_acceptable_ident(pass, id),
        Expr::CallExpr(c) => {
            if is_acceptable_cast(pass, c) {
                return true;
            }
            match type_of(pass, n) {
                Some(t) => !is_duration(pass, t),
                None => false,
            }
        }
        Expr::SelectorExpr(sel) => {
            is_acceptable_nested(pass, &sel.x) && is_acceptable_ident(pass, &sel.sel)
        }
        Expr::StarExpr(s) => is_acceptable_nested(pass, &s.x),
        Expr::IndexExpr(_i) => match type_of(pass, n) {
            Some(t) => !is_duration(pass, t),
            None => false,
        },
        _ => false,
    }
}

fn is_unacceptable(pass: &Pass<'_>, e: &Expr) -> bool {
    match unparen(e) {
        Expr::BasicLit(_) => false,
        Expr::Ident(id) => !is_acceptable_nested(pass, &Expr::Ident(id.clone())),
        Expr::CallExpr(c) => !is_acceptable_cast(pass, c),
        Expr::BinaryExpr(_)
        | Expr::UnaryExpr(_)
        | Expr::SelectorExpr(_)
        | Expr::StarExpr(_)
        | Expr::ParenExpr(_)
        | Expr::IndexExpr(_) => !is_acceptable_nested(pass, e),
        _ => true,
    }
}

fn check_binary(pass: &Pass<'_>, expr: &BinaryExpr, pending: &mut Vec<(u32, String)>) {
    if expr.op != Token::MUL && expr.op != Token::MulAssign {
        return;
    }
    let Some(x) = type_of(pass, &expr.x) else {
        return;
    };
    let Some(y) = type_of(pass, &expr.y) else {
        return;
    };
    if !(is_duration(pass, x) && is_duration(pass, y)) {
        return;
    }
    if is_unacceptable(pass, &expr.x) && is_unacceptable(pass, &expr.y) {
        pending.push((
            // `pass.Reportf(expr.Pos(), …)`, and `BinaryExpr.Pos()` is
            // `X.Pos()` — the left operand, not the operator between them.
            expr.x.pos().0 as u32,
            // `pass.Reportf(…, "Multiplication of durations: `%s`", formatNode(expr))`
            // — the *whole* BinaryExpr through `format.Node`, which is
            // go/printer. Rebuilding it as "{x} {op} {y}" agrees on `d *
            // time.Second` and diverges wherever go/printer's precedence rule
            // drops the blanks.
            format!(
                "Multiplication of durations: `{}`",
                code::node_text(pass, &Expr::BinaryExpr(expr.clone())).unwrap_or_default()
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "durationcheck requires inspect analyzer".to_string())?;

    if !has_time_import(pass) {
        return Ok(None);
    }

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::preorder(NodeRef::File(file), |n| {
            match n {
                NodeRef::BinaryExpr(be) => check_binary(pass, be, &mut pending),
                NodeRef::AssignStmt(AssignStmt {
                    tok: Some(Token::MulAssign),
                    lhs,
                    rhs,
                    tok_pos,
                    ..
                }) if lhs.len() == 1 && rhs.len() == 1 => {
                    check_binary(
                        pass,
                        &BinaryExpr {
                            x: Box::new(lhs[0].clone()),
                            op_pos: *tok_pos,
                            op: Token::MulAssign,
                            y: Box::new(rhs[0].clone()),
                            id: 0,
                        },
                        &mut pending,
                    );
                }
                _ => {}
            }
            true
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "durationcheck",
        doc: "check for two durations multiplied together",
        url: "https://github.com/charithe/durationcheck",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
