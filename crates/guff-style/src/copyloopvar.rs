//! Port of [`github.com/karamaru-alpha/copyloopvar`](https://github.com/karamaru-alpha/copyloopvar).
//!
//! `linters.settings.copyloopvar.check-alias` is wired (default false).

use std::sync::OnceLock;

use guff::ast::{AssignStmt, Expr, ForStmt, RangeStmt, Stmt};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};

use crate::options::CopyloopvarOptions;

fn ident_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Ident(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn is_simple_same_name(assign: &AssignStmt, rhs_name: &str, i: usize) -> bool {
    if assign.lhs.len() != 1 || i != 0 {
        return false;
    }
    ident_name(&assign.lhs[0]) == Some(rhs_name)
}

fn assign_end(assign: &AssignStmt) -> u32 {
    assign
        .rhs
        .last()
        .map(|e| e.end().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32)
}

fn report_pending(
    pending: &mut Vec<(u32, u32, String, bool)>,
    assign: &AssignStmt,
    rhs_name: &str,
    i: usize,
) {
    let pos = assign
        .lhs
        .first()
        .map(|e| e.pos().0 as u32)
        .unwrap_or(assign.tok_pos.0 as u32);
    let end = assign_end(assign);
    let message = format!(
        "The copy of the 'for' variable \"{rhs_name}\" can be deleted (Go 1.22+)"
    );
    let can_delete = is_simple_same_name(assign, rhs_name, i);
    pending.push((pos, end, message, can_delete));
}

fn check_assign_copies(
    assign: &AssignStmt,
    loop_vars: &[String],
    check_alias: bool,
    pending: &mut Vec<(u32, u32, String, bool)>,
) {
    if assign.tok != Some(Token::DEFINE) {
        return;
    }
    for (i, rh) in assign.rhs.iter().enumerate() {
        let Some(right) = ident_name(rh) else {
            continue;
        };
        if !loop_vars.iter().any(|v| v == right) {
            continue;
        }
        if !check_alias {
            // Default: require lhs name == rhs name.
            let Some(left) = assign.lhs.get(i).and_then(ident_name) else {
                continue;
            };
            if left != right {
                continue;
            }
        }
        report_pending(pending, assign, right, i);
    }
}

fn check_range_stmt(
    range_stmt: &RangeStmt,
    check_alias: bool,
    pending: &mut Vec<(u32, u32, String, bool)>,
) {
    let mut loop_vars = Vec::new();
    if let Some(key) = range_stmt.key.as_ref().and_then(ident_name) {
        loop_vars.push(key.to_string());
    } else {
        return;
    }
    if let Some(value) = &range_stmt.value {
        let Some(name) = ident_name(value) else {
            return;
        };
        loop_vars.push(name.to_string());
    }
    for stmt in &range_stmt.body.list {
        if let Stmt::AssignStmt(assign) = stmt {
            check_assign_copies(assign, &loop_vars, check_alias, pending);
        }
    }
}

fn check_for_stmt(
    for_stmt: &ForStmt,
    check_alias: bool,
    pending: &mut Vec<(u32, u32, String, bool)>,
) {
    let Some(init) = for_stmt.init.as_deref() else {
        return;
    };
    let Stmt::AssignStmt(init_assign) = init else {
        return;
    };
    let mut loop_vars = Vec::new();
    for lh in &init_assign.lhs {
        if let Some(name) = ident_name(lh) {
            loop_vars.push(name.to_string());
        }
    }
    if loop_vars.is_empty() {
        return;
    }
    for stmt in &for_stmt.body.list {
        if let Stmt::AssignStmt(assign) = stmt {
            check_assign_copies(assign, &loop_vars, check_alias, pending);
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "copyloopvar requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<CopyloopvarOptions>("copyloopvar")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::RangeStmt(s) => {
                    check_range_stmt(s, options.check_alias, &mut pending);
                    true
                }
                NodeRef::ForStmt(s) => {
                    check_for_stmt(s, options.check_alias, &mut pending);
                    true
                }
                _ => true,
            }
        });
    }

    for (pos, end, message, can_delete) in pending {
        let suggested_fixes = if can_delete {
            vec![SuggestedFix {
                message: "Delete the redundant copy".into(),
                text_edits: vec![TextEdit {
                    pos,
                    end,
                    new_text: String::new(),
                }],
            }]
        } else {
            Vec::new()
        };
        pass.report(Diagnostic {
            pos,
            end,
            message,
            suggested_fixes,
            ..Diagnostic::default()
        });
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "copyloopvar",
        doc: "detects places where loop variables are copied (unnecessary since Go 1.22)",
        url: "https://github.com/karamaru-alpha/copyloopvar",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
