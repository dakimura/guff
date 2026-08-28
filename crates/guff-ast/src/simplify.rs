// Port of Go's cmd/gofmt/simplify.go — the `-s` flag.
//
// Original: Copyright 2010 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
//! `gofmt -s`, and therefore golangci-lint's `formatters.settings.gofmt`
//! with its default `simplify: true`.
//!
//! Three rewrites, all of them removals:
//!
//! - a composite literal element whose own type repeats the outer element type
//!   drops the repeat (`[][]int{[]int{1}}` → `[][]int{{1}}`), and `&T{…}` under
//!   an element type of `*T` loses both the `&` and the `T`;
//! - `s[a:len(s)]` becomes `s[a:]` when `s` is a plain identifier;
//! - `for x, _ = range v` becomes `for x = range v`, and `for _ = range v`
//!   becomes `for range v`.
//!
//! Plus `removeEmptyDeclGroups`, which drops `const ()` and friends.
//!
//! # The walk is its own
//!
//! [`crate::walk`] is read-only, and [`crate::stamp`] has a mutable traversal
//! that cannot be reused: the simplifier needs `ast.Walk`'s
//! *return-nil-to-stop* behaviour, which stamping has no notion of. A
//! simplified composite literal must not have its children walked again —
//! `simplify_literal` has already done that — and folding the two passes
//! together would either lose that or bolt a foreign concept onto stamping.
//! So this mirrors `go/ast/walk.go` directly, case for case.

use crate::ast::*;
use crate::token::Token;

/// `gofmt -s`: simplify `f` in place.
pub fn simplify(f: &mut File) {
    // Remove empty declarations such as "const ()", etc.
    remove_empty_decl_groups(f);
    for d in &mut f.decls {
        walk_decl(d);
    }
}

// ============================================================
// The simplifier's three rewrites
// ============================================================

/// `simplifier.Visit` for expressions. Returns false when the walk must stop
/// here (Go's `return nil`), which happens for a composite literal whose
/// elements `simplify_literal` has already walked.
fn visit_expr(e: &mut Expr) -> bool {
    match e {
        Expr::CompositeLit(_) => visit_composite_lit(e),
        Expr::SliceExpr(n) => {
            // A slice expression of the form `s[a:len(s)]` can be simplified
            // to `s[a:]` if s is "simple enough" (for now we only accept
            // identifiers).
            //
            // Note: This may not be correct because len may have been
            // redeclared in the same package. However, this is extremely
            // unlikely and so far (April 2022, after years of supporting this
            // rewrite feature) has never come up, so let's keep it working as
            // is (see also #15153).
            if n.max.is_some() {
                // 3-index slices always require the 2nd and 3rd index.
                return true;
            }
            let Expr::Ident(s) = n.x.as_ref() else {
                return true;
            };
            let s_name = s.name.clone();
            let drop_high = match n.high.as_deref() {
                Some(Expr::CallExpr(call)) if call.args.len() == 1 && !call.ellipsis.is_valid() => {
                    matches!(call.fun.as_ref(), Expr::Ident(fun) if fun.name == "len")
                        && matches!(&call.args[0], Expr::Ident(arg) if arg.name == s_name)
                }
                _ => false,
            };
            if drop_high {
                n.high = None;
            }
            // Note: We could also simplify slice expressions of the form
            // s[0:b] to s[:b] but we leave them as is since sometimes we want
            // to be very explicit about the lower bound.
            true
        }
        _ => true,
    }
}

/// The `*ast.CompositeLit` arm of `simplifier.Visit`, split out because it
/// needs the whole `Expr` to hand each element slot to `simplify_literal`.
fn visit_composite_lit(e: &mut Expr) -> bool {
    // Array, slice, and map composite literals may be simplified. Upstream
    // reads the key/element type off `outer.Type` and then mutates
    // `outer.Elts` through it; the two borrows cannot overlap here, and the
    // types are only ever *read*, so they are cloned once per literal.
    let (key_type, elt_type) = {
        let Expr::CompositeLit(outer) = e else {
            return true;
        };
        match outer.ty.as_deref() {
            Some(Expr::ArrayType(t)) => (None, Some((*t.elt).clone())),
            Some(Expr::MapType(t)) => (Some((*t.key).clone()), Some((*t.value).clone())),
            _ => (None, None),
        }
    };

    let Some(elt_type) = elt_type else {
        return true;
    };

    let Expr::CompositeLit(outer) = e else {
        return true;
    };
    for x in &mut outer.elts {
        // Look at value of indexed/named elements.
        if let Expr::KeyValueExpr(kv) = x {
            if let Some(kt) = &key_type {
                simplify_literal(kt, kt, &mut kv.key);
            }
            simplify_literal(&elt_type, &elt_type, &mut kv.value);
            continue;
        }
        simplify_literal(&elt_type, &elt_type, x);
    }
    // Node was simplified — stop walk (there are no subnodes to simplify).
    false
}

/// `simplifier.simplifyLiteral`.
///
/// Upstream takes the element both by value (`x`) and by slot (`px`); a single
/// `&mut Expr` is both.
fn simplify_literal(typ: &Expr, ast_type: &Expr, px: &mut Expr) {
    walk_expr(px); // simplify x

    // If the element is a composite literal and its literal type matches the
    // outer literal's element type exactly, the inner literal type may be
    // omitted.
    if let Expr::CompositeLit(inner) = px {
        if match_expr_opt(Some(typ), inner.ty.as_deref()) {
            inner.ty = None;
        }
    }
    // If the outer literal's element type is a pointer type *T and the element
    // is & of a composite literal of type T, the inner &T may be omitted.
    if let Expr::StarExpr(ptr) = ast_type {
        let replacement = match px {
            Expr::UnaryExpr(addr) if addr.op == Token::AND => match addr.x.as_mut() {
                Expr::CompositeLit(inner) if match_expr_opt(Some(&ptr.x), inner.ty.as_deref()) => {
                    inner.ty = None; // drop T
                    Some(Expr::CompositeLit(inner.clone()))
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(inner) = replacement {
            *px = inner; // drop &
        }
    }
}

/// `isBlank`.
fn is_blank(x: Option<&Expr>) -> bool {
    matches!(x, Some(Expr::Ident(id)) if id.name == "_")
}

/// `removeEmptyDeclGroups`.
fn remove_empty_decl_groups(f: &mut File) {
    let comments = std::mem::take(&mut f.comments);
    f.decls.retain(|d| !is_empty_decl(&comments, d));
    f.comments = comments;
}

/// `isEmpty`: a `GenDecl` with no doc, no specs, and no comment inside it.
///
/// `pos`/`end` live on `Decl` rather than `GenDecl` here, so the whole decl is
/// passed and the kind checked inside.
fn is_empty_decl(comments: &[CommentGroup], d: &Decl) -> bool {
    let Decl::GenDecl(g) = d else {
        return false;
    };
    if g.doc.is_some() || !g.specs.is_empty() {
        return false;
    }
    // If there is a comment in the declaration, it is not considered empty.
    !comments
        .iter()
        .any(|c| d.pos() <= c.pos() && c.end() <= d.end())
}

// ============================================================
// The walk (mirror of go/ast/walk.go)
// ============================================================

fn walk_expr(e: &mut Expr) {
    if !visit_expr(e) {
        return;
    }
    match e {
        Expr::BadExpr(_) | Expr::Ident(_) | Expr::BasicLit(_) => {}
        Expr::Ellipsis(n) => {
            if let Some(elt) = n.elt.as_mut() {
                walk_expr(elt);
            }
        }
        Expr::FuncLit(n) => {
            walk_func_type(&mut n.ty);
            walk_block(&mut n.body);
        }
        Expr::CompositeLit(n) => {
            if let Some(t) = n.ty.as_mut() {
                walk_expr(t);
            }
            walk_exprs(&mut n.elts);
        }
        Expr::ParenExpr(n) => walk_expr(&mut n.x),
        Expr::SelectorExpr(n) => walk_expr(&mut n.x),
        Expr::IndexExpr(n) => {
            walk_expr(&mut n.x);
            walk_expr(&mut n.index);
        }
        Expr::IndexListExpr(n) => {
            walk_expr(&mut n.x);
            walk_exprs(&mut n.indices);
        }
        Expr::SliceExpr(n) => {
            walk_expr(&mut n.x);
            if let Some(low) = n.low.as_mut() {
                walk_expr(low);
            }
            if let Some(high) = n.high.as_mut() {
                walk_expr(high);
            }
            if let Some(max) = n.max.as_mut() {
                walk_expr(max);
            }
        }
        Expr::TypeAssertExpr(n) => {
            walk_expr(&mut n.x);
            if let Some(t) = n.ty.as_mut() {
                walk_expr(t);
            }
        }
        Expr::CallExpr(n) => walk_call(n),
        Expr::StarExpr(n) => walk_expr(&mut n.x),
        Expr::UnaryExpr(n) => walk_expr(&mut n.x),
        Expr::BinaryExpr(n) => {
            walk_expr(&mut n.x);
            walk_expr(&mut n.y);
        }
        Expr::KeyValueExpr(n) => {
            walk_expr(&mut n.key);
            walk_expr(&mut n.value);
        }
        Expr::ArrayType(n) => {
            if let Some(len) = n.len.as_mut() {
                walk_expr(len);
            }
            walk_expr(&mut n.elt);
        }
        Expr::StructType(n) => walk_field_list(&mut n.fields),
        Expr::FuncType(n) => walk_func_type(n),
        Expr::InterfaceType(n) => walk_field_list(&mut n.methods),
        Expr::MapType(n) => {
            walk_expr(&mut n.key);
            walk_expr(&mut n.value);
        }
        Expr::ChanType(n) => walk_expr(&mut n.value),
    }
}

fn walk_exprs(list: &mut [Expr]) {
    for e in list {
        walk_expr(e);
    }
}

fn walk_call(n: &mut CallExpr) {
    walk_expr(&mut n.fun);
    walk_exprs(&mut n.args);
}

fn walk_func_type(n: &mut FuncType) {
    if let Some(tp) = n.type_params.as_mut() {
        walk_field_list(tp);
    }
    if let Some(p) = n.params.as_mut() {
        walk_field_list(p);
    }
    if let Some(r) = n.results.as_mut() {
        walk_field_list(r);
    }
}

fn walk_field_list(fl: &mut FieldList) {
    for f in &mut fl.list {
        if let Some(t) = f.ty.as_mut() {
            walk_expr(t);
        }
    }
}

fn walk_block(b: &mut BlockStmt) {
    for s in &mut b.list {
        walk_stmt(s);
    }
}

fn walk_stmt(s: &mut Stmt) {
    // `simplifier.Visit` acts on exactly one statement kind.
    if let Stmt::RangeStmt(n) = s {
        // - a range of the form: for x, _ = range v {...}
        // can be simplified to: for x = range v {...}
        // - a range of the form: for _ = range v {...}
        // can be simplified to: for range v {...}
        if is_blank(n.value.as_ref()) {
            n.value = None;
        }
        if is_blank(n.key.as_ref()) && n.value.is_none() {
            n.key = None;
        }
    }

    match s {
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}
        Stmt::DeclStmt(n) => walk_decl(&mut n.decl),
        Stmt::LabeledStmt(n) => walk_stmt(&mut n.stmt),
        Stmt::ExprStmt(n) => walk_expr(&mut n.x),
        Stmt::SendStmt(n) => {
            walk_expr(&mut n.chan_);
            walk_expr(&mut n.value);
        }
        Stmt::IncDecStmt(n) => walk_expr(&mut n.x),
        Stmt::AssignStmt(n) => {
            walk_exprs(&mut n.lhs);
            walk_exprs(&mut n.rhs);
        }
        Stmt::GoStmt(n) => walk_call(&mut n.call),
        Stmt::DeferStmt(n) => walk_call(&mut n.call),
        Stmt::ReturnStmt(n) => walk_exprs(&mut n.results),
        Stmt::BlockStmt(n) => walk_block(n),
        Stmt::IfStmt(n) => {
            if let Some(init) = n.init.as_mut() {
                walk_stmt(init);
            }
            walk_expr(&mut n.cond);
            walk_block(&mut n.body);
            if let Some(e) = n.else_.as_mut() {
                walk_stmt(e);
            }
        }
        Stmt::CaseClause(n) => {
            walk_exprs(&mut n.list);
            for st in &mut n.body {
                walk_stmt(st);
            }
        }
        Stmt::SwitchStmt(n) => {
            if let Some(init) = n.init.as_mut() {
                walk_stmt(init);
            }
            if let Some(tag) = n.tag.as_mut() {
                walk_expr(tag);
            }
            walk_block(&mut n.body);
        }
        Stmt::TypeSwitchStmt(n) => {
            if let Some(init) = n.init.as_mut() {
                walk_stmt(init);
            }
            walk_stmt(&mut n.assign);
            walk_block(&mut n.body);
        }
        Stmt::CommClause(n) => {
            if let Some(comm) = n.comm.as_mut() {
                walk_stmt(comm);
            }
            for st in &mut n.body {
                walk_stmt(st);
            }
        }
        Stmt::SelectStmt(n) => walk_block(&mut n.body),
        Stmt::ForStmt(n) => {
            if let Some(init) = n.init.as_mut() {
                walk_stmt(init);
            }
            if let Some(cond) = n.cond.as_mut() {
                walk_expr(cond);
            }
            if let Some(post) = n.post.as_mut() {
                walk_stmt(post);
            }
            walk_block(&mut n.body);
        }
        Stmt::RangeStmt(n) => {
            if let Some(k) = n.key.as_mut() {
                walk_expr(k);
            }
            if let Some(v) = n.value.as_mut() {
                walk_expr(v);
            }
            walk_expr(&mut n.x);
            walk_block(&mut n.body);
        }
    }
}

fn walk_decl(d: &mut Decl) {
    match d {
        Decl::BadDecl(_) => {}
        Decl::GenDecl(n) => {
            for s in &mut n.specs {
                walk_spec(s);
            }
        }
        Decl::FuncDecl(n) => {
            if let Some(recv) = n.recv.as_mut() {
                walk_field_list(recv);
            }
            walk_func_type(&mut n.ty);
            if let Some(body) = n.body.as_mut() {
                walk_block(body);
            }
        }
    }
}

fn walk_spec(s: &mut Spec) {
    match s {
        Spec::ImportSpec(_) => {}
        Spec::ValueSpec(n) => {
            if let Some(t) = n.ty.as_mut() {
                walk_expr(t);
            }
            walk_exprs(&mut n.values);
        }
        Spec::TypeSpec(n) => {
            if let Some(tp) = n.type_params.as_mut() {
                walk_field_list(tp);
            }
            walk_expr(&mut n.ty);
        }
    }
}

// ============================================================
// `match(nil, pattern, val)` from cmd/gofmt/rewrite.go
// ============================================================
//
// With a nil binding map, upstream's reflection-based `match` collapses to
// deep structural equality over the AST with three carve-outs: identifiers
// compare by name alone, `token.Pos` and `*ast.Object` always match, and
// `CallExpr` additionally requires the two `Ellipsis` positions to agree in
// *validity* (that is how `f(x)` and `f(x...)` differ).
//
// Deliberately not routed through any existing expression comparison in this
// workspace: `parser`'s `expr_eq_shallow` answers a different question (should
// these parameters share a type group) and the analysis crates' helpers are
// ports of other upstream functions. Collapsing them would replace one
// faithful port with an approximation of another.

/// `match(nil, pattern, val)` where either side may be absent.
fn match_expr_opt(pattern: Option<&Expr>, val: Option<&Expr>) -> bool {
    match (pattern, val) {
        (None, None) => true,
        (Some(p), Some(v)) => match_expr(p, v),
        _ => false,
    }
}

fn match_exprs(a: &[Expr], b: &[Expr]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| match_expr(x, y))
}

fn match_expr(p: &Expr, v: &Expr) -> bool {
    match (p, v) {
        // For identifiers, only the names need to match (and none of the other
        // *ast.Object information).
        (Expr::Ident(a), Expr::Ident(b)) => a.name == b.name,
        (Expr::BadExpr(_), Expr::BadExpr(_)) => true,
        (Expr::BasicLit(a), Expr::BasicLit(b)) => a.kind == b.kind && a.value == b.value,
        (Expr::Ellipsis(a), Expr::Ellipsis(b)) => {
            match_expr_opt(a.elt.as_deref(), b.elt.as_deref())
        }
        // No `FuncLit` arm: `match` is only ever called on *type* expressions
        // (a composite literal's element type against an inner literal's own
        // type), and a function literal is not a type. Rather than approximate
        // upstream's reflection over statement bodies for an input that cannot
        // occur, the pair falls through to `false` below — the conservative
        // direction, since a false answer only declines to elide.
        (Expr::CompositeLit(a), Expr::CompositeLit(b)) => {
            match_expr_opt(a.ty.as_deref(), b.ty.as_deref())
                && match_exprs(&a.elts, &b.elts)
                && a.incomplete == b.incomplete
        }
        (Expr::ParenExpr(a), Expr::ParenExpr(b)) => match_expr(&a.x, &b.x),
        (Expr::SelectorExpr(a), Expr::SelectorExpr(b)) => {
            match_expr(&a.x, &b.x) && a.sel.name == b.sel.name
        }
        (Expr::IndexExpr(a), Expr::IndexExpr(b)) => {
            match_expr(&a.x, &b.x) && match_expr(&a.index, &b.index)
        }
        (Expr::IndexListExpr(a), Expr::IndexListExpr(b)) => {
            match_expr(&a.x, &b.x) && match_exprs(&a.indices, &b.indices)
        }
        (Expr::SliceExpr(a), Expr::SliceExpr(b)) => {
            match_expr(&a.x, &b.x)
                && match_expr_opt(a.low.as_deref(), b.low.as_deref())
                && match_expr_opt(a.high.as_deref(), b.high.as_deref())
                && match_expr_opt(a.max.as_deref(), b.max.as_deref())
                && a.slice3 == b.slice3
        }
        (Expr::TypeAssertExpr(a), Expr::TypeAssertExpr(b)) => {
            match_expr(&a.x, &b.x) && match_expr_opt(a.ty.as_deref(), b.ty.as_deref())
        }
        (Expr::CallExpr(a), Expr::CallExpr(b)) => {
            // For calls, the Ellipsis fields must match since that is how f(x)
            // and f(x...) are different.
            a.ellipsis.is_valid() == b.ellipsis.is_valid()
                && match_expr(&a.fun, &b.fun)
                && match_exprs(&a.args, &b.args)
        }
        (Expr::StarExpr(a), Expr::StarExpr(b)) => match_expr(&a.x, &b.x),
        (Expr::UnaryExpr(a), Expr::UnaryExpr(b)) => a.op == b.op && match_expr(&a.x, &b.x),
        (Expr::BinaryExpr(a), Expr::BinaryExpr(b)) => {
            a.op == b.op && match_expr(&a.x, &b.x) && match_expr(&a.y, &b.y)
        }
        (Expr::KeyValueExpr(a), Expr::KeyValueExpr(b)) => {
            match_expr(&a.key, &b.key) && match_expr(&a.value, &b.value)
        }
        (Expr::ArrayType(a), Expr::ArrayType(b)) => {
            match_expr_opt(a.len.as_deref(), b.len.as_deref()) && match_expr(&a.elt, &b.elt)
        }
        (Expr::StructType(a), Expr::StructType(b)) => {
            a.incomplete == b.incomplete && match_field_list(&a.fields, &b.fields)
        }
        (Expr::FuncType(a), Expr::FuncType(b)) => match_func_type(a, b),
        (Expr::InterfaceType(a), Expr::InterfaceType(b)) => {
            a.incomplete == b.incomplete && match_field_list(&a.methods, &b.methods)
        }
        (Expr::MapType(a), Expr::MapType(b)) => {
            match_expr(&a.key, &b.key) && match_expr(&a.value, &b.value)
        }
        (Expr::ChanType(a), Expr::ChanType(b)) => a.dir == b.dir && match_expr(&a.value, &b.value),
        // Different types never match.
        _ => false,
    }
}

fn match_func_type(a: &FuncType, b: &FuncType) -> bool {
    match_field_list_opt(a.type_params.as_ref(), b.type_params.as_ref())
        && match_field_list_opt(a.params.as_ref(), b.params.as_ref())
        && match_field_list_opt(a.results.as_ref(), b.results.as_ref())
}

fn match_field_list_opt(a: Option<&FieldList>, b: Option<&FieldList>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => match_field_list(x, y),
        _ => false,
    }
}

fn match_field_list(a: &FieldList, b: &FieldList) -> bool {
    a.list.len() == b.list.len()
        && a.list.iter().zip(&b.list).all(|(x, y)| {
            x.names.len() == y.names.len()
                && x.names
                    .iter()
                    .zip(&y.names)
                    .all(|(n, m)| n.name == m.name)
                && match_expr_opt(x.ty.as_ref(), y.ty.as_ref())
                && match_basic_lit_opt(x.tag.as_ref(), y.tag.as_ref())
                && match_comment_group_opt(x.doc.as_ref(), y.doc.as_ref())
                && match_comment_group_opt(x.comment.as_ref(), y.comment.as_ref())
        })
}

fn match_basic_lit_opt(a: Option<&BasicLit>, b: Option<&BasicLit>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => x.kind == y.kind && x.value == y.value,
        _ => false,
    }
}

/// Upstream reaches these through plain reflection — a `*ast.CommentGroup` is
/// not one of the always-match types — so the text is compared, and only the
/// `Slash` positions inside are ignored.
fn match_comment_group_opt(a: Option<&CommentGroup>, b: Option<&CommentGroup>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(x), Some(y)) => {
            x.list.len() == y.list.len()
                && x.list.iter().zip(&y.list).all(|(c, d)| c.text == d.text)
        }
        _ => false,
    }
}
