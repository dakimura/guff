//! Port of [`github.com/nakabonne/nestif`](https://github.com/nakabonne/nestif)
//! (golangci-lint wrapper in `pkg/golinters/nestif`).
//!
//! Default matches golangci-lint: `min-complexity=5` (report when complexity
//! is greater than or equal to this).
//!
//! DEFERRED: `linters.settings.nestif.min-complexity` wiring.

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{Decl, Expr, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// golangci-lint default for `linters.settings.nestif.min-complexity`.
const MIN_COMPLEXITY: usize = 5;

fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::CallExpr(c) => {
            let args: Vec<String> = c.args.iter().map(expr_string).collect();
            format!("{}({})", expr_string(&c.fun), args.join(", "))
        }
        Expr::ParenExpr(p) => format!("({})", expr_string(&p.x)),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::UnaryExpr(u) => format!("{}{}", u.op, expr_string(&u.x)),
        Expr::BinaryExpr(b) => format!("{} {} {}", expr_string(&b.x), b.op, expr_string(&b.y)),
        Expr::IndexExpr(i) => format!("{}[{}]", expr_string(&i.x), expr_string(&i.index)),
        Expr::BasicLit(l) => l.value.clone(),
        _ => "<expr>".into(),
    }
}

struct NestVisitor {
    complexity: usize,
    nesting: usize,
    elseifs: HashSet<u32>,
}

impl NestVisitor {
    fn new() -> Self {
        Self {
            complexity: 0,
            nesting: 0,
            elseifs: HashSet::new(),
        }
    }

    fn visit_if(&mut self, stmt: &guff::ast::IfStmt) {
        self.inc_complexity(stmt);

        self.nesting += 1;
        for s in &stmt.body.list {
            self.walk_stmt(s);
        }
        self.nesting = self.nesting.saturating_sub(1);

        match stmt.else_.as_deref() {
            Some(Stmt::BlockStmt(b)) => {
                self.complexity += 1;
                self.nesting += 1;
                for s in &b.list {
                    self.walk_stmt(s);
                }
                self.nesting = self.nesting.saturating_sub(1);
            }
            Some(Stmt::IfStmt(else_if)) => {
                if else_if.id != 0 {
                    self.elseifs.insert(else_if.id);
                }
                self.visit_if(else_if);
            }
            Some(other) => self.walk_stmt(other),
            None => {}
        }
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::IfStmt(i) => self.visit_if(i),
            Stmt::BlockStmt(b) => {
                for s in &b.list {
                    self.walk_stmt(s);
                }
            }
            Stmt::ForStmt(f) => {
                for s in &f.body.list {
                    self.walk_stmt(s);
                }
            }
            Stmt::RangeStmt(r) => {
                for s in &r.body.list {
                    self.walk_stmt(s);
                }
            }
            Stmt::SwitchStmt(s) => {
                for st in &s.body.list {
                    self.walk_stmt(st);
                }
            }
            Stmt::TypeSwitchStmt(s) => {
                for st in &s.body.list {
                    self.walk_stmt(st);
                }
            }
            Stmt::SelectStmt(s) => {
                for st in &s.body.list {
                    self.walk_stmt(st);
                }
            }
            Stmt::CaseClause(c) => {
                for s in &c.body {
                    self.walk_stmt(s);
                }
            }
            Stmt::CommClause(c) => {
                if let Some(comm) = &c.comm {
                    self.walk_stmt(comm);
                }
                for s in &c.body {
                    self.walk_stmt(s);
                }
            }
            Stmt::LabeledStmt(l) => self.walk_stmt(&l.stmt),
            Stmt::ExprStmt(e) => self.walk_expr(&e.x),
            Stmt::AssignStmt(a) => {
                for e in &a.rhs {
                    self.walk_expr(e);
                }
            }
            Stmt::GoStmt(g) => self.walk_expr(&Expr::CallExpr(g.call.clone())),
            Stmt::DeferStmt(d) => self.walk_expr(&Expr::CallExpr(d.call.clone())),
            Stmt::ReturnStmt(r) => {
                for e in &r.results {
                    self.walk_expr(e);
                }
            }
            _ => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FuncLit(lit) => {
                for s in &lit.body.list {
                    self.walk_stmt(s);
                }
            }
            Expr::CallExpr(c) => {
                self.walk_expr(&c.fun);
                for a in &c.args {
                    self.walk_expr(a);
                }
            }
            Expr::ParenExpr(p) => self.walk_expr(&p.x),
            Expr::UnaryExpr(u) => self.walk_expr(&u.x),
            Expr::StarExpr(s) => self.walk_expr(&s.x),
            Expr::SelectorExpr(s) => self.walk_expr(&s.x),
            Expr::BinaryExpr(b) => {
                self.walk_expr(&b.x);
                self.walk_expr(&b.y);
            }
            Expr::IndexExpr(i) => {
                self.walk_expr(&i.x);
                self.walk_expr(&i.index);
            }
            Expr::CompositeLit(c) => {
                for el in &c.elts {
                    self.walk_expr(el);
                }
            }
            Expr::KeyValueExpr(kv) => {
                self.walk_expr(&kv.key);
                self.walk_expr(&kv.value);
            }
            _ => {}
        }
    }

    fn inc_complexity(&mut self, n: &guff::ast::IfStmt) {
        if n.id != 0 && self.elseifs.contains(&n.id) {
            self.complexity += 1;
        } else {
            self.complexity += self.nesting;
        }
    }
}

fn check_if(stmt: &guff::ast::IfStmt) -> Option<(u32, String)> {
    let mut v = NestVisitor::new();
    v.visit_if(stmt);
    if v.complexity < MIN_COMPLEXITY {
        return None;
    }
    Some((
        stmt.if_.0 as u32,
        format!(
            "`if {}` has complex nested blocks (complexity: {})",
            expr_string(&stmt.cond),
            v.complexity
        ),
    ))
}

fn find_root_ifs(stmt: &Stmt, pending: &mut Vec<(u32, String)>) {
    walk::inspect(walk::stmt_ref(stmt), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::IfStmt(if_stmt) = n {
            if let Some(issue) = check_if(if_stmt) {
                pending.push(issue);
            }
            return false;
        }
        true
    });
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nestif requires inspect analyzer".to_string())?;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let Some(body) = &f.body else {
                continue;
            };
            for stmt in &body.list {
                find_root_ifs(stmt, &mut pending);
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
        name: "nestif",
        doc: "reports deeply nested if statements",
        url: "https://github.com/nakabonne/nestif",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
