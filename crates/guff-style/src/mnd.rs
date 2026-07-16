//! Port of [`github.com/tommy-muehle/go-mnd`](https://github.com/tommy-muehle/go-mnd)
//! (golangci-lint wrapper in `pkg/golinters/mnd`).
//!
//! Defaults match golangci-lint / upstream: all checks enabled; ignore `0`/`1`/
//! `0.0`/`1.0`; ignore `_test.go`; ignore common `strconv`/`time.Date` callers.

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

use crate::options::MndOptions;

fn is_ignored_number(value: &str, extra: &[String]) -> bool {
    let compact: String = value.chars().filter(|&c| c != '_').collect();
    if matches!(compact.as_str(), "0" | "0.0" | "1" | "1.0") {
        return true;
    }
    extra.iter().any(|n| {
        let ignored: String = n.chars().filter(|&c| c != '_').collect();
        ignored == compact
    })
}

fn is_magic_number(lit: &BasicLit, extra: &[String]) -> bool {
    matches!(lit.kind, Some(Token::INT) | Some(Token::FLOAT))
        && !is_ignored_number(&lit.value, extra)
}

fn report_lit(
    pending: &mut Vec<(u32, String)>,
    lit: &BasicLit,
    check: &str,
    ignored_numbers: &[String],
) {
    if !is_magic_number(lit, ignored_numbers) {
        return;
    }
    pending.push((
        lit.value_pos.0 as u32,
        format!("Magic number: {}, in <{check}> detected", lit.value),
    ));
}

fn check_binary_lits(
    pending: &mut Vec<(u32, String)>,
    expr: &BinaryExpr,
    check: &str,
    ignored_numbers: &[String],
) {
    if let Expr::BasicLit(x) = &*expr.x {
        report_lit(pending, x, check, ignored_numbers);
    }
    if let Expr::BasicLit(y) = &*expr.y {
        report_lit(pending, y, check, ignored_numbers);
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

fn is_ignored_function(name: &str, extra: &[String]) -> bool {
    if matches!(
        name,
        "time.Date"
            | "strconv.FormatInt"
            | "strconv.FormatUint"
            | "strconv.FormatFloat"
            | "strconv.ParseInt"
            | "strconv.ParseUint"
            | "strconv.ParseFloat"
    ) {
        return true;
    }
    extra.iter().any(|pat| name == pat || name.ends_with(pat.trim_start_matches('^')))
}

fn filename_ignored(filename: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pat| {
        if pat.contains('*') || pat.contains('?') {
            filename.contains(pat.trim_matches('*'))
        } else {
            filename.ends_with(pat) || filename.contains(pat)
        }
    })
}

fn check_argument(
    pending: &mut Vec<(u32, String)>,
    const_lines: &HashSet<i64>,
    fset: &guff::position::FileSet,
    expr: &CallExpr,
    ignored_numbers: &[String],
    ignored_functions: &[String],
) {
    let line = fset.position(expr.pos()).line;
    if const_lines.contains(&line) {
        return;
    }
    if let Some(name) = call_name(expr) {
        if is_ignored_function(&name, ignored_functions) {
            return;
        }
    }

    for (i, arg) in expr.args.iter().enumerate() {
        match arg {
            Expr::BasicLit(x) if is_magic_number(x, ignored_numbers) => {
                if i == 0 || matches!(arg, Expr::BasicLit(_)) {
                    report_lit(pending, x, "argument", ignored_numbers);
                }
            }
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "argument", ignored_numbers),
            _ => {}
        }
    }
}

fn check_unary(
    pending: &mut Vec<(u32, String)>,
    expr: &UnaryExpr,
    check: &str,
    ignored_numbers: &[String],
) {
    if let Expr::BasicLit(x) = &*expr.x {
        report_lit(pending, x, check, ignored_numbers);
    }
}

fn check_assign(
    pending: &mut Vec<(u32, String)>,
    stmt: &AssignStmt,
    ignored_numbers: &[String],
) {
    for e in &stmt.rhs {
        match e {
            Expr::UnaryExpr(u) => check_unary(pending, u, "assign", ignored_numbers),
            Expr::BinaryExpr(bin) => {
                if let Expr::UnaryExpr(u) = &*bin.y {
                    check_unary(pending, u, "assign", ignored_numbers);
                }
            }
            _ => {}
        }
    }
}

fn check_key_value(
    pending: &mut Vec<(u32, String)>,
    expr: &KeyValueExpr,
    ignored_numbers: &[String],
) {
    match &*expr.value {
        Expr::BasicLit(x) if is_magic_number(x, ignored_numbers) => {
            report_lit(pending, x, "assign", ignored_numbers)
        }
        Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "assign", ignored_numbers),
        _ => {}
    }
}

fn check_operation_assign(
    pending: &mut Vec<(u32, String)>,
    stmt: &AssignStmt,
    ignored_numbers: &[String],
) {
    for y in &stmt.rhs {
        let Expr::BinaryExpr(x) = y else {
            continue;
        };
        if let Expr::BinaryExpr(inner) = &*x.x {
            check_binary_lits(pending, inner, "operation", ignored_numbers);
        }
        if let Expr::BinaryExpr(inner) = &*x.y {
            check_binary_lits(pending, inner, "operation", ignored_numbers);
        }
        check_binary_lits(pending, x, "operation", ignored_numbers);
    }
}

fn check_condition(
    pending: &mut Vec<(u32, String)>,
    stmt: &IfStmt,
    ignored_numbers: &[String],
) {
    let Expr::BinaryExpr(expr) = &stmt.cond else {
        return;
    };
    check_binary_lits(pending, expr, "condition", ignored_numbers);
}

fn check_case(
    pending: &mut Vec<(u32, String)>,
    clause: &CaseClause,
    ignored_numbers: &[String],
) {
    for c in &clause.list {
        match c {
            Expr::BasicLit(x) if is_magic_number(x, ignored_numbers) => {
                report_lit(pending, x, "case", ignored_numbers)
            }
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "case", ignored_numbers),
            _ => {}
        }
    }
}

fn check_return(
    pending: &mut Vec<(u32, String)>,
    stmt: &ReturnStmt,
    ignored_numbers: &[String],
) {
    for expr in &stmt.results {
        match expr {
            Expr::BasicLit(x) if is_magic_number(x, ignored_numbers) => {
                report_lit(pending, x, "return", ignored_numbers)
            }
            Expr::BinaryExpr(bin) => check_binary_lits(pending, bin, "return", ignored_numbers),
            _ => {}
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "mnd requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<MndOptions>("mnd")
        .cloned()
        .unwrap_or_default();

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
        if filename.ends_with("_test.go") || filename_ignored(filename, &options.ignored_files) {
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
                NodeRef::CallExpr(c) if options.check_enabled("argument") => check_argument(
                    &mut pending,
                    &const_lines,
                    &fset,
                    c,
                    &options.ignored_numbers,
                    &options.ignored_functions,
                ),
                NodeRef::AssignStmt(a) => {
                    if options.check_enabled("assign") {
                        check_assign(&mut pending, a, &options.ignored_numbers);
                    }
                    if options.check_enabled("operation") {
                        check_operation_assign(&mut pending, a, &options.ignored_numbers);
                    }
                }
                NodeRef::KeyValueExpr(kv) if options.check_enabled("assign") => {
                    check_key_value(&mut pending, kv, &options.ignored_numbers)
                }
                NodeRef::ParenExpr(p) if options.check_enabled("operation") => {
                    if let Expr::BinaryExpr(bin) = &*p.x {
                        check_binary_lits(&mut pending, bin, "operation", &options.ignored_numbers);
                    }
                }
                NodeRef::IfStmt(s) if options.check_enabled("condition") => {
                    check_condition(&mut pending, s, &options.ignored_numbers)
                }
                NodeRef::CaseClause(c) if options.check_enabled("case") => {
                    check_case(&mut pending, c, &options.ignored_numbers)
                }
                NodeRef::ReturnStmt(r) if options.check_enabled("return") => {
                    check_return(&mut pending, r, &options.ignored_numbers)
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
        name: "mnd",
        doc: "An analyzer to detect magic numbers",
        url: "https://github.com/tommy-muehle/go-mnd",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
