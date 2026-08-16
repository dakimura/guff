//! `range-val-in-closure` — warn when loop variables are captured by goroutine/defer closures.

use guff::ast::{AssignStmt, Expr, ForStmt, FuncLit, GoStmt, Ident, IncDecStmt, RangeStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{go_version_at_least, unparen};

/// Upstream bails out on `file.Pkg.IsAtLeastGoVersion(lint.Go122)`: since Go
/// 1.22 each iteration gets its own copy of the loop variable, so capturing it
/// is no longer a bug.
pub fn applies(pass: &Pass<'_>) -> bool {
    !go_version_at_least(pass, 1, 22)
}

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::RangeStmt(r) => {
                check_loop(r.body.list.last(), loop_vars_from_range(r), &mut self.failures)
            }
            NodeRef::ForStmt(f) => {
                let vars = loop_vars_from_for(f);
                check_loop(f.body.list.last(), vars, &mut self.failures);
            }
            _ => {}
        }
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    if !applies(pass) {
        return Vec::new();
    }
    let mut c = Checker::new();
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

fn loop_vars_from_range(r: &RangeStmt) -> Vec<String> {
    let mut vars = Vec::new();
    if let Some(Expr::Ident(id)) = r.key.as_ref().map(|e| unparen(e)) {
        if id.name != "_" {
            vars.push(id.name.clone());
        }
    }
    if let Some(Expr::Ident(id)) = r.value.as_ref().map(|e| unparen(e)) {
        if id.name != "_" {
            vars.push(id.name.clone());
        }
    }
    vars
}

fn loop_vars_from_for(f: &ForStmt) -> Vec<String> {
    let mut vars = Vec::new();
    if let Some(post) = &f.post {
        match post.as_ref() {
            Stmt::AssignStmt(a) => {
                for lhs in &a.lhs {
                    if let Expr::Ident(id) = unparen(lhs) {
                        vars.push(id.name.clone());
                    }
                }
            }
            Stmt::IncDecStmt(i) => {
                if let Expr::Ident(id) = unparen(&i.x) {
                    vars.push(id.name.clone());
                }
            }
            _ => {}
        }
    }
    vars
}

fn check_loop(last: Option<&Stmt>, vars: Vec<String>, failures: &mut Vec<Failure>) {
    if vars.is_empty() {
        return;
    }
    let Some(last) = last else {
        return;
    };
    let call = match last {
        Stmt::GoStmt(g) => &g.call,
        Stmt::DeferStmt(d) => &d.call,
        _ => return,
    };
    let Expr::FuncLit(lit) = unparen(&call.fun) else {
        return;
    };
    let body = &lit.body;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::Ident(Ident { name, name_pos, .. })) = n else {
            return true;
        };
        if name != "_" && vars.iter().any(|v| v == name) {
            failures.push(Failure {
                rule: "range-val-in-closure",
                pos: name_pos.0 as u32,
                message: format!("loop variable {name} captured by func literal"),
                ..Failure::default()
            });
            return false;
        }
        true
    });
}
