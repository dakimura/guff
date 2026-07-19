//! `deep-exit` — disallow program exit calls outside `main` / `init`.

use guff::ast::{CallExpr, Expr, FuncDecl};
use guff::walk::{self, NodeRef};
use guff_analysis::Pass;

use crate::failure::Failure;
use crate::util::{is_pkg_dot_name, is_test_package, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let is_test = pass
        .files()
        .first()
        .map(|f| is_test_package(&f.name.name))
        .unwrap_or(false);
    let mut failures = Vec::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            let Some(NodeRef::FuncDecl(f)) = n else {
                return true;
            };
            if must_ignore(f, is_test) {
                return false;
            }
            if let Some(body) = &f.body {
                walk::inspect(NodeRef::BlockStmt(body), |inner| {
                    let Some(NodeRef::ExprStmt(stmt)) = inner else {
                        return true;
                    };
                    let Expr::CallExpr(call) = &stmt.x else {
                        return true;
                    };
                    if let Some((pkg, name)) = exit_call(call) {
                        let msg = if pkg == "flag"
                            && name == "NewFlagSet"
                            && call.args.len() == 2
                            && is_pkg_dot_name(&call.args[1], "flag", "ExitOnError")
                        {
                            "calls to flag.NewFlagSet with flag.ExitOnError only in main() or init() functions".into()
                        } else {
                            format!("calls to {pkg}.{name} only in main() or init() functions")
                        };
                        failures.push(Failure {
                            rule: "deep-exit",
                            pos: call.fun.pos().0 as u32,
                            message: msg,
            confidence: None,
        });
                    }
                    true
                });
            }
            false
        });
    }
    failures
}

fn must_ignore(f: &FuncDecl, is_test: bool) -> bool {
    let name = &f.name.name;
    if name == "init" || name == "main" {
        return true;
    }
    if is_test && name == "TestMain" {
        return true;
    }
    if is_test && is_test_example(name, f) {
        return true;
    }
    false
}

fn is_test_example(name: &str, f: &FuncDecl) -> bool {
    const PREFIX: &str = "Example";
    if !name.starts_with(PREFIX) {
        return false;
    }
    if name.len() == PREFIX.len() {
        return f.ty.params.as_ref().is_none_or(|p| p.list.is_empty());
    }
    let rest = &name[PREFIX.len()..];
    rest.chars()
        .next()
        .is_some_and(|c| c.is_uppercase())
        && f.ty.params.as_ref().is_none_or(|p| p.list.is_empty())
}

fn exit_call(call: &CallExpr) -> Option<(&str, &str)> {
    let Expr::SelectorExpr(sel) = unparen(&call.fun) else {
        return None;
    };
    let Expr::Ident(id) = unparen(&sel.x) else {
        return None;
    };
    let pkg = id.name.as_str();
    let name = sel.sel.name.as_str();
    if is_call_to_exit_function(pkg, name, call) {
        Some((pkg, name))
    } else {
        None
    }
}

fn is_call_to_exit_function(pkg: &str, name: &str, call: &CallExpr) -> bool {
    match (pkg, name) {
        ("os", "Exit") | ("syscall", "Exit") => true,
        ("log", "Fatal" | "Fatalf" | "Fatalln" | "Panic" | "Panicf" | "Panicln") => true,
        ("flag", "Parse") => true,
        ("flag", "NewFlagSet") => {
            call.args.len() == 2 && is_pkg_dot_name(&call.args[1], "flag", "ExitOnError")
        }
        _ => false,
    }
}
