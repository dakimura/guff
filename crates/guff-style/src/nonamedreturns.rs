//! Port of [`github.com/firefart/nonamedreturns`](https://github.com/firefart/nonamedreturns)
//! (golangci-lint wrapper in `pkg/golinters/nonamedreturns`).
//!
//! Reports named results in function / method / func-literal signatures.
//!
//! Default: every named return is reported, except a named `error` that is
//! referenced inside a `defer` and assigned somewhere in the body (including
//! via `return expr`).
//!
//! Settings (`linters.settings.nonamedreturns`):
//! - `report-error-in-defer` (default false): disable the error-in-defer exemption
//! - `allow-unused-named-returns` (default false): allow unused named results;
//!   report only when referenced in the body or used by a naked return

use std::collections::HashSet;
use std::sync::OnceLock;

use guff::ast::{BlockStmt, CallExpr, ChanDir, Decl, Expr, FieldList, Spec, Stmt};
use guff::token::Token;
use guff::walk::{decl_ref, preorder, stmt_ref, NodeRef};
use guff_analysis::code::object_of;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::ObjectData;
use guff_types::predicates::identical as types_identical;
use guff_types::{ObjectId, TypeId};

use crate::options::NonamedreturnsOptions;

fn universe_error_type(pass: &Pass<'_>) -> Option<TypeId> {
    let artifacts = pass.pkg().type_artifacts.as_ref()?;
    for oid in artifacts.objects.ids() {
        let ObjectData::TypeName(tn) = artifacts.objects.get(oid) else {
            continue;
        };
        if tn.name() != "error" {
            continue;
        }
        if oid.pkg(&artifacts.objects).is_some() {
            continue;
        }
        return tn.typ();
    }
    None
}

fn type_of_expr(pass: &Pass<'_>, expr: &Expr) -> Option<TypeId> {
    let info = pass.types_info()?;
    Some(info.types.get(&expr.id())?.typ)
}

fn types_are_identical(pass: &Pass<'_>, a: TypeId, b: TypeId) -> bool {
    let Some(artifacts) = pass.pkg().type_artifacts.as_ref() else {
        return false;
    };
    let mut types = artifacts.types.clone();
    types_identical(
        &mut types,
        &artifacts.objects,
        &artifacts.packages,
        a,
        b,
    )
}

fn is_exactly_error(pass: &Pass<'_>, ty: &Expr) -> bool {
    let Some(err) = universe_error_type(pass) else {
        return false;
    };
    let Some(typ) = type_of_expr(pass, ty) else {
        return false;
    };
    types_are_identical(pass, typ, err)
}

/// `go/types.writeSigExpr`: `(params)`, then the results — bare when there is
/// exactly one unnamed result, parenthesised otherwise.
fn sig_string(ft: &guff::ast::FuncType) -> String {
    let params = ft
        .params
        .as_ref()
        .map(|f| field_list_string(f))
        .unwrap_or_default();
    let mut out = format!("({params})");
    let Some(res) = ft.results.as_ref() else {
        return out;
    };
    let n: usize = res.list.iter().map(|f| f.names.len().max(1)).sum();
    if n == 0 {
        return out;
    }
    out.push(' ');
    if n == 1 && res.list.len() == 1 && res.list[0].names.is_empty() {
        if let Some(t) = res.list[0].ty.as_ref() {
            out.push_str(&expr_string(t));
        }
        return out;
    }
    out.push('(');
    out.push_str(&field_list_string(res));
    out.push(')');
    out
}

/// `go/types.writeFieldList` with `", "` and `iface = false`.
fn field_list_string(fields: &guff::ast::FieldList) -> String {
    let mut parts = Vec::new();
    for f in &fields.list {
        let names = f
            .names
            .iter()
            .map(|n| n.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let ty = f.ty.as_ref().map(expr_string).unwrap_or_default();
        parts.push(if names.is_empty() {
            ty
        } else {
            format!("{names} {ty}")
        });
    }
    parts.join(", ")
}

/// Approximate `go/types.ExprString` for common type expressions.
fn expr_string(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::SelectorExpr(sel) => format!("{}.{}", expr_string(&sel.x), sel.sel.name),
        Expr::StarExpr(s) => format!("*{}", expr_string(&s.x)),
        Expr::ParenExpr(p) => format!("({})", expr_string(&p.x)),
        Expr::ArrayType(a) => match &a.len {
            None => format!("[]{}", expr_string(&a.elt)),
            Some(len) => format!("[{}]{}", expr_string(len), expr_string(&a.elt)),
        },
        Expr::MapType(m) => format!("map[{}]{}", expr_string(&m.key), expr_string(&m.value)),
        // `ChanDir` is a bitset: a bidirectional channel carries both bits, so
        // the one-way cases are the ones missing a bit.
        Expr::ChanType(c) => {
            let send = c.dir.0 & ChanDir::SEND.0 != 0;
            let recv = c.dir.0 & ChanDir::RECV.0 != 0;
            let value = expr_string(&c.value);
            match (send, recv) {
                (false, true) => format!("<-chan {value}"),
                (true, false) => format!("chan<- {value}"),
                _ => format!("chan {value}"),
            }
        }
        Expr::InterfaceType(it) if it.methods.list.is_empty() => "interface{}".into(),
        Expr::StructType(st) if st.fields.list.is_empty() => "struct{}".into(),
        // `writeSigExpr`. This was `"func(...)"`, which is not a type anyone
        // wrote — nonamedreturns prints this string, so cobra's
        // `func(*Command) error` came out as `func(...)` and stood on both
        // sides of the diff at once.
        Expr::FuncType(ft) => format!("func{}", sig_string(ft)),
        Expr::Ellipsis(el) => match &el.elt {
            Some(t) => format!("...{}", expr_string(t)),
            None => "...".into(),
        },
        Expr::BasicLit(l) => l.value.clone(),
        _ => "<expr>".into(),
    }
}

fn mark_assigned(pass: &Pass<'_>, expr: &Expr, assigned: &mut HashSet<ObjectId>) {
    if let Expr::Ident(id) = expr {
        if let Some(obj) = object_of(pass, id) {
            assigned.insert(obj);
        }
    }
}

fn mark_idents_in_block(pass: &Pass<'_>, body: &BlockStmt, out: &mut HashSet<ObjectId>) {
    for stmt in &body.list {
        preorder(stmt_ref(stmt), |n| {
            if let NodeRef::Ident(id) = n {
                if let Some(obj) = object_of(pass, id) {
                    out.insert(obj);
                }
            }
            true
        });
    }
}

fn walk_expr_defer(
    pass: &Pass<'_>,
    expr: &Expr,
    in_closure: bool,
    defer_used: &mut HashSet<ObjectId>,
    assigned: &mut HashSet<ObjectId>,
    has_return_with_results: &mut bool,
) {
    match expr {
        Expr::FuncLit(fl) => {
            walk_block_defer(
                pass,
                &fl.body,
                true,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Expr::CallExpr(c) => walk_call_defer(
            pass,
            c,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Expr::ParenExpr(p) => walk_expr_defer(
            pass,
            &p.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Expr::SelectorExpr(s) => walk_expr_defer(
            pass,
            &s.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Expr::IndexExpr(i) => {
            walk_expr_defer(
                pass,
                &i.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_expr_defer(
                pass,
                &i.index,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Expr::SliceExpr(s) => {
            walk_expr_defer(
                pass,
                &s.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            if let Some(low) = &s.low {
                walk_expr_defer(
                    pass,
                    low,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            if let Some(high) = &s.high {
                walk_expr_defer(
                    pass,
                    high,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            if let Some(max) = &s.max {
                walk_expr_defer(
                    pass,
                    max,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Expr::StarExpr(s) => walk_expr_defer(
            pass,
            &s.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Expr::UnaryExpr(u) => walk_expr_defer(
            pass,
            &u.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Expr::BinaryExpr(b) => {
            walk_expr_defer(
                pass,
                &b.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_expr_defer(
                pass,
                &b.y,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Expr::KeyValueExpr(kv) => {
            walk_expr_defer(
                pass,
                &kv.key,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_expr_defer(
                pass,
                &kv.value,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Expr::CompositeLit(c) => {
            if let Some(ty) = &c.ty {
                walk_expr_defer(
                    pass,
                    ty,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            for elt in &c.elts {
                walk_expr_defer(
                    pass,
                    elt,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Expr::TypeAssertExpr(t) => {
            walk_expr_defer(
                pass,
                &t.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            if let Some(ty) = &t.ty {
                walk_expr_defer(
                    pass,
                    ty,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Expr::IndexListExpr(i) => {
            walk_expr_defer(
                pass,
                &i.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            for idx in &i.indices {
                walk_expr_defer(
                    pass,
                    idx,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        _ => {}
    }
}

fn walk_call_defer(
    pass: &Pass<'_>,
    call: &CallExpr,
    in_closure: bool,
    defer_used: &mut HashSet<ObjectId>,
    assigned: &mut HashSet<ObjectId>,
    has_return_with_results: &mut bool,
) {
    walk_expr_defer(
        pass,
        &call.fun,
        in_closure,
        defer_used,
        assigned,
        has_return_with_results,
    );
    for arg in &call.args {
        walk_expr_defer(
            pass,
            arg,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        );
    }
}

fn walk_block_defer(
    pass: &Pass<'_>,
    body: &BlockStmt,
    in_closure: bool,
    defer_used: &mut HashSet<ObjectId>,
    assigned: &mut HashSet<ObjectId>,
    has_return_with_results: &mut bool,
) {
    for stmt in &body.list {
        walk_stmt_defer(
            pass,
            stmt,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        );
    }
}

fn walk_stmt_defer(
    pass: &Pass<'_>,
    stmt: &Stmt,
    in_closure: bool,
    defer_used: &mut HashSet<ObjectId>,
    assigned: &mut HashSet<ObjectId>,
    has_return_with_results: &mut bool,
) {
    match stmt {
        Stmt::AssignStmt(a) => {
            for lh in &a.lhs {
                mark_assigned(pass, lh, assigned);
            }
            for rh in &a.rhs {
                walk_expr_defer(
                    pass,
                    rh,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Stmt::RangeStmt(r) => {
            if r.tok == Some(Token::ASSIGN) {
                if let Some(key) = &r.key {
                    mark_assigned(pass, key, assigned);
                }
                if let Some(value) = &r.value {
                    mark_assigned(pass, value, assigned);
                }
            }
            walk_expr_defer(
                pass,
                &r.x,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_block_defer(
                pass,
                &r.body,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::ReturnStmt(r) => {
            if !in_closure && !r.results.is_empty() {
                *has_return_with_results = true;
            }
            for e in &r.results {
                walk_expr_defer(
                    pass,
                    e,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Stmt::DeferStmt(d) => {
            if let Expr::FuncLit(fl) = d.call.fun.as_ref() {
                mark_idents_in_block(pass, &fl.body, defer_used);
            }
            walk_call_defer(
                pass,
                &d.call,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::GoStmt(g) => walk_call_defer(
            pass,
            &g.call,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::ExprStmt(e) => walk_expr_defer(
            pass,
            &e.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::SendStmt(s) => {
            walk_expr_defer(
                pass,
                &s.chan_,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_expr_defer(
                pass,
                &s.value,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::IncDecStmt(s) => walk_expr_defer(
            pass,
            &s.x,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::BlockStmt(b) => walk_block_defer(
            pass,
            b,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::LabeledStmt(l) => walk_stmt_defer(
            pass,
            &l.stmt,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::IfStmt(i) => {
            if let Some(init) = &i.init {
                walk_stmt_defer(
                    pass,
                    init,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            walk_expr_defer(
                pass,
                &i.cond,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_block_defer(
                pass,
                &i.body,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            if let Some(els) = &i.else_ {
                walk_stmt_defer(
                    pass,
                    els,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_defer(
                    pass,
                    init,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            if let Some(tag) = &s.tag {
                walk_expr_defer(
                    pass,
                    tag,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            walk_block_defer(
                pass,
                &s.body,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_defer(
                    pass,
                    init,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            walk_stmt_defer(
                pass,
                &s.assign,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
            walk_block_defer(
                pass,
                &s.body,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::SelectStmt(s) => walk_block_defer(
            pass,
            &s.body,
            in_closure,
            defer_used,
            assigned,
            has_return_with_results,
        ),
        Stmt::ForStmt(f) => {
            if let Some(init) = &f.init {
                walk_stmt_defer(
                    pass,
                    init,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            if let Some(cond) = &f.cond {
                walk_expr_defer(
                    pass,
                    cond,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            if let Some(post) = &f.post {
                walk_stmt_defer(
                    pass,
                    post,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            walk_block_defer(
                pass,
                &f.body,
                in_closure,
                defer_used,
                assigned,
                has_return_with_results,
            );
        }
        Stmt::CaseClause(c) => {
            for e in &c.list {
                walk_expr_defer(
                    pass,
                    e,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            for s in &c.body {
                walk_stmt_defer(
                    pass,
                    s,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Stmt::CommClause(c) => {
            if let Some(comm) = &c.comm {
                walk_stmt_defer(
                    pass,
                    comm,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
            for s in &c.body {
                walk_stmt_defer(
                    pass,
                    s,
                    in_closure,
                    defer_used,
                    assigned,
                    has_return_with_results,
                );
            }
        }
        Stmt::DeclStmt(d) => {
            if let Decl::GenDecl(g) = &d.decl {
                for spec in &g.specs {
                    match spec {
                        Spec::ValueSpec(vs) => {
                            if let Some(ty) = &vs.ty {
                                walk_expr_defer(
                                    pass,
                                    ty,
                                    in_closure,
                                    defer_used,
                                    assigned,
                                    has_return_with_results,
                                );
                            }
                            for v in &vs.values {
                                walk_expr_defer(
                                    pass,
                                    v,
                                    in_closure,
                                    defer_used,
                                    assigned,
                                    has_return_with_results,
                                );
                            }
                        }
                        Spec::TypeSpec(ts) => {
                            walk_expr_defer(
                                pass,
                                &ts.ty,
                                in_closure,
                                defer_used,
                                assigned,
                                has_return_with_results,
                            );
                        }
                        Spec::ImportSpec(_) => {}
                    }
                }
            }
        }
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}
    }
}

fn collect_defer_usage_and_assignments(
    pass: &Pass<'_>,
    body: &BlockStmt,
) -> (HashSet<ObjectId>, HashSet<ObjectId>, bool) {
    let mut defer_used = HashSet::new();
    let mut assigned = HashSet::new();
    let mut has_return_with_results = false;
    walk_block_defer(
        pass,
        body,
        false,
        &mut defer_used,
        &mut assigned,
        &mut has_return_with_results,
    );
    (defer_used, assigned, has_return_with_results)
}

fn walk_call_usage(
    pass: &Pass<'_>,
    call: &CallExpr,
    in_closure: bool,
    referenced: &mut HashSet<ObjectId>,
    has_naked_return: &mut bool,
) {
    walk_expr_usage(pass, &call.fun, in_closure, referenced, has_naked_return);
    for arg in &call.args {
        walk_expr_usage(pass, arg, in_closure, referenced, has_naked_return);
    }
}

fn walk_expr_usage(
    pass: &Pass<'_>,
    expr: &Expr,
    in_closure: bool,
    referenced: &mut HashSet<ObjectId>,
    has_naked_return: &mut bool,
) {
    match expr {
        Expr::Ident(id) => {
            if let Some(obj) = object_of(pass, id) {
                referenced.insert(obj);
            }
        }
        Expr::FuncLit(fl) => {
            walk_block_usage(pass, &fl.body, true, referenced, has_naked_return);
        }
        Expr::CallExpr(c) => {
            walk_call_usage(pass, c, in_closure, referenced, has_naked_return);
        }
        Expr::ParenExpr(p) => {
            walk_expr_usage(pass, &p.x, in_closure, referenced, has_naked_return)
        }
        Expr::SelectorExpr(s) => {
            walk_expr_usage(pass, &s.x, in_closure, referenced, has_naked_return)
        }
        Expr::IndexExpr(i) => {
            walk_expr_usage(pass, &i.x, in_closure, referenced, has_naked_return);
            walk_expr_usage(pass, &i.index, in_closure, referenced, has_naked_return);
        }
        Expr::SliceExpr(s) => {
            walk_expr_usage(pass, &s.x, in_closure, referenced, has_naked_return);
            if let Some(low) = &s.low {
                walk_expr_usage(pass, low, in_closure, referenced, has_naked_return);
            }
            if let Some(high) = &s.high {
                walk_expr_usage(pass, high, in_closure, referenced, has_naked_return);
            }
            if let Some(max) = &s.max {
                walk_expr_usage(pass, max, in_closure, referenced, has_naked_return);
            }
        }
        Expr::StarExpr(s) => {
            walk_expr_usage(pass, &s.x, in_closure, referenced, has_naked_return)
        }
        Expr::UnaryExpr(u) => {
            walk_expr_usage(pass, &u.x, in_closure, referenced, has_naked_return)
        }
        Expr::BinaryExpr(b) => {
            walk_expr_usage(pass, &b.x, in_closure, referenced, has_naked_return);
            walk_expr_usage(pass, &b.y, in_closure, referenced, has_naked_return);
        }
        Expr::KeyValueExpr(kv) => {
            walk_expr_usage(pass, &kv.key, in_closure, referenced, has_naked_return);
            walk_expr_usage(pass, &kv.value, in_closure, referenced, has_naked_return);
        }
        Expr::CompositeLit(c) => {
            if let Some(ty) = &c.ty {
                walk_expr_usage(pass, ty, in_closure, referenced, has_naked_return);
            }
            for elt in &c.elts {
                walk_expr_usage(pass, elt, in_closure, referenced, has_naked_return);
            }
        }
        Expr::TypeAssertExpr(t) => {
            walk_expr_usage(pass, &t.x, in_closure, referenced, has_naked_return);
            if let Some(ty) = &t.ty {
                walk_expr_usage(pass, ty, in_closure, referenced, has_naked_return);
            }
        }
        Expr::IndexListExpr(i) => {
            walk_expr_usage(pass, &i.x, in_closure, referenced, has_naked_return);
            for idx in &i.indices {
                walk_expr_usage(pass, idx, in_closure, referenced, has_naked_return);
            }
        }
        _ => {}
    }
}

fn walk_block_usage(
    pass: &Pass<'_>,
    body: &BlockStmt,
    in_closure: bool,
    referenced: &mut HashSet<ObjectId>,
    has_naked_return: &mut bool,
) {
    for stmt in &body.list {
        walk_stmt_usage(pass, stmt, in_closure, referenced, has_naked_return);
    }
}

fn walk_stmt_usage(
    pass: &Pass<'_>,
    stmt: &Stmt,
    in_closure: bool,
    referenced: &mut HashSet<ObjectId>,
    has_naked_return: &mut bool,
) {
    match stmt {
        Stmt::ReturnStmt(r) => {
            if !in_closure && r.results.is_empty() {
                *has_naked_return = true;
            }
            for e in &r.results {
                walk_expr_usage(pass, e, in_closure, referenced, has_naked_return);
            }
        }
        Stmt::AssignStmt(a) => {
            for lh in &a.lhs {
                walk_expr_usage(pass, lh, in_closure, referenced, has_naked_return);
            }
            for rh in &a.rhs {
                walk_expr_usage(pass, rh, in_closure, referenced, has_naked_return);
            }
        }
        Stmt::RangeStmt(r) => {
            if let Some(key) = &r.key {
                walk_expr_usage(pass, key, in_closure, referenced, has_naked_return);
            }
            if let Some(value) = &r.value {
                walk_expr_usage(pass, value, in_closure, referenced, has_naked_return);
            }
            walk_expr_usage(pass, &r.x, in_closure, referenced, has_naked_return);
            walk_block_usage(pass, &r.body, in_closure, referenced, has_naked_return);
        }
        Stmt::DeferStmt(d) => {
            walk_call_usage(pass, &d.call, in_closure, referenced, has_naked_return);
        }
        Stmt::GoStmt(g) => {
            walk_call_usage(pass, &g.call, in_closure, referenced, has_naked_return);
        }
        Stmt::ExprStmt(e) => {
            walk_expr_usage(pass, &e.x, in_closure, referenced, has_naked_return)
        }
        Stmt::SendStmt(s) => {
            walk_expr_usage(pass, &s.chan_, in_closure, referenced, has_naked_return);
            walk_expr_usage(pass, &s.value, in_closure, referenced, has_naked_return);
        }
        Stmt::IncDecStmt(s) => {
            walk_expr_usage(pass, &s.x, in_closure, referenced, has_naked_return)
        }
        Stmt::BlockStmt(b) => {
            walk_block_usage(pass, b, in_closure, referenced, has_naked_return)
        }
        Stmt::LabeledStmt(l) => {
            walk_stmt_usage(pass, &l.stmt, in_closure, referenced, has_naked_return)
        }
        Stmt::IfStmt(i) => {
            if let Some(init) = &i.init {
                walk_stmt_usage(pass, init, in_closure, referenced, has_naked_return);
            }
            walk_expr_usage(pass, &i.cond, in_closure, referenced, has_naked_return);
            walk_block_usage(pass, &i.body, in_closure, referenced, has_naked_return);
            if let Some(els) = &i.else_ {
                walk_stmt_usage(pass, els, in_closure, referenced, has_naked_return);
            }
        }
        Stmt::SwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_usage(pass, init, in_closure, referenced, has_naked_return);
            }
            if let Some(tag) = &s.tag {
                walk_expr_usage(pass, tag, in_closure, referenced, has_naked_return);
            }
            walk_block_usage(pass, &s.body, in_closure, referenced, has_naked_return);
        }
        Stmt::TypeSwitchStmt(s) => {
            if let Some(init) = &s.init {
                walk_stmt_usage(pass, init, in_closure, referenced, has_naked_return);
            }
            walk_stmt_usage(pass, &s.assign, in_closure, referenced, has_naked_return);
            walk_block_usage(pass, &s.body, in_closure, referenced, has_naked_return);
        }
        Stmt::SelectStmt(s) => {
            walk_block_usage(pass, &s.body, in_closure, referenced, has_naked_return)
        }
        Stmt::ForStmt(f) => {
            if let Some(init) = &f.init {
                walk_stmt_usage(pass, init, in_closure, referenced, has_naked_return);
            }
            if let Some(cond) = &f.cond {
                walk_expr_usage(pass, cond, in_closure, referenced, has_naked_return);
            }
            if let Some(post) = &f.post {
                walk_stmt_usage(pass, post, in_closure, referenced, has_naked_return);
            }
            walk_block_usage(pass, &f.body, in_closure, referenced, has_naked_return);
        }
        Stmt::CaseClause(c) => {
            for e in &c.list {
                walk_expr_usage(pass, e, in_closure, referenced, has_naked_return);
            }
            for s in &c.body {
                walk_stmt_usage(pass, s, in_closure, referenced, has_naked_return);
            }
        }
        Stmt::CommClause(c) => {
            if let Some(comm) = &c.comm {
                walk_stmt_usage(pass, comm, in_closure, referenced, has_naked_return);
            }
            for s in &c.body {
                walk_stmt_usage(pass, s, in_closure, referenced, has_naked_return);
            }
        }
        Stmt::DeclStmt(d) => {
            preorder(decl_ref(&d.decl), |n| {
                if let NodeRef::Ident(id) = n {
                    if let Some(obj) = object_of(pass, id) {
                        referenced.insert(obj);
                    }
                }
                true
            });
        }
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}
    }
}

fn collect_named_return_usage(
    pass: &Pass<'_>,
    body: &BlockStmt,
) -> (HashSet<ObjectId>, bool) {
    let mut referenced = HashSet::new();
    let mut has_naked_return = false;
    walk_block_usage(pass, body, false, &mut referenced, &mut has_naked_return);
    (referenced, has_naked_return)
}

fn check_results(
    pass: &Pass<'_>,
    results: &FieldList,
    body: &BlockStmt,
    // Upstream reports the **function**, not the named return: `func_pos` is
    // the `func` keyword, which is `(*ast.FuncDecl).Pos()` and
    // `(*ast.FuncLit).Pos()` alike. Reporting the identifier put every finding
    // a dozen columns to the right — invisible to the isolate and OSS keys,
    // which carry no column.
    func_pos: u32,
    opts: &NonamedreturnsOptions,
    pending: &mut Vec<(u32, String)>,
) {
    if opts.allow_unused_named_returns {
        let mut usage: Option<(HashSet<ObjectId>, bool)> = None;
        for field in &results.list {
            if field.names.is_empty() {
                continue;
            }
            let ty_str = field
                .ty
                .as_ref()
                .map(expr_string)
                .unwrap_or_else(|| "<expr>".into());
            for name in &field.names {
                if name.name == "_" {
                    continue;
                }
                if usage.is_none() {
                    usage = Some(collect_named_return_usage(pass, body));
                }
                let (referenced, has_naked) = usage.as_ref().unwrap();
                let Some(obj) = object_of(pass, name) else {
                    continue;
                };
                if referenced.contains(&obj) || *has_naked {
                    pending.push((
                        func_pos,
                        format!(
                            "named return \"{}\" with type \"{}\" must not be referenced or used by a naked return",
                            name.name, ty_str
                        ),
                    ));
                }
            }
        }
        return;
    }

    let mut defer_info: Option<(HashSet<ObjectId>, HashSet<ObjectId>, bool)> = None;

    for field in &results.list {
        if field.names.is_empty() {
            continue;
        }
        let Some(ty) = field.ty.as_ref() else {
            continue;
        };
        let ty_str = expr_string(ty);
        let is_error = is_exactly_error(pass, ty);

        for name in &field.names {
            if name.name == "_" {
                continue;
            }

            if !opts.report_error_in_defer && is_error {
                if defer_info.is_none() {
                    defer_info = Some(collect_defer_usage_and_assignments(pass, body));
                }
                let (defer_used, assigned, has_return_with_results) =
                    defer_info.as_ref().unwrap();
                if let Some(obj) = object_of(pass, name) {
                    if defer_used.contains(&obj)
                        && (assigned.contains(&obj) || *has_return_with_results)
                    {
                        continue;
                    }
                }
            }

            pending.push((
                func_pos,
                format!(
                    "named return \"{}\" with type \"{}\" found",
                    name.name, ty_str
                ),
            ));
        }
    }
}

fn check_func(
    pass: &Pass<'_>,
    results: Option<&FieldList>,
    body: Option<&BlockStmt>,
    func_pos: u32,
    opts: &NonamedreturnsOptions,
    pending: &mut Vec<(u32, String)>,
) {
    let Some(body) = body else {
        return;
    };
    let Some(results) = results else {
        return;
    };
    check_results(pass, results, body, func_pos, opts, pending);
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "nonamedreturns requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<NonamedreturnsOptions>("nonamedreturns")
        .copied()
        .unwrap_or_default();

    let mut pending: Vec<(u32, String)> = Vec::new();
    for file in pass.files() {
        preorder(NodeRef::File(file), |n| match n {
            NodeRef::FuncDecl(fd) => {
                check_func(
                    pass,
                    fd.ty.results.as_ref(),
                    fd.body.as_ref(),
                    fd.ty.func.0 as u32,
                    &opts,
                    &mut pending,
                );
                true
            }
            NodeRef::FuncLit(fl) => {
                check_func(
                    pass,
                    fl.ty.results.as_ref(),
                    Some(&fl.body),
                    fl.ty.func.0 as u32,
                    &opts,
                    &mut pending,
                );
                true
            }
            _ => true,
        });
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "nonamedreturns",
        doc: "Reports all named returns.",
        url: "https://github.com/firefart/nonamedreturns",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn analyzer_graph_is_valid() {
        validate(&[analyzer()]).expect("valid analyzer graph");
    }
}
