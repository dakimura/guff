//! `function-length` — warn on functions exceeding statement/line limits (50/75 default).

use guff::ast::{BlockStmt, Expr, FuncDecl, FuncLit, Stmt};
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

    pub fn visit(&mut self, n: NodeRef<'_>) {
        let NodeRef::FuncDecl(f) = n else {
            return;
        };
        check_func(self.pass, f, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new(pass);
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_func(pass: &Pass<'_>, f: &FuncDecl, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    if body.list.is_empty() {
        return;
    }
    let stmt_count = count_stmts(&body.list);
    if stmt_count > MAX_STMTS {
        failures.push(Failure {
            rule: "function-length",
            pos: f.name.name_pos.0 as u32,
            message: format!(
                "maximum number of statements per function exceeded; max {MAX_STMTS} but got {stmt_count}"
            ),
            confidence: None,
        });
    }
    let line_count = count_lines(pass, body);
    if line_count > MAX_LINES {
        failures.push(Failure {
            rule: "function-length",
            pos: f.name.name_pos.0 as u32,
            message: format!(
                "maximum number of lines per function exceeded; max {MAX_LINES} but got {line_count}"
            ),
            confidence: None,
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
    if let Expr::FuncLit(FuncLit { body, .. }) = expr {
        count_stmts(&body.list)
    } else {
        0
    }
}
