//! Port of [`github.com/tommy-muehle/go-mnd`](https://github.com/tommy-muehle/go-mnd)
//! (golangci-lint wrapper in `pkg/golinters/mnd`).
//!
//! Defaults match golangci-lint / upstream: all checks enabled; ignore `0`/`1`/
//! `0.0`/`1.0`; ignore `_test.go`; ignore common `strconv`/`time.Date` callers.
//!
//! DEFERRED: `linters.settings.mnd` wiring (`checks`, `ignored-numbers`,
//! `ignored-files`, `ignored-functions`).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BasicLit, BinaryExpr, CallExpr, CaseClause, Expr, IfStmt, KeyValueExpr, ReturnStmt,
    Spec, UnaryExpr,
};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

fn is_ignored_number(value: &str) -> bool {
    let compact: String = value.chars().filter(|&c| c != '_').collect();
    matches!(compact.as_str(), "0" | "0.0" | "1" | "1.0")
}

fn is_magic_number(lit: &BasicLit) -> bool {
    matches!(lit.kind, Some(Token::INT) | Some(Token::FLOAT)) && !is_ignored_number(&lit.value)
}

fn report_lit(pending: &mut Vec<(u32, String)>, lit: &BasicLit, check: &str) {
    pending.push((
        lit.value_pos.0 as u32,
        format!("Magic number: {}, in <{check}> detected", lit.value),
    ));
}

fn check_binary_lits(pending: &mut Vec<(u32, String)>, expr: &BinaryExpr, check: &str) {
    if let Expr::BasicLit(x) = &*expr.x {
        if is_magic_number(x) {
            report_lit(pending, x, check);
        }
    }
    if let Expr::BasicLit(y) = &*expr.y {
        if is_magic_number(y) {
            report_lit(pending, y, check);
        }
    }
}

fn call_name(expr: &CallExpr) -> Option<String> {
    match &*expr.fun {
        Expr::Ident(id) => Some(id.name.clone()),
        Expr::SelectorExpr(sel) => {
            if let Expr::Ident(pkg) = &*sel.x {
                Some(format!("{}.{}", pkg.name, sel.sel.name))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn is_ignored_function(name: &str) -> bool {
    matches!(
        name,
        "time.Date"
            | "strconv.FormatInt"
            | "strconv.FormatUint"
            | "strconv.FormatFloat"
            | "strconv.ParseInt"
            | "strconv.ParseUint"
            | "strconv.ParseFloat"
    )
}

fn check_argument(
    pending: &mut Vec<(u32, String)>,
    const_lines: &HashSet<i64>,
    fset: &guff::position::FileSet,
    expr: &CallExpr,
) {
    let line = fset.position(expr.pos()).line;
    if const_lines.contains(&line) {
        return;
    }
    if let Some(name) = call_name(expr) {
        if is_ignored_function(&name) {
            return;
        }
    }

    for (i, arg) in expr.args.iter().enumerate() {
        match arg {
            Expr::BasicLit(x) if is_magic_number(x) => {
                if i == 0 || matches!(arg, Expr::BasicLit(_)) {
                    report_lit(pending, x, "argument");
                }
            }
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "argument"),
            _ => {}
        }
    }
}

fn check_unary(pending: &mut Vec<(u32, String)>, expr: &UnaryExpr, check: &str) {
    if let Expr::BasicLit(x) = &*expr.x {
        if is_magic_number(x) {
            report_lit(pending, x, check);
        }
    }
}

fn check_assign(pending: &mut Vec<(u32, String)>, stmt: &AssignStmt) {
    for e in &stmt.rhs {
        match e {
            Expr::UnaryExpr(u) => check_unary(pending, u, "assign"),
            Expr::BinaryExpr(bin) => {
                if let Expr::UnaryExpr(u) = &*bin.y {
                    check_unary(pending, u, "assign");
                }
            }
            _ => {}
        }
    }
}

fn check_key_value(pending: &mut Vec<(u32, String)>, expr: &KeyValueExpr) {
    match &*expr.value {
        Expr::BasicLit(x) if is_magic_number(x) => report_lit(pending, x, "assign"),
        Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "assign"),
        _ => {}
    }
}

fn check_operation_assign(pending: &mut Vec<(u32, String)>, stmt: &AssignStmt) {
    for y in &stmt.rhs {
        let Expr::BinaryExpr(x) = y else {
            continue;
        };
        if let Expr::BinaryExpr(inner) = &*x.x {
            check_binary_lits(pending, inner, "operation");
        }
        if let Expr::BinaryExpr(inner) = &*x.y {
            check_binary_lits(pending, inner, "operation");
        }
        check_binary_lits(pending, x, "operation");
    }
}

fn check_condition(pending: &mut Vec<(u32, String)>, stmt: &IfStmt) {
    let Expr::BinaryExpr(expr) = &stmt.cond else {
        return;
    };
    check_binary_lits(pending, expr, "condition");
}

fn check_case(pending: &mut Vec<(u32, String)>, clause: &CaseClause) {
    for c in &clause.list {
        match c {
            Expr::BasicLit(x) if is_magic_number(x) => report_lit(pending, x, "case"),
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "case"),
            _ => {}
        }
    }
}

fn check_return(pending: &mut Vec<(u32, String)>, stmt: &ReturnStmt) {
    for expr in &stmt.results {
        match expr {
            Expr::BasicLit(x) if is_magic_number(x) => report_lit(pending, x, "return"),
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "return"),
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "mnd requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    let pkg = pass.pkg();

    for (i, file) in pass.files().iter().enumerate() {
        let fallback = fset.position(file.pos()).filename;
        let filename = pkg
            .compiled_go_files
            .get(i)
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or(fallback.as_str());
        if filename.ends_with("_test.go") {
            continue;
        }

        let mut const_lines = HashSet::new();
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            if let NodeRef::GenDecl(d) = n {
                if d.tok == Some(Token::CONST) {
                    for spec in &d.specs {
                        if let Spec::ValueSpec(vs) = spec {
                            let pos = vs.names.first().map(|n| n.pos()).unwrap_or(d.tok_pos);
                            const_lines.insert(fset.position(pos).line);
                        }
                    }
                }
            }
            true
        });

        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::CallExpr(c) => check_argument(&mut pending, &const_lines, &fset, c),
                NodeRef::AssignStmt(a) => {
                    check_assign(&mut pending, a);
                    check_operation_assign(&mut pending, a);
                }
                NodeRef::KeyValueExpr(kv) => check_key_value(&mut pending, kv),
                NodeRef::ParenExpr(p) => {
                    if let Expr::BinaryExpr(bin) = &*p.x {
                        check_binary_lits(&mut pending, bin, "operation");
                    }
                }
                NodeRef::IfStmt(s) => check_condition(&mut pending, s),
                NodeRef::CaseClause(c) => check_case(&mut pending, c),
                NodeRef::ReturnStmt(r) => check_return(&mut pending, r),
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
        name: "mnd",
        doc: "An analyzer to detect magic numbers",
        url: "https://github.com/tommy-muehle/go-mnd",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
