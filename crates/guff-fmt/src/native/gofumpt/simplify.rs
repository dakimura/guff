//! Port of gofumpt `format/simplify.go` + `format/rewrite.go` match helpers.
//!
//! gofumpt always simplifies (unlike `gofmt` without `-s`).

use guff::ast::{
    CompositeLit, Decl, Expr, File, GenDecl, RangeStmt, SliceExpr, Stmt,
};
use guff::token::Token;
use guff::Pos;

/// Apply gofmt-style simplify rewrites to `file` in place.
pub(crate) fn simplify(file: &mut File) {
    remove_empty_decl_groups(file);
    walk_file(file);
}

fn remove_empty_decl_groups(file: &mut File) {
    let mut i = 0;
    while i < file.decls.len() {
        let empty = match &file.decls[i] {
            Decl::GenDecl(g) => is_empty(file, g),
            _ => false,
        };
        if empty {
            file.decls.remove(i);
        } else {
            i += 1;
        }
    }
}

fn is_empty(file: &File, g: &GenDecl) -> bool {
    if g.doc.is_some() || !g.specs.is_empty() {
        return false;
    }
    let g_pos = g.tok_pos;
    let g_end = Decl::GenDecl(g.clone()).end();
    for c in &file.comments {
        if g_pos <= c.pos() && c.end() <= g_end {
            return false;
        }
    }
    true
}

fn walk_file(file: &mut File) {
    for d in &mut file.decls {
        walk_decl(d);
    }
}

fn walk_decl(d: &mut Decl) {
    match d {
        Decl::FuncDecl(f) => {
            if let Some(body) = &mut f.body {
                walk_block(body);
            }
        }
        Decl::GenDecl(_) | Decl::BadDecl(_) => {}
    }
}

fn walk_block(b: &mut guff::ast::BlockStmt) {
    for s in &mut b.list {
        walk_stmt(s);
    }
}

fn walk_stmt(s: &mut Stmt) {
    match s {
        Stmt::DeclStmt(d) => walk_decl(&mut d.decl),
        Stmt::LabeledStmt(l) => walk_stmt(&mut l.stmt),
        Stmt::ExprStmt(e) => walk_expr(&mut e.x),
        Stmt::SendStmt(s) => {
            walk_expr(&mut s.chan_);
            walk_expr(&mut s.value);
        }
        Stmt::IncDecStmt(i) => walk_expr(&mut i.x),
        Stmt::AssignStmt(a) => {
            for e in &mut a.lhs {
                walk_expr(e);
            }
            for e in &mut a.rhs {
                walk_expr(e);
            }
        }
        Stmt::GoStmt(g) => walk_expr_call(&mut g.call),
        Stmt::DeferStmt(d) => walk_expr_call(&mut d.call),
        Stmt::ReturnStmt(r) => {
            for e in &mut r.results {
                walk_expr(e);
            }
        }
        Stmt::BlockStmt(b) => walk_block(b),
        Stmt::IfStmt(i) => {
            if let Some(init) = &mut i.init {
                walk_stmt(init);
            }
            walk_expr(&mut i.cond);
            walk_block(&mut i.body);
            if let Some(e) = &mut i.else_ {
                walk_stmt(e);
            }
        }
        Stmt::CaseClause(c) => {
            for e in &mut c.list {
                walk_expr(e);
            }
            for st in &mut c.body {
                walk_stmt(st);
            }
        }
        Stmt::SwitchStmt(sw) => {
            if let Some(init) = &mut sw.init {
                walk_stmt(init);
            }
            if let Some(tag) = &mut sw.tag {
                walk_expr(tag);
            }
            walk_block(&mut sw.body);
        }
        Stmt::TypeSwitchStmt(ts) => {
            if let Some(init) = &mut ts.init {
                walk_stmt(init);
            }
            walk_stmt(&mut ts.assign);
            walk_block(&mut ts.body);
        }
        Stmt::CommClause(c) => {
            if let Some(comm) = &mut c.comm {
                walk_stmt(comm);
            }
            for st in &mut c.body {
                walk_stmt(st);
            }
        }
        Stmt::SelectStmt(sel) => walk_block(&mut sel.body),
        Stmt::ForStmt(f) => {
            if let Some(init) = &mut f.init {
                walk_stmt(init);
            }
            if let Some(cond) = &mut f.cond {
                walk_expr(cond);
            }
            if let Some(post) = &mut f.post {
                walk_stmt(post);
            }
            walk_block(&mut f.body);
        }
        Stmt::RangeStmt(r) => {
            simplify_range(r);
            if let Some(k) = &mut r.key {
                walk_expr(k);
            }
            if let Some(v) = &mut r.value {
                walk_expr(v);
            }
            walk_expr(&mut r.x);
            walk_block(&mut r.body);
        }
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) | Stmt::BranchStmt(_) => {}
    }
}

fn walk_expr_call(c: &mut guff::ast::CallExpr) {
    walk_expr(&mut c.fun);
    for a in &mut c.args {
        walk_expr(a);
    }
}

fn walk_expr(e: &mut Expr) {
    // Apply CompositeLit / SliceExpr simplify at this node first (Go Visit order).
    match e {
        Expr::CompositeLit(c) => {
            simplify_composite(c);
            return; // Go returns nil after simplifying a composite lit
        }
        Expr::SliceExpr(s) => {
            simplify_slice(s);
        }
        _ => {}
    }
    match e {
        Expr::BadExpr(_) | Expr::Ident(_) | Expr::BasicLit(_) => {}
        Expr::Ellipsis(el) => {
            if let Some(x) = &mut el.elt {
                walk_expr(x);
            }
        }
        Expr::FuncLit(f) => {
            walk_block(&mut f.body);
        }
        Expr::ParenExpr(p) => walk_expr(&mut p.x),
        Expr::SelectorExpr(s) => walk_expr(&mut s.x),
        Expr::IndexExpr(i) => {
            walk_expr(&mut i.x);
            walk_expr(&mut i.index);
        }
        Expr::IndexListExpr(i) => {
            walk_expr(&mut i.x);
            for idx in &mut i.indices {
                walk_expr(idx);
            }
        }
        Expr::SliceExpr(s) => {
            walk_expr(&mut s.x);
            if let Some(l) = &mut s.low {
                walk_expr(l);
            }
            if let Some(h) = &mut s.high {
                walk_expr(h);
            }
            if let Some(m) = &mut s.max {
                walk_expr(m);
            }
        }
        Expr::TypeAssertExpr(t) => {
            walk_expr(&mut t.x);
            if let Some(ty) = &mut t.ty {
                walk_expr(ty);
            }
        }
        Expr::CallExpr(c) => walk_expr_call(c),
        Expr::StarExpr(s) => walk_expr(&mut s.x),
        Expr::UnaryExpr(u) => walk_expr(&mut u.x),
        Expr::BinaryExpr(b) => {
            walk_expr(&mut b.x);
            walk_expr(&mut b.y);
        }
        Expr::KeyValueExpr(kv) => {
            walk_expr(&mut kv.key);
            walk_expr(&mut kv.value);
        }
        Expr::ArrayType(a) => {
            if let Some(len) = &mut a.len {
                walk_expr(len);
            }
            walk_expr(&mut a.elt);
        }
        Expr::StructType(s) => {
            for f in &mut s.fields.list {
                if let Some(ty) = &mut f.ty {
                    walk_expr(ty);
                }
            }
        }
        Expr::FuncType(_) => {}
        Expr::InterfaceType(i) => {
            for f in &mut i.methods.list {
                if let Some(ty) = &mut f.ty {
                    walk_expr(ty);
                }
            }
        }
        Expr::MapType(m) => {
            walk_expr(&mut m.key);
            walk_expr(&mut m.value);
        }
        Expr::ChanType(c) => walk_expr(&mut c.value),
        Expr::CompositeLit(_) => unreachable!("handled above"),
    }
}

fn simplify_range(n: &mut RangeStmt) {
    if is_blank(n.value.as_ref()) {
        n.value = None;
    }
    if is_blank(n.key.as_ref()) && n.value.is_none() {
        n.key = None;
    }
}

fn simplify_slice(n: &mut SliceExpr) {
    if n.max.is_some() {
        return;
    }
    let Expr::Ident(s) = n.x.as_ref() else {
        return;
    };
    let s_name = s.name.clone();
    let Some(Expr::CallExpr(call)) = n.high.as_deref() else {
        return;
    };
    if call.args.len() != 1 || call.ellipsis.is_valid() {
        return;
    }
    let Expr::Ident(fun) = call.fun.as_ref() else {
        return;
    };
    if fun.name != "len" {
        return;
    }
    let Expr::Ident(arg) = &call.args[0] else {
        return;
    };
    if arg.name == s_name {
        n.high = None;
    }
}

fn simplify_composite(outer: &mut CompositeLit) {
    let (key_type, elt_type): (Option<Expr>, Option<Expr>) = match outer.ty.as_deref() {
        Some(Expr::ArrayType(a)) => (None, Some(a.elt.as_ref().clone())),
        Some(Expr::MapType(m)) => (Some(m.key.as_ref().clone()), Some(m.value.as_ref().clone())),
        _ => (None, None),
    };
    let Some(elt_type) = elt_type else {
        // Still walk children if we can't simplify the outer type.
        for e in &mut outer.elts {
            walk_expr(e);
        }
        return;
    };

    for elt in &mut outer.elts {
        match elt {
            Expr::KeyValueExpr(kv) => {
                if let Some(kt) = &key_type {
                    simplify_literal(kt, &mut kv.key);
                }
                simplify_literal(&elt_type, &mut kv.value);
            }
            other => simplify_literal(&elt_type, other),
        }
    }
}

fn simplify_literal(ast_type: &Expr, x: &mut Expr) {
    walk_expr(x);

    if let Expr::CompositeLit(inner) = x {
        if let Some(ty) = &inner.ty {
            if expr_match(ast_type, ty) {
                inner.ty = None;
            }
        }
    }

    if let Expr::StarExpr(ptr) = ast_type {
        let drop_amp = if let Expr::UnaryExpr(addr) = &*x {
            addr.op == Token::AND
                && matches!(
                    addr.x.as_ref(),
                    Expr::CompositeLit(inner)
                        if inner.ty.as_ref().is_some_and(|ty| expr_match(&ptr.x, ty))
                )
        } else {
            false
        };
        if drop_amp {
            let placeholder = Expr::BadExpr(guff::ast::BadExpr {
                from: Pos::default(),
                to: Pos::default(),
                id: 0,
            });
            if let Expr::UnaryExpr(addr) = std::mem::replace(x, placeholder) {
                if let Expr::CompositeLit(mut inner) = *addr.x {
                    inner.ty = None;
                    *x = Expr::CompositeLit(inner);
                }
            }
        }
    }
}

fn is_blank(x: Option<&Expr>) -> bool {
    matches!(x, Some(Expr::Ident(id)) if id.name == "_")
}

/// Structural expr equality ignoring positions (gofumpt `match` with m=nil).
pub(crate) fn expr_match(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name,
        (Expr::BasicLit(x), Expr::BasicLit(y)) => x.kind == y.kind && x.value == y.value,
        (Expr::StarExpr(x), Expr::StarExpr(y)) => expr_match(&x.x, &y.x),
        (Expr::ParenExpr(x), Expr::ParenExpr(y)) => expr_match(&x.x, &y.x),
        (Expr::UnaryExpr(x), Expr::UnaryExpr(y)) => x.op == y.op && expr_match(&x.x, &y.x),
        (Expr::BinaryExpr(x), Expr::BinaryExpr(y)) => {
            x.op == y.op && expr_match(&x.x, &y.x) && expr_match(&x.y, &y.y)
        }
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name && expr_match(&x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            expr_match(&x.x, &y.x) && expr_match(&x.index, &y.index)
        }
        (Expr::IndexListExpr(x), Expr::IndexListExpr(y)) => {
            expr_match(&x.x, &y.x)
                && x.indices.len() == y.indices.len()
                && x.indices
                    .iter()
                    .zip(y.indices.iter())
                    .all(|(a, b)| expr_match(a, b))
        }
        (Expr::SliceExpr(x), Expr::SliceExpr(y)) => {
            expr_match(&x.x, &y.x)
                && opt_expr_match(x.low.as_deref(), y.low.as_deref())
                && opt_expr_match(x.high.as_deref(), y.high.as_deref())
                && opt_expr_match(x.max.as_deref(), y.max.as_deref())
                && x.slice3 == y.slice3
        }
        (Expr::CallExpr(x), Expr::CallExpr(y)) => {
            if x.ellipsis.is_valid() != y.ellipsis.is_valid() {
                return false;
            }
            expr_match(&x.fun, &y.fun)
                && x.args.len() == y.args.len()
                && x.args.iter().zip(y.args.iter()).all(|(a, b)| expr_match(a, b))
        }
        (Expr::ArrayType(x), Expr::ArrayType(y)) => {
            opt_expr_match(x.len.as_deref(), y.len.as_deref()) && expr_match(&x.elt, &y.elt)
        }
        (Expr::MapType(x), Expr::MapType(y)) => {
            expr_match(&x.key, &y.key) && expr_match(&x.value, &y.value)
        }
        (Expr::ChanType(x), Expr::ChanType(y)) => x.dir == y.dir && expr_match(&x.value, &y.value),
        (Expr::StructType(x), Expr::StructType(y)) => {
            // Compare printed shapes via field count only when empty; rare in simplify.
            x.fields.list.len() == y.fields.list.len()
                && x.fields
                    .list
                    .iter()
                    .zip(y.fields.list.iter())
                    .all(|(fa, fb)| {
                        fa.names.len() == fb.names.len()
                            && fa.names.iter().zip(fb.names.iter()).all(|(a, b)| a.name == b.name)
                            && match (&fa.ty, &fb.ty) {
                                (None, None) => true,
                                (Some(a), Some(b)) => expr_match(a, b),
                                _ => false,
                            }
                    })
        }
        (Expr::InterfaceType(x), Expr::InterfaceType(y)) => {
            x.methods.list.len() == y.methods.list.len()
        }
        (Expr::Ellipsis(x), Expr::Ellipsis(y)) => {
            opt_expr_match(x.elt.as_deref(), y.elt.as_deref())
        }
        (Expr::KeyValueExpr(x), Expr::KeyValueExpr(y)) => {
            expr_match(&x.key, &y.key) && expr_match(&x.value, &y.value)
        }
        (Expr::CompositeLit(x), Expr::CompositeLit(y)) => {
            opt_expr_match(x.ty.as_deref(), y.ty.as_deref())
                && x.elts.len() == y.elts.len()
                && x.elts.iter().zip(y.elts.iter()).all(|(a, b)| expr_match(a, b))
        }
        (Expr::FuncType(_), Expr::FuncType(_)) => false, // rare; treat as unequal unless identical ptr
        _ => false,
    }
}

fn opt_expr_match(a: Option<&Expr>, b: Option<&Expr>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => expr_match(a, b),
        _ => false,
    }
}

