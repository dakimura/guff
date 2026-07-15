//! `var-naming` — naming conventions for identifiers.

use guff::ast::{AssignStmt, Decl, Expr, FuncDecl, GenDecl, RangeStmt, Spec, TypeSpec, ValueSpec};
use guff::token::Token;
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::names::{canonical_name, is_upper_underscore};
use crate::util::{is_blank, unparen};

const KNOWN_EXCEPTIONS: &[&str] = &["LastInsertId", "kWh"];

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(n) = n else {
                return true;
            };
            match n {
                NodeRef::AssignStmt(a) => check_assign(a, &mut failures),
                NodeRef::FuncDecl(f) => check_func(f, &mut failures),
                NodeRef::GenDecl(g) => check_gen(g, &mut failures),
                NodeRef::RangeStmt(r) => check_range(r, &mut failures),
                NodeRef::StructType(s) => {
                    for field in &s.fields.list {
                        for name in &field.names {
                            check(name, "struct field", &mut failures);
                        }
                    }
                }
                _ => {}
            }
            true
        });
    }
    failures
}

fn check_assign(a: &AssignStmt, failures: &mut Vec<Failure>) {
    if a.tok == Some(Token::ASSIGN) {
        return;
    }
    for lhs in &a.lhs {
        if let Expr::Ident(id) = lhs {
            check(id, "var", failures);
        }
    }
}

fn check_func(f: &FuncDecl, failures: &mut Vec<Failure>) {
    let name = &f.name.name;
    if is_test_func(name) {
        return;
    }
    let thing = if f.recv.is_some() { "method" } else { "func" };
    check(&f.name, thing, failures);
    if let Some(params) = &f.ty.params {
        check_field_list(params, &format!("{thing} parameter"), failures);
    }
    if let Some(results) = &f.ty.results {
        check_field_list(results, &format!("{thing} result"), failures);
    }
}

fn is_test_func(name: &str) -> bool {
    name.starts_with("Example")
        || name.starts_with("Test")
        || name.starts_with("Benchmark")
        || name.starts_with("Fuzz")
}

fn check_gen(g: &GenDecl, failures: &mut Vec<Failure>) {
    if g.tok == Some(Token::IMPORT) {
        return;
    }
    let thing = match g.tok {
        Some(Token::CONST) => "const",
        Some(Token::TYPE) => "type",
        Some(Token::VAR) => "var",
        _ => return,
    };
    for spec in &g.specs {
        match spec {
            Spec::TypeSpec(TypeSpec { name, .. }) => check(name, thing, failures),
            Spec::ValueSpec(ValueSpec { names, .. }) => {
                for id in names {
                    check(id, thing, failures);
                }
            }
            _ => {}
        }
    }
}

fn check_range(r: &RangeStmt, failures: &mut Vec<Failure>) {
    if r.tok == Some(Token::ASSIGN) {
        return;
    }
    if let Some(Expr::Ident(id)) = r.key.as_ref() {
        check(id, "range var", failures);
    }
    if let Some(Expr::Ident(id)) = r.value.as_ref() {
        check(id, "range var", failures);
    }
}

fn check_field_list(list: &guff::ast::FieldList, thing: &str, failures: &mut Vec<Failure>) {
    for field in &list.list {
        for id in &field.names {
            check(id, thing, failures);
        }
    }
}

fn check(id: &guff::ast::Ident, thing: &str, failures: &mut Vec<Failure>) {
    if is_blank(id) || KNOWN_EXCEPTIONS.contains(&id.name.as_str()) {
        return;
    }
    if is_upper_underscore(&id.name) {
        failures.push(Failure {
            rule: "var-naming",
            pos: id.name_pos.0 as u32,
            message: "don't use ALL_CAPS in Go names; use CamelCase".into(),
        });
        return;
    }
    let should = canonical_name(&id.name);
    if id.name == should {
        return;
    }
    if id.name.len() > 2 && id.name[1..].contains('_') {
        failures.push(Failure {
            rule: "var-naming",
            pos: id.name_pos.0 as u32,
            message: format!(
                "don't use underscores in Go names; {thing} {} should be {should}",
                id.name
            ),
        });
        return;
    }
    failures.push(Failure {
        rule: "var-naming",
        pos: id.name_pos.0 as u32,
        message: format!("{thing} {} should be {should}", id.name),
    });
}
