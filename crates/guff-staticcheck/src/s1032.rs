//! S1032 — use `sort.Ints` / `sort.Float64s` / `sort.Strings`.
//!
//! Port of `honnef.co/go/tools/simple/s1032`.

use std::sync::OnceLock;

use guff::ast::{CallExpr, Expr, Stmt};
use guff::node_mask;
use guff::walk::NodeRef;
use guff_analysis::code::{is_call_to, selector_name};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, RunError, RunFn, Pass};

fn is_permissible_sort(pass: &Pass<'_>, call: &CallExpr) -> bool {
    // `sort.Sort()` with no arguments only parses in code that does not
    // type-check, but analyzers still see it — indexing here panicked the
    // worker on helm and kubernetes, silently dropping S1032 for the whole
    // package.
    let Some(arg0) = call.args.first() else {
        return true;
    };
    let Expr::CallExpr(typeconv) = arg0 else {
        return true;
    };
    let Expr::SelectorExpr(sel) = &*typeconv.fun else {
        return true;
    };
    let Some(name) = selector_name(pass, sel) else {
        return true;
    };
    !matches!(
        name.as_str(),
        "sort.IntSlice" | "sort.Float64Slice" | "sort.StringSlice"
    )
}

fn check_sort_call(pass: &Pass<'_>, call: &CallExpr) -> Option<String> {
    if !is_call_to(pass, call, "sort.Sort") || call.args.is_empty() {
        return None;
    }
    let Expr::CallExpr(typeconv) = &call.args[0] else {
        return None;
    };
    let Expr::SelectorExpr(sel) = &*typeconv.fun else {
        return None;
    };
    let name = selector_name(pass, sel)?;
    match name.as_str() {
        "sort.IntSlice" => Some(
            "should use sort.Ints(...) instead of sort.Sort(sort.IntSlice(...))".into(),
        ),
        "sort.Float64Slice" => Some(
            "should use sort.Float64s(...) instead of sort.Sort(sort.Float64Slice(...))".into(),
        ),
        "sort.StringSlice" => Some(
            "should use sort.Strings(...) instead of sort.Sort(sort.StringSlice(...))".into(),
        ),
        _ => None,
    }
}

fn check_body(pass: &Pass<'_>, body: &guff::ast::BlockStmt) -> Vec<(u32, String)> {
    let mut errors = Vec::new();
    let mut permissible = false;
    for stmt in &body.list {
        if permissible {
            break;
        }
        let Stmt::ExprStmt(expr_stmt) = stmt else {
            continue;
        };
        let Expr::CallExpr(call) = &expr_stmt.x else {
            continue;
        };
        if !is_call_to(pass, call, "sort.Sort") {
            continue;
        }
        if is_permissible_sort(pass, call) {
            permissible = true;
            continue;
        }
        if let Some(msg) = check_sort_call(pass, call) {
            errors.push((call.lparen.0 as u32, msg));
        }
    }
    if permissible {
        Vec::new()
    } else {
        errors
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "S1032 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, String)> = Vec::new();
    inspect.preorder_typed(node_mask!(FuncDecl, FuncLit), pass.files(), |n| {
        let body = match n {
            NodeRef::FuncDecl(f) => f.body.as_ref(),
            NodeRef::FuncLit(f) => Some(&f.body),
            _ => None,
        };
        if let Some(body) = body {
            pending.extend(check_body(pass, body));
        }
    });
    for (pos, message) in pending {
        pass.report_unless_generated(pos, message);
    }
    Ok(None)
}

fn s1032_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "S1032",
        doc: "use sort.Ints, sort.Float64s, or sort.Strings",
        url: "https://staticcheck.dev/docs/checks/#S1032",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(s1032_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn s1032_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }
}
