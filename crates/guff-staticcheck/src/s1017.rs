//! S1017 — replace manual trimming with `strings.TrimPrefix` / `TrimSuffix`.
//!
//! Port of `honnef.co/go/tools/simple/s1017` (HasPrefix/HasSuffix + slice cases).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{AssignStmt, CallExpr, Expr, IfStmt, SliceExpr, Stmt};
use guff::node_mask;
use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::code::{call_name, expr_to_int, expr_to_string, is_call_to, same_non_dynamic};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn is_len_on_expr(pass: &Pass<'_>, call: &CallExpr, target: &Expr) -> bool {
    is_call_to(pass, call, "len") && call.args.len() == 1 && same_non_dynamic(pass, &call.args[0], target)
}

fn validate_prefix_offset(pass: &Pass<'_>, off: &Expr, prefix: &Expr) -> bool {
    match off {
        Expr::CallExpr(call) => is_len_on_expr(pass, call, prefix),
        Expr::BasicLit(_) => {
            let Some(s) = expr_to_string(pass, prefix) else {
                return false;
            };
            expr_to_int(pass, off) == Some(s.len() as i64)
        }
        _ => false,
    }
}

fn check_if_has_prefix_slice(
    pass: &Pass<'_>,
    if_: &IfStmt,
    cond_call: &CallExpr,
    pkg: &str,
) -> Option<(u32, String)> {
    let Stmt::AssignStmt(assign) = if_.body.list.first()? else {
        return None;
    };
    if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    if !same_non_dynamic(pass, &cond_call.args[0], &assign.lhs[0]) {
        return None;
    }
    let Expr::SliceExpr(slice) = &assign.rhs[0] else {
        return None;
    };
    if slice.slice3 || !same_non_dynamic(pass, &slice.x, &cond_call.args[0]) {
        return None;
    }
    if slice.high.is_some() {
        return None;
    }
    if !validate_prefix_offset(pass, slice.low.as_deref()?, &cond_call.args[1]) {
        return None;
    }
    let replacement = match pkg {
        "strings" => "strings.TrimPrefix",
        "bytes" => "bytes.TrimPrefix",
        _ => return None,
    };
    Some((
        if_.if_.0 as u32,
        format!("should replace this if statement with an unconditional {replacement}"),
    ))
}

fn check_if(pass: &Pass<'_>, if_: &IfStmt, seen: &mut HashSet<u32>) -> Option<(u32, String)> {
    if if_.init.is_some() || if_.else_.is_some() {
        return None;
    }
    let pos = if_.if_.0 as u32;
    if seen.contains(&pos) {
        return None;
    }
    if if_.body.list.len() != 1 {
        return None;
    }
    let Expr::CallExpr(cond_call) = &if_.cond else {
        return None;
    };
    let cond_name = call_name(pass, &cond_call.fun)?;
    let (pkg, fun, _replacement) = match cond_name.as_str() {
        "strings.HasPrefix" => ("strings", "HasPrefix", "strings.TrimPrefix"),
        "strings.HasSuffix" => ("strings", "HasSuffix", "strings.TrimSuffix"),
        "bytes.HasPrefix" => ("bytes", "HasPrefix", "bytes.TrimPrefix"),
        "bytes.HasSuffix" => ("bytes", "HasSuffix", "bytes.TrimSuffix"),
        _ => return None,
    };

    let Stmt::AssignStmt(assign) = &if_.body.list[0] else {
        return None;
    };
    if assign.tok != Some(Token::ASSIGN) || assign.lhs.len() != 1 || assign.rhs.len() != 1 {
        return None;
    }
    if !same_non_dynamic(pass, &cond_call.args[0], &assign.lhs[0]) {
        return None;
    }

    match &assign.rhs[0] {
        Expr::CallExpr(rhs_call) => {
            if rhs_call.args.len() < 2 {
                return None;
            }
            if !same_non_dynamic(pass, &cond_call.args[0], &rhs_call.args[0])
                || !same_non_dynamic(pass, &cond_call.args[1], &rhs_call.args[1])
            {
                return None;
            }
            let rhs_name = call_name(pass, &rhs_call.fun)?;
            let ok = match (cond_name.as_str(), rhs_name.as_str()) {
                ("strings.HasPrefix", "strings.TrimPrefix")
                | ("strings.HasSuffix", "strings.TrimSuffix")
                | ("bytes.HasPrefix", "bytes.TrimPrefix")
                | ("bytes.HasSuffix", "bytes.TrimSuffix") => true,
                _ => false,
            };
            if !ok {
                return None;
            }
            seen.insert(pos);
            Some((
                pos,
                format!("should replace this if statement with an unconditional {rhs_name}"),
            ))
        }
        Expr::SliceExpr(slice) if fun == "HasPrefix" => {
            check_if_has_prefix_slice(pass, if_, cond_call, pkg).map(|(p, m)| {
                seen.insert(p);
                (p, m)
            })
        }
        _ => None,
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1017 requires inspect analyzer".to_string())?
        .clone();

    let mut seen = HashSet::new();
    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(IfStmt), pass.files(), |n| {
        let NodeRef::IfStmt(if_) = n else {
            return;
        };
        if let Some(diag) = check_if(pass, if_, &mut seen) {
            pending.push(diag);
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1017_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1017",
        doc: "replace manual trimming with strings.TrimPrefix",
        url: "https://staticcheck.dev/docs/checks/#S1017",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1017_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1017_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
