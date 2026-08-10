//! `unexported-naming` — warn when local symbols use exported (uppercase) names.

use guff::ast::{AssignStmt, Decl, Expr, File, Ident, Spec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_exported_ident, unparen};

pub struct Checker {
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            failures: Vec::new(),
        }
    }

    pub fn on_file(&mut self, file: &File) {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            lint_fields(&f.ty.params, &mut self.failures);
            lint_fields(&f.ty.results, &mut self.failures);
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        match n {
            NodeRef::FuncLit(f) => {
                lint_fields(&f.ty.params, &mut self.failures);
                lint_fields(&f.ty.results, &mut self.failures);
            }
            NodeRef::AssignStmt(a) => lint_assign(a, &mut self.failures),
            // Upstream only reaches value declarations through `*ast.DeclStmt`,
            // i.e. `var`/`const` *inside a function body*: package-level ones are
            // the exported API and are none of this rule's business. It also looks
            // at `gd.Specs[0]` alone, so `var ( A = 1; B = 2 )` in a body reports A
            // and stays quiet about B.
            NodeRef::DeclStmt(ds) => {
                let Decl::GenDecl(gd) = &ds.decl else {
                    return;
                };
                let Some(Spec::ValueSpec(vs)) = gd.specs.first() else {
                    return;
                };
                for id in &vs.names {
                    lint_ident(id, &mut self.failures);
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
    let mut c = Checker::new();
    for file in pass.files() {
        c.on_file(file);
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn lint_assign(a: &AssignStmt, failures: &mut Vec<Failure>) {
    if a.tok != Some(Token::DEFINE) {
        return;
    }
    for lhs in &a.lhs {
        if let Expr::Ident(id) = unparen(lhs) {
            lint_ident(id, failures);
        }
    }
}

fn lint_fields(fields: &Option<guff::ast::FieldList>, failures: &mut Vec<Failure>) {
    let Some(fields) = fields else {
        return;
    };
    for field in &fields.list {
        for id in &field.names {
            lint_ident(id, failures);
        }
    }
}

fn lint_ident(id: &Ident, failures: &mut Vec<Failure>) {
    if is_exported_ident(&id.name) {
        failures.push(Failure {
            rule: "unexported-naming",
            pos: id.name_pos.0 as u32,
            message: format!(
                "the symbol {} is local, its name should start with a lowercase letter",
                id.name
            ),
            ..Failure::default()
        });
    }
}
