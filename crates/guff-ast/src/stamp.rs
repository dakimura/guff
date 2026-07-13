//! Post-parse stamping pass that assigns stable node ids to every [`Expr`]
//! (and any not-yet-stamped [`Ident`]) in a freshly parsed tree.
//!
//! # Why a separate pass
//!
//! Go's `go/types` keys its `Info` maps (`Defs`/`Uses`/`Types`) on the
//! `*ast.Ident` / `*ast.Expr` pointer. This port cannot rely on pointer
//! identity: the type checker clones `Expr`/`Ident` values freely (e.g. when
//! storing the evaluated node in an [`crate::ast`]-shaped operand), so two
//! values denoting the same source node would be distinct allocations.
//!
//! Instead every expression carries a `u32` id (see [`Ident::id`] and the
//! `id` field on each `Expr` variant struct), and the maps key on that. The
//! parser already stamps identifiers inline as it builds them; this pass walks
//! the parsed tree once and assigns a fresh [`next_node_id`] to **every other**
//! `Expr` node (any node whose id is still `0`). Clones made later by the type
//! checker inherit the id via `#[derive(Clone)]`, so they map back to the same
//! source node.
//!
//! Nodes the checker synthesises itself (e.g. the `x <op> 1` expression built
//! for an `IncDec` statement) never go through this pass, keep id `0`, and are
//! therefore never recorded — matching Go's behaviour of only recording nodes
//! that originate in the source.
//!
//! The pass must run exactly once per parsed file, after parsing and before any
//! cloning. [`crate::parser::parse_file`] calls [`stamp_node_ids`]; the
//! expression-only entry points call [`stamp_expr_ids`].

use crate::ast::*;

/// Assign ids to every unstamped node reachable from `file`.
pub fn stamp_node_ids(file: &mut File) {
    if file.id == 0 {
        file.id = next_node_id();
    }
    stamp_ident(&mut file.name);
    for d in &mut file.decls {
        stamp_decl(d);
    }
}

/// Assign ids to every unstamped node in a standalone expression (used by the
/// `parse_expr*` entry points, which produce an `Expr` with no enclosing file).
pub fn stamp_expr_ids(e: &mut Expr) {
    stamp_expr(e);
}

// ============================================================
// Leaves / helpers
// ============================================================

#[inline]
fn stamp_ident(id: &mut Ident) {
    if id.id == 0 {
        id.id = next_node_id();
    }
}

/// Stamp a bare [`FuncType`] (one not wrapped in `Expr::FuncType`, as appears in
/// `FuncLit.ty` and `FuncDecl.ty`).
fn stamp_func_type(ft: &mut FuncType) {
    if ft.id == 0 {
        ft.id = next_node_id();
    }
    if let Some(tp) = &mut ft.type_params {
        stamp_field_list(tp);
    }
    if let Some(p) = &mut ft.params {
        stamp_field_list(p);
    }
    if let Some(r) = &mut ft.results {
        stamp_field_list(r);
    }
}

fn stamp_field_list(fl: &mut FieldList) {
    for f in &mut fl.list {
        stamp_field(f);
    }
}

fn stamp_field(f: &mut Field) {
    if f.id == 0 {
        f.id = next_node_id();
    }
    for n in &mut f.names {
        stamp_ident(n);
    }
    if let Some(t) = &mut f.ty {
        stamp_expr(t);
    }
    // `f.tag` is a bare `BasicLit` that the checker never evaluates as an
    // expression, so it is intentionally left unstamped.
}

// ============================================================
// Expressions
// ============================================================

fn stamp_expr(e: &mut Expr) {
    // Stamp this node first (unless already stamped, e.g. an `Expr::Ident`
    // whose inner identifier the parser already stamped), then recurse.
    if e.id() == 0 {
        e.set_id(next_node_id());
    }
    match e {
        Expr::BadExpr(_) | Expr::Ident(_) | Expr::BasicLit(_) => {}

        Expr::Ellipsis(x) => {
            if let Some(elt) = &mut x.elt {
                stamp_expr(elt);
            }
        }
        Expr::FuncLit(x) => {
            stamp_func_type(&mut x.ty);
            stamp_block(&mut x.body);
        }
        Expr::CompositeLit(x) => {
            if let Some(t) = &mut x.ty {
                stamp_expr(t);
            }
            for elt in &mut x.elts {
                stamp_expr(elt);
            }
        }
        Expr::ParenExpr(x) => stamp_expr(&mut x.x),
        Expr::SelectorExpr(x) => {
            stamp_expr(&mut x.x);
            stamp_ident(&mut x.sel);
        }
        Expr::IndexExpr(x) => {
            stamp_expr(&mut x.x);
            stamp_expr(&mut x.index);
        }
        Expr::IndexListExpr(x) => {
            stamp_expr(&mut x.x);
            for i in &mut x.indices {
                stamp_expr(i);
            }
        }
        Expr::SliceExpr(x) => {
            stamp_expr(&mut x.x);
            if let Some(lo) = &mut x.low {
                stamp_expr(lo);
            }
            if let Some(hi) = &mut x.high {
                stamp_expr(hi);
            }
            if let Some(mx) = &mut x.max {
                stamp_expr(mx);
            }
        }
        Expr::TypeAssertExpr(x) => {
            stamp_expr(&mut x.x);
            if let Some(t) = &mut x.ty {
                stamp_expr(t);
            }
        }
        Expr::CallExpr(x) => {
            stamp_expr(&mut x.fun);
            for a in &mut x.args {
                stamp_expr(a);
            }
        }
        Expr::StarExpr(x) => stamp_expr(&mut x.x),
        Expr::UnaryExpr(x) => stamp_expr(&mut x.x),
        Expr::BinaryExpr(x) => {
            stamp_expr(&mut x.x);
            stamp_expr(&mut x.y);
        }
        Expr::KeyValueExpr(x) => {
            stamp_expr(&mut x.key);
            stamp_expr(&mut x.value);
        }
        Expr::ArrayType(x) => {
            if let Some(l) = &mut x.len {
                stamp_expr(l);
            }
            stamp_expr(&mut x.elt);
        }
        Expr::StructType(x) => stamp_field_list(&mut x.fields),
        Expr::FuncType(x) => {
            if let Some(tp) = &mut x.type_params {
                stamp_field_list(tp);
            }
            if let Some(p) = &mut x.params {
                stamp_field_list(p);
            }
            if let Some(r) = &mut x.results {
                stamp_field_list(r);
            }
        }
        Expr::InterfaceType(x) => stamp_field_list(&mut x.methods),
        Expr::MapType(x) => {
            stamp_expr(&mut x.key);
            stamp_expr(&mut x.value);
        }
        Expr::ChanType(x) => stamp_expr(&mut x.value),
    }
}

/// Stamp a bare [`CallExpr`] (as appears in `GoStmt.call` / `DeferStmt.call`).
fn stamp_call(c: &mut CallExpr) {
    if c.id == 0 {
        c.id = next_node_id();
    }
    stamp_expr(&mut c.fun);
    for a in &mut c.args {
        stamp_expr(a);
    }
}

// ============================================================
// Statements
// ============================================================

fn stamp_block(b: &mut BlockStmt) {
    if b.id == 0 {
        b.id = next_node_id();
    }
    for s in &mut b.list {
        stamp_stmt(s);
    }
}

fn stamp_stmt(s: &mut Stmt) {
    match s {
        Stmt::BadStmt(_) | Stmt::EmptyStmt(_) => {}
        Stmt::DeclStmt(s) => stamp_decl(&mut s.decl),
        Stmt::LabeledStmt(s) => {
            stamp_ident(&mut s.label);
            stamp_stmt(&mut s.stmt);
        }
        Stmt::ExprStmt(s) => stamp_expr(&mut s.x),
        Stmt::SendStmt(s) => {
            stamp_expr(&mut s.chan_);
            stamp_expr(&mut s.value);
        }
        Stmt::IncDecStmt(s) => stamp_expr(&mut s.x),
        Stmt::AssignStmt(s) => {
            for e in &mut s.lhs {
                stamp_expr(e);
            }
            for e in &mut s.rhs {
                stamp_expr(e);
            }
        }
        Stmt::GoStmt(s) => stamp_call(&mut s.call),
        Stmt::DeferStmt(s) => stamp_call(&mut s.call),
        Stmt::ReturnStmt(s) => {
            for e in &mut s.results {
                stamp_expr(e);
            }
        }
        Stmt::BranchStmt(s) => {
            if let Some(l) = &mut s.label {
                stamp_ident(l);
            }
        }
        Stmt::BlockStmt(s) => stamp_block(s),
        Stmt::IfStmt(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(init) = &mut s.init {
                stamp_stmt(init);
            }
            stamp_expr(&mut s.cond);
            stamp_block(&mut s.body);
            if let Some(e) = &mut s.else_ {
                stamp_stmt(e);
            }
        }
        Stmt::CaseClause(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            for e in &mut s.list {
                stamp_expr(e);
            }
            for st in &mut s.body {
                stamp_stmt(st);
            }
        }
        Stmt::SwitchStmt(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(init) = &mut s.init {
                stamp_stmt(init);
            }
            if let Some(t) = &mut s.tag {
                stamp_expr(t);
            }
            stamp_block(&mut s.body);
        }
        Stmt::TypeSwitchStmt(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(init) = &mut s.init {
                stamp_stmt(init);
            }
            stamp_stmt(&mut s.assign);
            stamp_block(&mut s.body);
        }
        Stmt::CommClause(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(c) = &mut s.comm {
                stamp_stmt(c);
            }
            for st in &mut s.body {
                stamp_stmt(st);
            }
        }
        Stmt::SelectStmt(s) => stamp_block(&mut s.body),
        Stmt::ForStmt(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(init) = &mut s.init {
                stamp_stmt(init);
            }
            if let Some(c) = &mut s.cond {
                stamp_expr(c);
            }
            if let Some(p) = &mut s.post {
                stamp_stmt(p);
            }
            stamp_block(&mut s.body);
        }
        Stmt::RangeStmt(s) => {
            if s.id == 0 {
                s.id = next_node_id();
            }
            if let Some(k) = &mut s.key {
                stamp_expr(k);
            }
            if let Some(val) = &mut s.value {
                stamp_expr(val);
            }
            stamp_expr(&mut s.x);
            stamp_block(&mut s.body);
        }
    }
}

// ============================================================
// Declarations / specs
// ============================================================

fn stamp_decl(d: &mut Decl) {
    match d {
        Decl::BadDecl(_) => {}
        Decl::GenDecl(d) => {
            for sp in &mut d.specs {
                stamp_spec(sp);
            }
        }
        Decl::FuncDecl(d) => {
            if let Some(recv) = &mut d.recv {
                stamp_field_list(recv);
            }
            stamp_ident(&mut d.name);
            stamp_func_type(&mut d.ty);
            if let Some(body) = &mut d.body {
                stamp_block(body);
            }
        }
    }
}

fn stamp_spec(sp: &mut Spec) {
    match sp {
        Spec::ImportSpec(sp) => {
            if sp.id == 0 {
                sp.id = next_node_id();
            }
            if let Some(n) = &mut sp.name {
                stamp_ident(n);
            }
            // sp.path is a bare BasicLit (not evaluated as an expression).
        }
        Spec::ValueSpec(sp) => {
            for n in &mut sp.names {
                stamp_ident(n);
            }
            if let Some(t) = &mut sp.ty {
                stamp_expr(t);
            }
            for val in &mut sp.values {
                stamp_expr(val);
            }
        }
        Spec::TypeSpec(sp) => {
            if sp.id == 0 {
                sp.id = next_node_id();
            }
            stamp_ident(&mut sp.name);
            if let Some(tp) = &mut sp.type_params {
                stamp_field_list(tp);
            }
            stamp_expr(&mut sp.ty);
        }
    }
}
