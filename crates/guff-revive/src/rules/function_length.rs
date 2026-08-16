//! `function-length` — warn on functions exceeding statement/line limits (50/75 default).

use guff::ast::{BlockStmt, Expr, File, FuncDecl, FuncLit, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_STMTS: usize = 50;
const MAX_LINES: usize = 75;

pub struct Checker<'a> {
    pass: &'a Pass<'a>,
    failures: Vec<Failure>,
}

impl<'a> Checker<'a> {
    pub fn new(pass: &'a Pass<'a>) -> Self {
        Self {
            pass,
            failures: Vec::new(),
        }
    }

    /// This rule is file-scoped, not node-scoped: one empty-bodied function
    /// silences the whole file (see [`check_file`]), which a node-at-a-time
    /// visitor cannot express. All the work happens here.
    pub fn on_file(&mut self, file: &File) {
        check_file(self.pass, file, &mut self.failures);
    }

    pub fn visit(&mut self, _n: NodeRef<'_>) {}

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut out = Vec::new();
    for file in pass.files() {
        check_file(pass, file, &mut out);
    }
    out
}

fn check_file(pass: &Pass<'_>, file: &File, out: &mut Vec<Failure>) {
    {
        // Upstream walks `file.AST.Decls` itself and bails out of the *whole
        // file* on the first function with an empty body:
        //
        //     emptyBody := body == nil || len(body.List) == 0
        //     if emptyBody { return nil }
        //
        // `return nil` rather than `continue`, inside `Apply`, which runs per
        // file — so one `func f() {}` silences function-length for every
        // function below it *and discards the failures already collected*.
        // It reads like a slip for `continue`, but it is what golangci-lint
        // 2.12.2 ships, and reproducing it is the whole point of this tier:
        // extended_bad.go has empty-bodied functions near the top, so upstream
        // reports nothing there at all.
        let mut per_file = Vec::new();
        let mut aborted = false;
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            let empty_body = f.body.as_ref().is_none_or(|b| b.list.is_empty());
            if empty_body {
                aborted = true;
                break;
            }
            check_func(pass, f, &mut per_file);
        }
        if !aborted {
            out.append(&mut per_file);
        }
    }
}

fn check_func(pass: &Pass<'_>, f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    let stmt_count = count_stmts(&body.list);
    if stmt_count > MAX_STMTS {
        failures.push(Failure {
            rule: "function-length",
            pos: f.ty.func.0 as u32,
            message: format!(
                "maximum number of statements per function exceeded; max {MAX_STMTS} but got {stmt_count}"
            ),
            ..Failure::default()
        });
    }
    let line_count = count_lines(pass, body);
    if line_count > MAX_LINES {
        failures.push(Failure {
            rule: "function-length",
            pos: f.ty.func.0 as u32,
            message: format!(
                "maximum number of lines per function exceeded; max {MAX_LINES} but got {line_count}"
            ),
            ..Failure::default()
        });
    }
}

fn count_lines(pass: &Pass<'_>, body: &BlockStmt) -> usize {
    let start = pass.fset().position(body.lbrace).line;
    let end = pass.fset().position(body.rbrace).line;
    end.saturating_sub(start).saturating_sub(1).max(0) as usize
}

fn count_stmts(stmts: &[Stmt]) -> usize {
    let mut count = 0;
    for stmt in stmts {
        match stmt {
            Stmt::BlockStmt(b) => count += count_stmts(&b.list),
            Stmt::IfStmt(i) => {
                count += 1 + count_body_stmts(&i.body.list);
                if let Some(Stmt::BlockStmt(else_block)) = i.else_.as_deref() {
                    count += count_stmts(&else_block.list);
                }
            }
            Stmt::ForStmt(f) => {
                count += 1 + count_body_stmts(&f.body.list);
            }
            Stmt::RangeStmt(r) => {
                count += 1 + count_body_stmts(&r.body.list);
            }
            Stmt::SwitchStmt(s) => {
                count += 1 + count_body_stmts(&s.body.list);
            }
            Stmt::TypeSwitchStmt(s) => {
                count += 1 + count_body_stmts(&s.body.list);
            }
            Stmt::SelectStmt(s) => {
                count += 1 + count_body_stmts(&s.body.list);
            }
            Stmt::CaseClause(c) => {
                count += count_stmts(&c.body);
            }
            Stmt::CommClause(c) => {
                count += count_stmts(&c.body);
            }
            Stmt::AssignStmt(a) => {
                count += 1;
                if let Some(rhs) = a.rhs.first() {
                    count += count_func_lit_stmts(rhs);
                }
            }
            Stmt::GoStmt(g) => count += 1 + count_func_lit_stmts(&g.call.fun),
            Stmt::DeferStmt(d) => count += 1 + count_func_lit_stmts(&d.call.fun),
            _ => count += 1,
        }
    }
    count
}

fn count_body_stmts(stmts: &[Stmt]) -> usize {
    count_stmts(stmts)
}

fn count_func_lit_stmts(expr: &Expr) -> usize {
    if let Expr::FuncLit(lit) = expr {
        count_stmts(&lit.body.list)
    } else {
        0
    }
}
