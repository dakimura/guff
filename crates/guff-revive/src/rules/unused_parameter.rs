//! `unused-parameter` — warn on unused function parameters.

use guff::ast::{Expr, FuncDecl, FuncLit, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::config::AllowRegex;
use crate::failure::Failure;
use crate::util::is_blank;

pub struct Checker {
    allow: AllowRegex,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new(pass: &Pass<'_>) -> Self {
        Self {
            allow: AllowRegex::new(pass, "unused-parameter"),
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::FuncDecl(f) => {
                if let Some(body) = &f.body {
                    if let Some(params) = &f.ty.params {
                        check_func(&params.list, body, &self.allow, &mut self.failures);
                    }
                }
            }
            NodeRef::FuncLit(f) => {
                if let Some(params) = &f.ty.params {
                    check_func(&params.list, &f.body, &self.allow, &mut self.failures);
                }
            }
            _ => {}
        }
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

fn check_func(
    params: &[guff::ast::Field],
    body: &guff::ast::BlockStmt,
    allow: &AllowRegex,
    failures: &mut Vec<Failure>,
) {
    let mut unused: Vec<(String, i64)> = Vec::new();
    for field in params {
        for name in &field.names {
            if is_blank(name) {
                continue;
            }
            unused.push((name.name.clone(), name.name_pos.0));
        }
    }
    if unused.is_empty() {
        return;
    }
    let mut used = std::collections::HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(n) = n else {
            return true;
        };
        if let NodeRef::Ident(id) = n {
            used.insert(id.name.clone());
        }
        true
    });
    for (name, pos) in unused {
        // Upstream skips the allowed names before it ever looks at whether the
        // parameter is used, so a name the regex accepts is never a finding.
        if allow.allows(&name) {
            continue;
        }
        if !used.contains(&name) {
            let message = match &allow.configured {
                Some(pattern) => format!(
                    "parameter '{name}' seems to be unused, consider removing or renaming it to match {pattern}"
                ),
                None => format!(
                    "parameter '{name}' seems to be unused, consider removing or renaming it as _"
                ),
            };
            failures.push(Failure {
                rule: "unused-parameter",
                pos: pos as u32,
                message,
                ..Failure::default()
            });
        }
    }
}
