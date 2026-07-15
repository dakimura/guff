//! Port of [`github.com/ultraware/funlen`](https://github.com/ultraware/funlen)
//! (golangci-lint wrapper in `pkg/golinters/funlen`).
//!
//! Defaults match ultraware/golangci when settings are unset:
//! `lines=60`, `statements=40`, `ignore-comments=true`.
//!
//! DEFERRED: reliable comment stripping when packages are parsed without
//! `PARSE_COMMENTS` (production typecheck).

use std::sync::OnceLock;

use guff::ast::{Decl, Expr, FuncDecl, Stmt};
use guff::position::{FileSet, Pos};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::FunlenOptions;

fn check_inline_func(expr: &Expr) -> usize {
    match expr {
        Expr::FuncLit(lit) => parse_stmts(&lit.body.list),
        _ => 0,
    }
}

fn parse_stmts(stmts: &[Stmt]) -> usize {
    let mut total = 0usize;
    for stmt in stmts {
        total += 1;
        match stmt {
            Stmt::BlockStmt(b) => {
                total = total.saturating_add(parse_stmts(&b.list)).saturating_sub(1);
            }
            Stmt::ForStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::RangeStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::IfStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::SwitchStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::TypeSwitchStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::SelectStmt(s) => total += parse_stmts(&s.body.list),
            Stmt::CaseClause(c) => total += parse_stmts(&c.body),
            Stmt::AssignStmt(a) => {
                if let Some(rhs) = a.rhs.first() {
                    total += check_inline_func(rhs);
                }
            }
            Stmt::GoStmt(g) => total += check_inline_func(&g.call.fun),
            Stmt::DeferStmt(d) => total += check_inline_func(&d.call.fun),
            _ => {}
        }
    }
    total
}

fn func_pos(f: &FuncDecl) -> Pos {
    f.ty.pos()
}

fn func_end(f: &FuncDecl) -> Pos {
    f.body
        .as_ref()
        .map(|b| b.end())
        .unwrap_or_else(|| f.ty.end())
}

fn get_lines(
    fset: &FileSet,
    f: &FuncDecl,
    file_comments: &[guff::ast::CommentGroup],
    ignore_comments: bool,
) -> usize {
    let start_line = fset.position(func_pos(f)).line;
    let end_line = fset.position(func_end(f)).line;
    let line_count = end_line.saturating_sub(start_line).saturating_sub(1) as usize;

    if !ignore_comments {
        return line_count;
    }

    let mut comment_count = 0usize;
    for group in file_comments {
        let c_start = fset.position(group.pos()).line;
        let c_end = fset.position(group.end()).line;
        if c_start > start_line && c_end < end_line {
            comment_count += group.list.len();
        }
    }
    line_count.saturating_sub(comment_count)
}

fn check_func(
    fset: &FileSet,
    file_comments: &[guff::ast::CommentGroup],
    decl: &FuncDecl,
    options: FunlenOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(body) = &decl.body else {
        return;
    };

    let stmts = parse_stmts(&body.list);
    if stmts > options.statements {
        pending.push((
            decl.name.name_pos.0 as u32,
            format!(
                "Function '{}' has too many statements ({} > {})",
                decl.name.name, stmts, options.statements
            ),
        ));
        return;
    }

    let lines = get_lines(fset, decl, file_comments, options.ignore_comments);
    if lines > options.lines {
        pending.push((
            decl.name.name_pos.0 as u32,
            format!(
                "Function '{}' is too long ({} > {})",
                decl.name.name, lines, options.lines
            ),
        ));
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "funlen requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<FunlenOptions>("funlen")
        .copied()
        .unwrap_or_default();

    let mut pending = Vec::new();
    let fset = pass.fset().clone();
    for file in pass.files() {
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                check_func(&fset, &file.comments, f, options, &mut pending);
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "funlen",
        doc: "checks for long functions",
        url: "https://github.com/ultraware/funlen",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
