//! Port of [`github.com/uudashr/gocognit`](https://github.com/uudashr/gocognit)
//! (golangci-lint wrapper in `pkg/golinters/gocognit`).
//!
//! Default matches golangci-lint: `min-complexity=30` (report when complexity
//! is strictly greater than this).
//!
//! DEFERRED: `//gocognit:ignore` directive support (needs PARSE_COMMENTS).

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BinaryExpr, CallExpr, Decl, Expr, FuncDecl, Stmt};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::GocognitOptions;

fn recv_string(expr: &Expr) -> String {
    match expr {
        Expr::Ident(id) => id.name.clone(),
        Expr::StarExpr(s) => format!("*{}", recv_string(&s.x)),
        Expr::IndexExpr(i) => recv_string(&i.x),
        Expr::IndexListExpr(i) => recv_string(&i.x),
        _ => "BADRECV".into(),
    }
}

fn func_name(fn_: &FuncDecl) -> String {
    if let Some(recv) = &fn_.recv {
        if let Some(field) = recv.list.first() {
            if let Some(ty) = &field.ty {
                return format!("({}).{}", recv_string(ty), fn_.name.name);
            }
        }
    }
    fn_.name.name.clone()
}

fn format_code(code: &str) -> String {
    if code.contains('`') {
        code.to_string()
    } else {
        format!("`{code}`")
    }
}

fn expr_id(e: &Expr) -> u32 {
    e.id()
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

    fn inc_nesting(&mut self) {
        self.nesting += 1;
    }

    fn dec_nesting(&mut self) {
        self.nesting = self.nesting.saturating_sub(1);
    }

    fn inc_complexity(&mut self) {
        self.complexity += 1;
    }

    fn nest_inc_complexity(&mut self) {
        self.complexity += self.nesting + 1;
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
            Stmt::SwitchStmt(n) => {
                self.nest_inc_complexity();
                if let Some(init) = &n.init {
                    self.walk_stmt(init);
                }
                if let Some(tag) = &n.tag {
                    self.walk_expr(tag);
                }
                self.inc_nesting();
                for s in &n.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
            }
            Stmt::TypeSwitchStmt(n) => {
                self.nest_inc_complexity();
                if let Some(init) = &n.init {
                    self.walk_stmt(init);
                }
                self.walk_stmt(&n.assign);
                self.inc_nesting();
                for s in &n.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
            }
            Stmt::SelectStmt(n) => {
                self.nest_inc_complexity();
                self.inc_nesting();
                for s in &n.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
            }
            Stmt::ForStmt(n) => {
                self.nest_inc_complexity();
                if let Some(init) = &n.init {
                    self.walk_stmt(init);
                }
                if let Some(cond) = &n.cond {
                    self.walk_expr(cond);
                }
                if let Some(post) = &n.post {
                    self.walk_stmt(post);
                }
                self.inc_nesting();
                for s in &n.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
            }
            Stmt::RangeStmt(n) => {
                self.nest_inc_complexity();
                if let Some(key) = &n.key {
                    self.walk_expr(key);
                }
                if let Some(value) = &n.value {
                    self.walk_expr(value);
                }
                self.walk_expr(&n.x);
                self.inc_nesting();
                for s in &n.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
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

    fn visit_if(&mut self, n: &guff::ast::IfStmt) {
        if self.is_else(n.id) {
            self.inc_complexity();
        } else {
            self.nest_inc_complexity();
        }

        if let Some(init) = &n.init {
            self.walk_stmt(init);
        }
        self.walk_expr(&n.cond);

        self.inc_nesting();
        for s in &n.body.list {
            self.walk_stmt(s);
        }
        self.dec_nesting();

        match n.else_.as_deref() {
            Some(Stmt::BlockStmt(b)) => {
                self.inc_complexity();
                for s in &b.list {
                    self.walk_stmt(s);
                }
            }
            Some(Stmt::IfStmt(else_if)) => {
                self.mark_else(else_if.id);
                self.visit_if(else_if);
            }
            Some(other) => self.walk_stmt(other),
            None => {}
        }
    }

    fn walk_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::FuncLit(lit) => {
                self.inc_nesting();
                for s in &lit.body.list {
                    self.walk_stmt(s);
                }
                self.dec_nesting();
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
        self.mark_calculated(expr_id(exp));
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

fn complexity(fn_: &FuncDecl) -> usize {
    let name = fn_.name.name.as_str();
    let mut v = ComplexityVisitor::new(name);
    if let Some(body) = &fn_.body {
        for s in &body.list {
            v.walk_stmt(s);
        }
    }
    v.complexity
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "gocognit requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<GocognitOptions>("gocognit")
        .copied()
        .unwrap_or_default();
    let min_complexity = options.min_complexity;

    let mut pending = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            let c = complexity(f);
            if c > min_complexity {
                let name = func_name(f);
                pending.push((
                    // gocognit records `fn.Pos()` — the `func` keyword.
                    f.ty.pos().0 as u32,
                    format!(
                        "cognitive complexity {c} of func {} is high (> {min_complexity})",
                        format_code(&name)
                    ),
                ));
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
        name: "gocognit",
        doc: "computes and checks the cognitive complexity of functions",
        url: "https://github.com/uudashr/gocognit",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}
