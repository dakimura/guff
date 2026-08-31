//! `datarace` — spot goroutines capturing named returns or range variables.
//!
//! Upstream keys on `*ast.Object`, the identity `go/parser` gives a declared
//! name, so an inner declaration that reuses an outer name is a different key
//! and never matches. guff has no `*ast.Object`; the equivalent identity is the
//! type-checker's object, which [`guff_analysis::code::object_of`] resolves
//! through `Info.defs` and `Info.uses`. Keying on the *name* instead made every
//! shadowing declaration a capture: fiber's
//! `func(addr net.Addr) { addrChan <- addr.String() }`, nested two closures
//! inside a function whose named result is also `addr`, drew two.

use std::collections::HashSet;

use guff::ast::{Decl, Expr, Field, FuncDecl, FuncLit, GoStmt, Ident, RangeStmt, Stmt};
use guff::walk::{self, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::Pass;
use guff_types::ObjectId;

use crate::failure::Failure;
use crate::util::{go_version_at_least, unparen};

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let go122_for = go_version_at_least(pass, 1, 22);
    let mut failures = Vec::new();
    for file in pass.files() {
        for decl in &file.decls {
            let Decl::FuncDecl(f) = decl else {
                continue;
            };
            check_func(pass, f, go122_for, &mut failures);
        }
    }
    failures
}

fn check_func(pass: &Pass<'_>, f: &FuncDecl, go122_for: bool, failures: &mut Vec<Failure>) {
    let Some(body) = &f.body else {
        return;
    };
    let return_ids = extract_return_ids(pass, f);
    let mut range_ids: HashSet<ObjectId> = HashSet::new();
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        match n {
            Some(NodeRef::RangeStmt(r)) => {
                let ids = range_var_ids(pass, r);
                for id in &ids {
                    range_ids.insert(*id);
                }
                walk::inspect(NodeRef::BlockStmt(&r.body), |inner| {
                    if let Some(NodeRef::GoStmt(go)) = inner {
                        check_go_stmt(pass, go, &return_ids, &range_ids, go122_for, failures);
                    }
                    true
                });
                for id in ids {
                    range_ids.remove(&id);
                }
                false
            }
            Some(NodeRef::GoStmt(go)) => {
                check_go_stmt(pass, go, &return_ids, &range_ids, go122_for, failures);
                true
            }
            _ => true,
        }
    });
}

/// The type-checker objects the named results declare — upstream's
/// `extractReturnIDs`, which collects `*ast.Object`s.
fn extract_return_ids(pass: &Pass<'_>, f: &FuncDecl) -> HashSet<ObjectId> {
    let mut out = HashSet::new();
    let Some(results) = &f.ty.results else {
        return out;
    };
    for field in &results.list {
        for name in &field.names {
            if name.name == "_" {
                continue;
            }
            if let Some(obj) = object_of(pass, name) {
                out.insert(obj);
            }
        }
    }
    out
}

fn range_var_ids(pass: &Pass<'_>, r: &RangeStmt) -> Vec<ObjectId> {
    let mut out = Vec::new();
    for expr in [&r.key, &r.value] {
        let Some(expr) = expr.as_ref() else {
            continue;
        };
        if let Expr::Ident(id) = unparen(expr) {
            if id.name == "_" {
                continue;
            }
            if let Some(obj) = object_of(pass, id) {
                out.push(obj);
            }
        }
    }
    out
}

fn check_go_stmt(
    pass: &Pass<'_>,
    go: &GoStmt,
    return_ids: &HashSet<ObjectId>,
    range_ids: &HashSet<ObjectId>,
    go122_for: bool,
    failures: &mut Vec<Failure>,
) {
    let Expr::FuncLit(lit) = unparen(&go.call.fun) else {
        return;
    };
    let body = &lit.body;
    walk::inspect(NodeRef::BlockStmt(body), |n| {
        let Some(NodeRef::Ident(id)) = n else {
            return true;
        };
        if id.name == "_" {
            return true;
        }
        // No object means nothing upstream's `id.Obj` could have matched
        // either: a selector's field or method name, a package name, a label.
        let Some(obj) = object_of(pass, id) else {
            return true;
        };
        let name = &id.name;
        if !go122_for && range_ids.contains(&obj) {
            failures.push(Failure {
                rule: "datarace",
                pos: id.name_pos.0 as u32,
                message: format!("datarace: range value {name} is captured (by-reference) in goroutine"),
                ..Failure::default()
            });
            return false;
        }
        if return_ids.contains(&obj) {
            failures.push(Failure {
                rule: "datarace",
                pos: id.name_pos.0 as u32,
                message: format!(
                    "potential datarace: return value {name} is captured (by-reference) in goroutine"
                ),
                ..Failure::default()
            });
            return false;
        }
        true
    });
}
