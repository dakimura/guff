//! `max-control-nesting` — restrict maximum nesting of control structures (default 5).

use guff::ast::{CaseClause, CommClause, Expr, FuncLit, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_NESTING: i64 = 5;

/// `Configure`: `arguments[0]` is the limit, 5 when there is none.
fn max_nesting(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "max-control-nesting", 0).unwrap_or(DEFAULT_MAX_NESTING)
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let max = max_nesting(pass);
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let guff::ast::Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(body) = &f.body else {
                continue;
            };
            let mut walker = Walker {
                max,
                nesting: 0,
                last_ctrl_pos: 0,
                failures: &mut failures,
            };
            walker.visit_block(body);
        }
    }
    failures
}

struct Walker<'a> {
    max: i64,
    nesting: usize,
    last_ctrl_pos: i64,
    failures: &'a mut Vec<Failure>,
}

impl Walker<'_> {
    fn visit_block(&mut self, block: &guff::ast::BlockStmt) {
        for stmt in &block.list {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        if self.nesting as i64 > self.max {
            self.failures.push(Failure {
                rule: "max-control-nesting",
                pos: self.last_ctrl_pos as u32,
                message: format!("control flow nesting exceeds {}", self.max),
                ..Failure::default()
            });
            return;
        }

        match stmt {
            Stmt::IfStmt(i) => {
                self.last_ctrl_pos = i.if_.0;
                self.nesting += 1;
                self.visit_block(&i.body);
                if let Some(else_branch) = &i.else_ {
                    self.visit_else(else_branch);
                }
                self.nesting -= 1;
            }
            Stmt::ForStmt(f) => {
                self.last_ctrl_pos = f.for_.0;
                self.nesting += 1;
                self.visit_block(&f.body);
                self.nesting -= 1;
            }
            // `for … range` is not a nesting level. Upstream's visitor
            // switches on IfStmt / ForStmt / CaseClause / CommClause / FuncLit
            // and nothing else; a `*ast.RangeStmt` falls through to the default
            // `return w`, so `ast.Walk` descends into its body with the counter
            // untouched — and `lastCtrlStmt`, which is where the failure is
            // reported, keeps pointing at the enclosing control statement.
            // jaeger nests four `range` loops and gets no finding for it.
            Stmt::RangeStmt(r) => self.visit_block(&r.body),
            Stmt::CaseClause(CaseClause { body, case, .. }) => {
                self.last_ctrl_pos = case.0;
                self.nesting += 1;
                for s in body {
                    self.visit_stmt(s);
                }
                self.nesting -= 1;
            }
            Stmt::CommClause(CommClause { body, case, .. }) => {
                self.last_ctrl_pos = case.0;
                self.nesting += 1;
                for s in body {
                    self.visit_stmt(s);
                }
                self.nesting -= 1;
            }
            Stmt::SwitchStmt(s) => {
                for inner in &s.body.list {
                    self.visit_stmt(inner);
                }
            }
            Stmt::SelectStmt(s) => {
                for inner in &s.body.list {
                    self.visit_stmt(inner);
                }
            }
            Stmt::TypeSwitchStmt(s) => {
                for inner in &s.body.list {
                    self.visit_stmt(inner);
                }
            }
            Stmt::AssignStmt(a) => {
                if let Some(rhs) = a.rhs.first() {
                    self.visit_func_lit_expr(rhs);
                }
            }
            Stmt::GoStmt(g) => self.visit_func_lit_expr(&g.call.fun),
            Stmt::DeferStmt(d) => self.visit_func_lit_expr(&d.call.fun),
            _ => {}
        }
    }

    fn visit_else(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::BlockStmt(b) => self.visit_block(b),
            Stmt::IfStmt(i) => self.visit_stmt(&Stmt::IfStmt(i.clone())),
            _ => {}
        }
    }

    fn visit_func_lit_expr(&mut self, expr: &Expr) {
        if let Expr::FuncLit(lit) = expr {
            let body = &lit.body;
            let mut inner = Walker {
                max: self.max,
                nesting: 0,
                last_ctrl_pos: 0,
                failures: self.failures,
            };
            inner.visit_block(body);
        }
    }
}
