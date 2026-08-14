//! Port of [`github.com/ultraware/funlen`](https://github.com/ultraware/funlen)
//! (golangci-lint wrapper in `pkg/golinters/funlen`).
//!
//! Defaults match ultraware/golangci when settings are unset:
//! `lines=60`, `statements=40`, `ignore-comments=true`.
//!
//! The production typecheck parses without `PARSE_COMMENTS`, so `file.comments`
//! is empty and `ignore-comments` had nothing to subtract. Files are re-parsed
//! with comments on demand — only for functions that exceed the limit *before*
//! subtracting comments, since subtracting can only lower the count.

use std::fs;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::ast::{Decl, Expr, File, FuncDecl, Stmt};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::FunlenOptions;

/// `cached` is the package's already-read source bytes; re-opening the file the
/// type-checker just read makes the kernel do the work twice (PERF_TASKS_V3
/// V1-4).
fn reparse_with_comments(path: &Path, cached: Option<&[u8]>) -> Option<(Arc<FileSet>, File)> {
    let owned;
    let src: &[u8] = match cached {
        Some(b) => b,
        None => {
            owned = fs::read(path).ok()?;
            &owned
        }
    };
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

/// Comments inside `[start_line, end_line]`, counted the way ultraware/funlen
/// does: one per `ast.Comment` in each enclosed group, so a `/* … */` block
/// counts once no matter how many lines it spans.
fn comment_count_in_lines(
    fset: &FileSet,
    comments: &[guff::ast::CommentGroup],
    start_line: i64,
    end_line: i64,
) -> usize {
    let mut count = 0usize;
    for group in comments {
        let c_start = fset.position(group.pos()).line;
        let c_end = fset.position(group.end()).line;
        if c_start > start_line && c_end < end_line {
            count += group.list.len();
        }
    }
    count
}

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

/// Raw line span of `f`, before any comment subtraction.
fn raw_lines(fset: &FileSet, f: &FuncDecl) -> (i64, i64, usize) {
    let start_line = fset.position(func_pos(f)).line;
    let end_line = fset.position(func_end(f)).line;
    // Line is i64; do not use saturating_sub(1) on zero (that yields -1 → usize::MAX).
    // ultraware/funlen counts lines strictly between the signature and closing brace.
    let count = if end_line > start_line + 1 {
        (end_line - start_line - 1) as usize
    } else {
        0
    };
    (start_line, end_line, count)
}

fn check_func(
    fset: &FileSet,
    file_comments: &[guff::ast::CommentGroup],
    reparsed: Option<&(Arc<FileSet>, File)>,
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

    let (start_line, end_line, raw) = raw_lines(fset, decl);
    if raw <= options.lines {
        // Subtracting comments only lowers the count, so this function is
        // already under the limit — no need to look at comments at all.
        return;
    }

    let lines = if options.ignore_comments {
        // Prefer the in-pass comments when the file was parsed with them;
        // otherwise fall back to the on-demand re-parse. Line numbers are
        // identical between the two parses of the same source.
        let comments = if file_comments.is_empty() {
            reparsed.map_or(0, |(re_fset, re_file)| {
                comment_count_in_lines(re_fset, &re_file.comments, start_line, end_line)
            })
        } else {
            comment_count_in_lines(fset, file_comments, start_line, end_line)
        };
        raw.saturating_sub(comments)
    } else {
        raw
    };

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
    let paths = pass.pkg().compiled_go_files.clone();
    for (i, file) in pass.files().iter().enumerate() {
        // Re-parse only when a function is actually over the limit and the
        // pass ASTs carry no comments, so the common case reads no files.
        let mut reparsed = None;
        if options.ignore_comments && file.comments.is_empty() {
            let over_limit = file.decls.iter().any(|d| match d {
                Decl::FuncDecl(f) => {
                    f.body.is_some() && raw_lines(&fset, f).2 > options.lines
                }
                _ => false,
            });
            if over_limit {
                reparsed = paths
                    .get(i)
                    .and_then(|p| reparse_with_comments(p, pass.pkg().source_bytes(i)));
            }
        }
        for decl in &file.decls {
            if let Decl::FuncDecl(f) = decl {
                check_func(
                    &fset,
                    &file.comments,
                    reparsed.as_ref(),
                    f,
                    options,
                    &mut pending,
                );
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
