//! `cognitive-complexity` — restrict maximum cognitive complexity (default 7).

use std::collections::HashSet;

use guff::ast::{BinaryExpr, CallExpr, Decl, Expr, FuncDecl, Stmt};
use guff::token::Token;
use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_COMPLEXITY: i64 = 7;

/// `Configure`: `arguments[0]` is the limit, and anything that is not an
/// integer is a configuration *error* upstream. guff had the default baked in
/// as a constant and never read the argument at all.
fn max_complexity(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "cognitive-complexity", 0)
        .unwrap_or(DEFAULT_MAX_COMPLEXITY)
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let max = max_complexity(pass);
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let c = complexity(f) as i64;
            if c > max {
                failures.push(Failure {
                    rule: "cognitive-complexity",
                    pos: f.ty.func.0 as u32,
                    message: format!(
                        "function {} has cognitive complexity {} (> max enabled {})",
                        func_name(f),
                        c,
                        max
                    ),
                    ..Failure::default()
                });
            }
        }
    }
    failures
}

fn recv_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_string(&s.x)),
        _ => "BADRECV".into(),
    }
}

fn func_name(f: &FuncDecl) -> String {
    if let Some(recv) = &f.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_string(ty), f.name.name);
            }
        }
    }
    f.name.name.clone()
}

struct ComplexityVisitor<'a> {
    func_name: &'a str,
    complexity: usize,
    nesting: usize,
    else_nodes: HashSet<u32>,
    calculated_exprs: HashSet<u32>,
}

impl<'a> ComplexityVisitor<'a> {
    fn new(func_name: &'a str) -> Self {
        Self {
            func_name,
            complexity: 0,
            nesting: 0,
            else_nodes: HashSet::new(),
            calculated_exprs: HashSet::new(),
        }
    }

    /// Upstream's `walk(complexityIncrement, targets...)`:
    ///
    /// ```go
    /// v.complexity += complexityIncrement + v.nestingLevel
    /// nesting := v.nestingLevel
    /// v.nestingLevel++
    /// for _, t := range targets { ast.Walk(v, t) }
    /// v.nestingLevel = nesting
    /// ```
    ///
    /// The increment is added to the *current* nesting level, which is why a
    /// function literal — increment 0 — still costs something once it is inside
    /// a loop.
    fn walk(&mut self, increment: usize, body: impl FnOnce(&mut Self)) {
        self.complexity += increment + self.nesting;
        let nesting = self.nesting;
        self.nesting += 1;
        body(self);
        self.nesting = nesting;
    }

    fn inc_complexity(&mut self) {
        self.complexity += 1;
    }

    fn mark_else(&mut self, id: u32) {
        if id != 0 {
            self.else_nodes.insert(id);
        }
    }

    fn is_else(&self, id: u32) -> bool {
        id != 0 && self.else_nodes.contains(&id)
    }

    fn mark_calculated(&mut self, id: u32) {
        if id != 0 {
            self.calculated_exprs.insert(id);
        }
    }

    fn is_calculated(&self, id: u32) -> bool {
        id != 0 && self.calculated_exprs.contains(&id)
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::IfStmt(n) => self.visit_if(n),
            // `v.walk(1, n.Body)` — neither the init statement nor the tag.
            Stmt::SwitchStmt(n) => {
                self.walk(1, |v| {
                    for s in &n.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            Stmt::TypeSwitchStmt(n) => {
                self.walk(1, |v| {
                    for s in &n.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            Stmt::SelectStmt(n) => {
                self.walk(1, |v| {
                    for s in &n.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            // `targets := []ast.Node{n.Cond, n.Body}` — upstream walks the
            // condition and the body and **not** `Init` or `Post`, so a
            // logical operator in either does not count.
            Stmt::ForStmt(n) => {
                self.walk(1, |v| {
                    if let Some(cond) = &n.cond {
                        v.walk_expr(cond);
                    }
                    for s in &n.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            // `v.walk(1, n.Body)` — the key, the value and the ranged
            // expression are not walked.
            Stmt::RangeStmt(n) => {
                self.walk(1, |v| {
                    for s in &n.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            Stmt::BranchStmt(n) => {
                if n.label.is_some() {
                    self.inc_complexity();
                }
            }
            Stmt::BlockStmt(b) => {
                for s in &b.list {
                    self.walk_stmt(s);
                }
            }
            Stmt::CaseClause(c) => {
                for e in &c.list {
                    self.walk_expr(e);
                }
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
            Stmt::SendStmt(s) => {
                self.walk_expr(&s.chan_);
                self.walk_expr(&s.value);
            }
            Stmt::IncDecStmt(s) => self.walk_expr(&s.x),
            Stmt::AssignStmt(a) => {
                for e in &a.lhs {
                    self.walk_expr(e);
                }
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
            Stmt::DeclStmt(d) => {
                if let Decl::GenDecl(g) = &d.decl {
                    for spec in &g.specs {
                        if let guff::ast::Spec::ValueSpec(vs) = spec {
                            for v in &vs.values {
                                self.walk_expr(v);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Upstream's `walkIfElse`:
    ///
    /// ```go
    /// v.complexity += 1 + v.nestingLevel
    /// v.nestingLevel++
    /// w(n)   // Cond, Body, then +1 per `else if`; a plain `else` is walked
    /// v.nestingLevel--
    /// ```
    ///
    /// Two things guff had wrong. The `Init` statement is not walked at all, so
    /// `if x := a && b; x {}` is 1 and not 2. And a **plain trailing `else` adds
    /// nothing** — only an `else if` costs 1 — so an if / else-if / else-if /
    /// else chain is 3, where guff counted 4.
    fn visit_if(&mut self, n: &guff::ast::IfStmt) {
        self.complexity += 1 + self.nesting;
        let nesting = self.nesting;
        self.nesting += 1;
        self.walk_if_chain(n);
        self.nesting = nesting;
    }

    fn walk_if_chain(&mut self, n: &guff::ast::IfStmt) {
        self.walk_expr(&n.cond);
        for s in &n.body.list {
            self.walk_stmt(s);
        }
        match n.else_.as_deref() {
            Some(Stmt::IfStmt(else_if)) => {
                self.inc_complexity();
                self.walk_if_chain(else_if);
            }
            Some(other) => self.walk_stmt(other),
            None => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            // `v.walk(0, n.Body)` — "do not increment the complexity, just do
            // the nesting", but `walk` still adds the *current* nesting level.
            // guff added nothing, so every function literal below a loop
            // undercounted: `for range s { f := func() {} }` is 2 upstream and
            // was 1 here.
            Expr::FuncLit(lit) => {
                self.walk(0, |v| {
                    for s in &lit.body.list {
                        v.walk_stmt(s);
                    }
                });
            }
            Expr::BinaryExpr(b) => self.visit_binary(b),
            Expr::CallExpr(c) => self.visit_call(c),
            Expr::ParenExpr(p) => self.walk_expr(&p.x),
            Expr::UnaryExpr(u) => self.walk_expr(&u.x),
            Expr::StarExpr(s) => self.walk_expr(&s.x),
            Expr::SelectorExpr(s) => self.walk_expr(&s.x),
            Expr::IndexExpr(i) => {
                self.walk_expr(&i.x);
                self.walk_expr(&i.index);
            }
            Expr::IndexListExpr(i) => {
                self.walk_expr(&i.x);
                for idx in &i.indices {
                    self.walk_expr(idx);
                }
            }
            Expr::SliceExpr(s) => {
                self.walk_expr(&s.x);
                if let Some(low) = &s.low {
                    self.walk_expr(low);
                }
                if let Some(high) = &s.high {
                    self.walk_expr(high);
                }
                if let Some(max) = &s.max {
                    self.walk_expr(max);
                }
            }
            Expr::TypeAssertExpr(t) => {
                self.walk_expr(&t.x);
                if let Some(ty) = &t.ty {
                    self.walk_expr(ty);
                }
            }
            Expr::CompositeLit(c) => {
                if let Some(ty) = &c.ty {
                    self.walk_expr(ty);
                }
                for el in &c.elts {
                    self.walk_expr(el);
                }
            }
            Expr::KeyValueExpr(kv) => {
                self.walk_expr(&kv.key);
                self.walk_expr(&kv.value);
            }
            Expr::Ellipsis(e) => {
                if let Some(elt) = &e.elt {
                    self.walk_expr(elt);
                }
            }
            _ => {}
        }
    }

    fn visit_binary(&mut self, n: &BinaryExpr) {
        if is_binary_logical(n.op) && !self.is_calculated(n.id) {
            let ops = self.collect_binary_ops_bin(n);
            let mut last_op = None;
            for op in ops {
                if last_op != Some(op) {
                    self.inc_complexity();
                    last_op = Some(op);
                }
            }
        }
        self.walk_expr(&n.x);
        self.walk_expr(&n.y);
    }

    fn visit_call(&mut self, n: &CallExpr) {
        if let Expr::Ident(id) = n.fun.as_ref() {
            if id.name == self.func_name {
                self.inc_complexity();
            }
        }
        self.walk_expr(&n.fun);
        for arg in &n.args {
            self.walk_expr(arg);
        }
    }

    fn collect_binary_ops_bin(&mut self, b: &BinaryExpr) -> Vec<Token> {
        self.mark_calculated(b.id);
        merge_binary_ops(
            self.collect_binary_ops_expr(&b.x),
            b.op,
            self.collect_binary_ops_expr(&b.y),
        )
    }

    fn collect_binary_ops_expr(&mut self, exp: &Expr) -> Vec<Token> {
        self.mark_calculated(exp.id());
        let Expr::BinaryExpr(b) = exp else {
            return Vec::new();
        };
        merge_binary_ops(
            self.collect_binary_ops_expr(&b.x),
            b.op,
            self.collect_binary_ops_expr(&b.y),
        )
    }
}

fn is_binary_logical(op: Token) -> bool {
    op == Token::LAND || op == Token::LOR
}

fn merge_binary_ops(x: Vec<Token>, op: Token, y: Vec<Token>) -> Vec<Token> {
    let mut out = x;
    if is_binary_logical(op) {
        out.push(op);
    }
    out.extend(y);
    out
}

fn complexity(f: &FuncDecl) -> usize {
    let name = f.name.name.as_str();
    let mut v = ComplexityVisitor::new(name);
    if let Some(body) = &f.body {
        for s in &body.list {
            v.walk_stmt(s);
        }
    }
    v.complexity
}
