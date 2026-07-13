// Port of Go's go/parser/resolver.go to Rust.
//
// Original: Copyright 2021 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Walks a parsed `ast::File` to resolve identifiers within file scope
// and populate `file.scope` / `file.unresolved`. As in Go, callers
// can supply a `decl_err` callback to be notified of redeclaration
// errors.
//
// Translation notes:
//
// * Go's `ast.Visitor` "return nil to skip subtree, return v to continue"
//   pattern doesn't map cleanly to our [`crate::walk::Visitor`] trait
//   (which uses `enter`/`leave`). Instead we hand-roll recursive descent
//   over the AST, matching `resolver.go`'s switch statement structure.
// * `*ast.Ident.Obj = ...` mutates an existing AST node. Our
//   [`ast::Ident::obj`] is wrapped in `RefCell<…>` so we can write
//   through `&Ident`.
// * The unresolved-list is `Vec<&'a Ident>` while we hold the file
//   immutably; we convert to owned clones at the end so it can be
//   stored back into `file.unresolved`.

use std::sync::Arc;

use crate::ast::{
    AssignStmt, BlockStmt, CaseClause, CommClause, CompositeLit, Decl, Expr, FieldList, File,
    ForStmt, FuncDecl, FuncLit, FuncType, GenDecl, Ident, IfStmt, InterfaceType, LabeledStmt,
    RangeStmt, SelectStmt, SelectorExpr, Spec, StructType, SwitchStmt, TypeSwitchStmt,
};
use crate::position::{File as PosFile, Pos};
use crate::scope::{ObjData, ObjDecl, ObjKind, Object, Scope};
use crate::token::Token;
use crate::walk::{for_each_child, NodeRef};

const MAX_SCOPE_DEPTH: usize = 1000;

/// Resolve identifiers in `file` against the implicit file scope and
/// populate `file.scope` and `file.unresolved`. The `decl_err` callback
/// is invoked once per redeclaration error (with the offending
/// identifier's position).
pub fn resolve_file<'a>(
    file: &'a mut File,
    handle: &Arc<PosFile>,
    decl_err: Option<Box<dyn Fn(Pos, &str) + 'a>>,
) {
    let pkg_scope = Scope::new(None);
    let still_owned: Vec<Ident>;
    {
        let mut r = Resolver {
            _handle: Arc::clone(handle),
            decl_err,
            pkg_scope: Arc::clone(&pkg_scope),
            top_scope: Some(Arc::clone(&pkg_scope)),
            unresolved: Vec::new(),
            depth: 1,
            label_scope: None,
            target_stack: Vec::new(),
        };

        for decl in file.decls.iter() {
            r.walk_decl(decl);
        }
        r.close_scope();
        debug_assert!(r.top_scope.is_none(), "unbalanced scopes");
        debug_assert!(r.label_scope.is_none(), "unbalanced label scopes");

        // Resolve global identifiers within the same file.
        let mut still: Vec<&Ident> = Vec::new();
        for ident in r.unresolved.drain(..) {
            let obj = r.pkg_scope.lookup(&ident.name);
            *ident.obj.borrow_mut() = obj.clone();
            if obj.is_none() {
                still.push(ident);
            }
        }
        still_owned = still.into_iter().cloned().collect();
    }
    file.scope = Some(pkg_scope);
    file.unresolved = still_owned;
}

struct Resolver<'a> {
    _handle: Arc<PosFile>,
    decl_err: Option<Box<dyn Fn(Pos, &str) + 'a>>,
    pkg_scope: Arc<Scope>,
    top_scope: Option<Arc<Scope>>,
    unresolved: Vec<&'a Ident>,
    depth: usize,
    label_scope: Option<Arc<Scope>>,
    target_stack: Vec<Vec<&'a Ident>>,
}

impl<'a> Resolver<'a> {
    // ---------------- scope housekeeping ----------------------------

    fn open_scope(&mut self) {
        self.depth += 1;
        if self.depth > MAX_SCOPE_DEPTH {
            // Go panics with bailout; here we just stop opening.
            return;
        }
        let outer = self.top_scope.clone();
        self.top_scope = Some(Scope::new(outer));
    }

    fn close_scope(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
        }
        let outer = self.top_scope.as_ref().and_then(|s| s.outer());
        self.top_scope = outer;
    }

    fn open_label_scope(&mut self) {
        let outer = self.label_scope.clone();
        self.label_scope = Some(Scope::new(outer));
        self.target_stack.push(Vec::new());
    }

    fn close_label_scope(&mut self) {
        // Resolve labels collected at this depth.
        let labels = self.target_stack.pop().unwrap_or_default();
        if let Some(scope) = self.label_scope.clone() {
            for ident in labels {
                let obj = scope.lookup(&ident.name);
                *ident.obj.borrow_mut() = obj.clone();
                if obj.is_none() {
                    if let Some(eh) = &self.decl_err {
                        eh(ident.pos(), &format!("label {} undefined", ident.name));
                    }
                }
            }
        }
        let outer = self.label_scope.as_ref().and_then(|s| s.outer());
        self.label_scope = outer;
    }

    // ---------------- declare / resolve ----------------------------

    /// Declare `idents` as new objects in `scope`. `decl` is recorded
    /// on the object so callers can later trace declarations back.
    fn declare(
        &self,
        decl: ObjDecl,
        data: ObjData,
        scope: &Arc<Scope>,
        kind: ObjKind,
        idents: &[&'a Ident],
    ) {
        for ident in idents {
            // Build a fresh object referencing this declaration.
            let mut obj = (*Object::new(kind, &ident.name)).clone();
            obj.decl = decl.clone();
            obj.data = data.clone();
            let arc = Arc::new(obj);

            // Receiver type-parameter identifiers are added to scope
            // but NOT recorded on the ident itself (see go.dev/issue/50956).
            // We approximate that distinction by passing ObjDecl::None for
            // such cases (since they're declared with `decl, ok :=
            // decl.(*ast.Ident)` matching in Go).
            let skip_set_on_ident = matches!(decl, ObjDecl::None);
            if !skip_set_on_ident {
                *ident.obj.borrow_mut() = Some(Arc::clone(&arc));
            }

            if ident.name != "_" {
                if let Some(alt) = scope.insert(Arc::clone(&arc)) {
                    if let Some(eh) = &self.decl_err {
                        let prev = alt.pos();
                        let mut msg = format!("{} redeclared in this block", ident.name);
                        if prev.is_valid() {
                            msg.push_str(&format!("\n\tprevious declaration at {}", prev.0));
                        }
                        eh(ident.pos(), &msg);
                    }
                }
            }
        }
    }

    fn short_var_decl(&self, assign: &'a AssignStmt) {
        // Short variable declarations may redeclare existing variables
        // provided at least one new variable is being introduced.
        let scope = match &self.top_scope {
            Some(s) => Arc::clone(s),
            None => return,
        };
        let mut new_count = 0usize;
        for x in &assign.lhs {
            if let Expr::Ident(ident) = x {
                let mut obj = (*Object::new(ObjKind::Var, &ident.name)).clone();
                obj.decl = ObjDecl::AssignStmt(Box::new(assign.clone()));
                let arc = Arc::new(obj);
                *ident.obj.borrow_mut() = Some(Arc::clone(&arc));
                if ident.name != "_" {
                    if let Some(alt) = scope.insert(Arc::clone(&arc)) {
                        // Redeclaration of existing var in same scope.
                        *ident.obj.borrow_mut() = Some(alt);
                    } else {
                        new_count += 1;
                    }
                }
            }
        }
        if new_count == 0 {
            if let Some(eh) = &self.decl_err {
                let pos = assign.lhs.first().map(|e| e.pos()).unwrap_or_default();
                eh(pos, "no new variables on left side of :=");
            }
        }
    }

    /// Look up `ident` in the enclosing scope chain. If found, set
    /// `ident.obj`. If not found and `collect_unresolved` is true,
    /// add it to the unresolved list.
    fn resolve(&mut self, ident: &'a Ident, collect_unresolved: bool) {
        if ident.name == "_" {
            return;
        }
        let mut cur = self.top_scope.clone();
        while let Some(s) = cur {
            if let Some(obj) = s.lookup(&ident.name) {
                // Receiver type-parameter idents in scope are skipped
                // (matches Go's check that obj.Decl isn't *Ident).
                if !matches!(obj.decl, ObjDecl::None) {
                    *ident.obj.borrow_mut() = Some(obj);
                }
                return;
            }
            cur = s.outer();
        }
        if collect_unresolved {
            self.unresolved.push(ident);
        }
    }

    // ---------------- recursive walk ------------------------------

    fn walk_expr(&mut self, e: &'a Expr) {
        match e {
            Expr::Ident(id) => self.resolve(id, true),
            Expr::FuncLit(fl) => self.walk_func_lit(fl),
            Expr::SelectorExpr(s) => self.walk_selector(s),
            Expr::StructType(s) => self.walk_struct_type(s),
            Expr::FuncType(t) => self.walk_func_type_node(t),
            Expr::InterfaceType(t) => self.walk_interface_type(t),
            Expr::CompositeLit(c) => self.walk_composite_lit(c),
            _ => {
                // Default: walk children using NodeRef dispatch.
                self.walk_children(crate::walk::expr_ref(e));
            }
        }
    }

    fn walk_stmt(&mut self, s: &'a Stmt) {
        match s {
            Stmt::LabeledStmt(l) => self.walk_labeled(l),
            Stmt::AssignStmt(a) => self.walk_assign(a),
            Stmt::BranchStmt(b) => self.walk_branch(b),
            Stmt::BlockStmt(b) => self.walk_block_stmt(b),
            Stmt::IfStmt(i) => self.walk_if(i),
            Stmt::CaseClause(c) => self.walk_case_clause(c),
            Stmt::SwitchStmt(s) => self.walk_switch(s),
            Stmt::TypeSwitchStmt(t) => self.walk_type_switch(t),
            Stmt::CommClause(c) => self.walk_comm_clause(c),
            Stmt::SelectStmt(s) => self.walk_select(s),
            Stmt::ForStmt(f) => self.walk_for_stmt(f),
            Stmt::RangeStmt(r) => self.walk_range_stmt(r),
            _ => self.walk_children(crate::walk::stmt_ref(s)),
        }
    }

    fn walk_decl(&mut self, d: &'a Decl) {
        match d {
            Decl::FuncDecl(f) => self.walk_func_decl(f),
            Decl::GenDecl(g) => self.walk_gen_decl(g),
            Decl::BadDecl(_) => {}
        }
    }

    /// Default walk: descend into every child of `n` using the existing
    /// `for_each_child` dispatch (re-enter the resolver per child).
    fn walk_children(&mut self, n: NodeRef<'a>) {
        for_each_child(n, |c| self.walk_node(c));
    }

    fn walk_node(&mut self, n: NodeRef<'a>) {
        match n {
            NodeRef::Ident(id) => self.resolve(id, true),
            NodeRef::FuncLit(fl) => self.walk_func_lit(fl),
            NodeRef::SelectorExpr(s) => self.walk_selector(s),
            NodeRef::StructType(s) => self.walk_struct_type(s),
            NodeRef::FuncType(t) => self.walk_func_type_node(t),
            NodeRef::InterfaceType(t) => self.walk_interface_type(t),
            NodeRef::CompositeLit(c) => self.walk_composite_lit(c),
            NodeRef::LabeledStmt(l) => self.walk_labeled(l),
            NodeRef::AssignStmt(a) => self.walk_assign(a),
            NodeRef::BranchStmt(b) => self.walk_branch(b),
            NodeRef::BlockStmt(b) => self.walk_block_stmt(b),
            NodeRef::IfStmt(i) => self.walk_if(i),
            NodeRef::CaseClause(c) => self.walk_case_clause(c),
            NodeRef::SwitchStmt(s) => self.walk_switch(s),
            NodeRef::TypeSwitchStmt(t) => self.walk_type_switch(t),
            NodeRef::CommClause(c) => self.walk_comm_clause(c),
            NodeRef::SelectStmt(s) => self.walk_select(s),
            NodeRef::ForStmt(f) => self.walk_for_stmt(f),
            NodeRef::RangeStmt(r) => self.walk_range_stmt(r),
            NodeRef::FuncDecl(f) => self.walk_func_decl(f),
            NodeRef::GenDecl(g) => self.walk_gen_decl(g),
            _ => for_each_child(n, |c| self.walk_node(c)),
        }
    }

    // ---------- per-variant handlers (mirroring resolver.go cases) --

    fn walk_func_lit(&mut self, fl: &'a FuncLit) {
        self.open_scope();
        self.walk_func_type_signature(&fl.ty);
        self.walk_body(&fl.body);
        self.close_scope();
    }

    fn walk_selector(&mut self, s: &'a SelectorExpr) {
        self.walk_expr(&s.x);
        // Don't try to resolve `Sel` — no qualified resolution.
    }

    fn walk_struct_type(&mut self, s: &'a StructType) {
        self.open_scope();
        self.walk_field_list(&s.fields, ObjKind::Var);
        self.close_scope();
    }

    fn walk_func_type_node(&mut self, t: &'a FuncType) {
        self.open_scope();
        self.walk_func_type_signature(t);
        self.close_scope();
    }

    fn walk_interface_type(&mut self, t: &'a InterfaceType) {
        self.open_scope();
        self.walk_field_list(&t.methods, ObjKind::Fun);
        self.close_scope();
    }

    fn walk_composite_lit(&mut self, c: &'a CompositeLit) {
        if let Some(t) = &c.ty {
            self.walk_expr(t);
        }
        for e in &c.elts {
            if let Expr::KeyValueExpr(kv) = e {
                // Try to resolve the key as an ident — but don't add to
                // unresolved if it doesn't resolve (go.dev/issue/45160).
                if let Expr::Ident(id) = &*kv.key {
                    self.resolve(id, false);
                } else {
                    self.walk_expr(&kv.key);
                }
                self.walk_expr(&kv.value);
            } else {
                self.walk_expr(e);
            }
        }
    }

    fn walk_labeled(&mut self, l: &'a LabeledStmt) {
        if let Some(scope) = self.label_scope.clone() {
            self.declare(
                ObjDecl::LabeledStmt(Box::new(l.clone())),
                ObjData::None,
                &scope,
                ObjKind::Lbl,
                &[&l.label],
            );
        }
        self.walk_stmt(&l.stmt);
    }

    fn walk_assign(&mut self, a: &'a AssignStmt) {
        for e in &a.rhs {
            self.walk_expr(e);
        }
        if a.tok == Some(Token::DEFINE) {
            self.short_var_decl(a);
        } else {
            for e in &a.lhs {
                self.walk_expr(e);
            }
        }
    }

    fn walk_branch(&mut self, b: &'a crate::ast::BranchStmt) {
        if b.tok != Token::FALLTHROUGH {
            if let Some(label) = &b.label {
                if let Some(top) = self.target_stack.last_mut() {
                    top.push(label);
                }
            }
        }
    }

    fn walk_block_stmt(&mut self, b: &'a BlockStmt) {
        self.open_scope();
        for s in &b.list {
            self.walk_stmt(s);
        }
        self.close_scope();
    }

    fn walk_if(&mut self, i: &'a IfStmt) {
        self.open_scope();
        if let Some(init) = &i.init {
            self.walk_stmt(init);
        }
        self.walk_expr(&i.cond);
        self.walk_block_stmt(&i.body);
        if let Some(el) = &i.else_ {
            self.walk_stmt(el);
        }
        self.close_scope();
    }

    fn walk_case_clause(&mut self, c: &'a CaseClause) {
        for e in &c.list {
            self.walk_expr(e);
        }
        self.open_scope();
        for s in &c.body {
            self.walk_stmt(s);
        }
        self.close_scope();
    }

    fn walk_switch(&mut self, s: &'a SwitchStmt) {
        self.open_scope();
        if let Some(init) = &s.init {
            self.walk_stmt(init);
        }
        if let Some(tag) = &s.tag {
            // Match Go's extra scope when both init and tag are present.
            let extra = s.init.is_some();
            if extra {
                self.open_scope();
            }
            self.walk_expr(tag);
            if extra {
                self.close_scope();
            }
        }
        for st in &s.body.list {
            self.walk_stmt(st);
        }
        self.close_scope();
    }

    fn walk_type_switch(&mut self, t: &'a TypeSwitchStmt) {
        let init_scope = t.init.is_some();
        if init_scope {
            self.open_scope();
            if let Some(init) = &t.init {
                self.walk_stmt(init);
            }
        }
        self.open_scope();
        self.walk_stmt(&t.assign);
        for st in &t.body.list {
            self.walk_stmt(st);
        }
        self.close_scope();
        if init_scope {
            self.close_scope();
        }
    }

    fn walk_comm_clause(&mut self, c: &'a CommClause) {
        self.open_scope();
        if let Some(comm) = &c.comm {
            self.walk_stmt(comm);
        }
        for s in &c.body {
            self.walk_stmt(s);
        }
        self.close_scope();
    }

    fn walk_select(&mut self, s: &'a SelectStmt) {
        for st in &s.body.list {
            self.walk_stmt(st);
        }
    }

    fn walk_for_stmt(&mut self, f: &'a ForStmt) {
        self.open_scope();
        if let Some(init) = &f.init {
            self.walk_stmt(init);
        }
        if let Some(cond) = &f.cond {
            self.walk_expr(cond);
        }
        if let Some(post) = &f.post {
            self.walk_stmt(post);
        }
        self.walk_block_stmt(&f.body);
        self.close_scope();
    }

    fn walk_range_stmt(&mut self, r: &'a RangeStmt) {
        self.open_scope();
        self.walk_expr(&r.x);
        let mut lhs_idents: Vec<&'a Ident> = Vec::new();
        if let Some(Expr::Ident(id)) = &r.key {
            lhs_idents.push(id);
        }
        if let Some(Expr::Ident(id)) = &r.value {
            lhs_idents.push(id);
        }
        if !lhs_idents.is_empty() && r.tok == Some(Token::DEFINE) {
            // Synthesize an AssignStmt to feed shortVarDecl with proper
            // LHS / RHS. Since we only need it to register the idents,
            // build a minimal AST.
            let lhs_exprs: Vec<Expr> = lhs_idents
                .iter()
                .map(|id| Expr::Ident((*id).clone()))
                .collect();
            let synth = AssignStmt {
                lhs: lhs_exprs,
                tok_pos: r.tok_pos,
                tok: Some(Token::DEFINE),
                rhs: vec![],
            };
            // shortVarDecl reads .lhs only via Ident match — pass the
            // synthesized stmt but register against the ORIGINAL idents
            // by manually mimicking the logic:
            self.short_var_decl_for_idents(&synth, &lhs_idents);
        } else if !lhs_idents.is_empty() {
            // Walk normally.
            if let Some(k) = &r.key {
                self.walk_expr(k);
            }
            if let Some(v) = &r.value {
                self.walk_expr(v);
            }
        }
        self.walk_block_stmt(&r.body);
        self.close_scope();
    }

    fn short_var_decl_for_idents(&self, decl: &AssignStmt, idents: &[&'a Ident]) {
        let scope = match &self.top_scope {
            Some(s) => Arc::clone(s),
            None => return,
        };
        let mut new_count = 0usize;
        for ident in idents {
            let mut obj = (*Object::new(ObjKind::Var, &ident.name)).clone();
            obj.decl = ObjDecl::AssignStmt(Box::new(decl.clone()));
            let arc = Arc::new(obj);
            *ident.obj.borrow_mut() = Some(Arc::clone(&arc));
            if ident.name != "_" {
                if let Some(alt) = scope.insert(Arc::clone(&arc)) {
                    *ident.obj.borrow_mut() = Some(alt);
                } else {
                    new_count += 1;
                }
            }
        }
        if new_count == 0 {
            if let Some(eh) = &self.decl_err {
                eh(
                    idents.first().map(|i| i.pos()).unwrap_or_default(),
                    "no new variables on left side of :=",
                );
            }
        }
    }

    // ---------- declarations -----------------------------------------

    fn walk_gen_decl(&mut self, g: &'a GenDecl) {
        match g.tok {
            Some(Token::CONST) | Some(Token::VAR) => {
                let kind = if g.tok == Some(Token::VAR) {
                    ObjKind::Var
                } else {
                    ObjKind::Con
                };
                let scope = match &self.top_scope {
                    Some(s) => Arc::clone(s),
                    None => return,
                };
                for (i, spec) in g.specs.iter().enumerate() {
                    if let Spec::ValueSpec(vs) = spec {
                        for v in &vs.values {
                            self.walk_expr(v);
                        }
                        if let Some(t) = &vs.ty {
                            self.walk_expr(t);
                        }
                        let names: Vec<&'a Ident> = vs.names.iter().collect();
                        self.declare(
                            ObjDecl::ValueSpec(Box::new(vs.clone())),
                            ObjData::Int(i as i64),
                            &scope,
                            kind,
                            &names,
                        );
                    }
                }
            }
            Some(Token::TYPE) => {
                let scope = match &self.top_scope {
                    Some(s) => Arc::clone(s),
                    None => return,
                };
                for spec in &g.specs {
                    if let Spec::TypeSpec(ts) = spec {
                        self.declare(
                            ObjDecl::TypeSpec(Box::new(ts.clone())),
                            ObjData::None,
                            &scope,
                            ObjKind::Typ,
                            &[&ts.name],
                        );
                        if let Some(tp) = &ts.type_params {
                            self.open_scope();
                            self.walk_tparams(tp);
                            self.walk_expr(&ts.ty);
                            self.close_scope();
                        } else {
                            self.walk_expr(&ts.ty);
                        }
                    }
                }
            }
            _ => {
                // IMPORT and others: walk children with defaults.
                self.walk_children(NodeRef::GenDecl(g));
            }
        }
    }

    fn walk_func_decl(&mut self, f: &'a FuncDecl) {
        self.open_scope();
        self.walk_recv(f.recv.as_ref());
        if let Some(tp) = &f.ty.type_params {
            self.walk_tparams(tp);
        }
        self.resolve_list(f.ty.params.as_ref());
        self.resolve_list(f.ty.results.as_ref());
        self.declare_list(f.recv.as_ref(), ObjKind::Var);
        self.declare_list(f.ty.params.as_ref(), ObjKind::Var);
        self.declare_list(f.ty.results.as_ref(), ObjKind::Var);
        if let Some(body) = &f.body {
            self.walk_body(body);
        }
        if f.recv.is_none() && f.name.name != "init" {
            let scope = Arc::clone(&self.pkg_scope);
            self.declare(
                ObjDecl::FuncDecl(Box::new(f.clone())),
                ObjData::None,
                &scope,
                ObjKind::Fun,
                &[&f.name],
            );
        }
        self.close_scope();
    }

    // ---------- FuncType / FieldList plumbing -----------------------

    fn walk_func_type_signature(&mut self, t: &'a FuncType) {
        // Note: type_params handled separately for FuncDecls.
        self.resolve_list(t.params.as_ref());
        self.resolve_list(t.results.as_ref());
        self.declare_list(t.params.as_ref(), ObjKind::Var);
        self.declare_list(t.results.as_ref(), ObjKind::Var);
    }

    fn resolve_list(&mut self, list: Option<&'a FieldList>) {
        if let Some(fl) = list {
            for f in &fl.list {
                if let Some(t) = &f.ty {
                    self.walk_expr(t);
                }
            }
        }
    }

    fn declare_list(&mut self, list: Option<&'a FieldList>, kind: ObjKind) {
        if let Some(fl) = list {
            let scope = match &self.top_scope {
                Some(s) => Arc::clone(s),
                None => return,
            };
            for f in &fl.list {
                let names: Vec<&'a Ident> = f.names.iter().collect();
                self.declare(
                    ObjDecl::Field(Box::new(f.clone())),
                    ObjData::None,
                    &scope,
                    kind,
                    &names,
                );
            }
        }
    }

    fn walk_recv(&mut self, recv: Option<&'a FieldList>) {
        let Some(recv) = recv else { return };
        let Some(first) = recv.list.first() else {
            return;
        };
        let Some(ty) = &first.ty else { return };
        let typ = match ty {
            Expr::StarExpr(s) => &*s.x,
            other => other,
        };
        let mut declare_exprs: Vec<&'a Expr> = Vec::new();
        let mut resolve_exprs: Vec<&'a Expr> = Vec::new();
        match typ {
            Expr::IndexExpr(ix) => {
                declare_exprs.push(&ix.index);
                resolve_exprs.push(&ix.x);
            }
            Expr::IndexListExpr(ix) => {
                for i in &ix.indices {
                    declare_exprs.push(i);
                }
                resolve_exprs.push(&ix.x);
            }
            other => resolve_exprs.push(other),
        }
        let scope = match &self.top_scope {
            Some(s) => Arc::clone(s),
            None => return,
        };
        for expr in declare_exprs {
            if let Expr::Ident(id) = expr {
                self.declare(
                    ObjDecl::None, // see go.dev/issue/50956 — only adds to scope
                    ObjData::None,
                    &scope,
                    ObjKind::Typ,
                    &[id],
                );
            } else {
                resolve_exprs.push(expr);
            }
        }
        for expr in resolve_exprs {
            self.walk_expr(expr);
        }
        // Remaining receivers (the parser tolerates >1 entry though Go
        // spec allows only one — walk them too for robustness).
        for f in &recv.list[1..] {
            if let Some(t) = &f.ty {
                self.walk_expr(t);
            }
        }
    }

    fn walk_field_list(&mut self, list: &'a FieldList, kind: ObjKind) {
        self.resolve_list(Some(list));
        self.declare_list(Some(list), kind);
    }

    fn walk_tparams(&mut self, list: &'a FieldList) {
        // Type parameters are declared eagerly so they can be referenced
        // in constraint expressions held in field.Type.
        self.declare_list(Some(list), ObjKind::Typ);
        self.resolve_list(Some(list));
    }

    fn walk_body(&mut self, body: &'a BlockStmt) {
        self.open_label_scope();
        for s in &body.list {
            self.walk_stmt(s);
        }
        self.close_label_scope();
    }
}

// Local re-exports / aliases used in method bodies.
use crate::ast::Stmt;

// ====================================================================
// Tests
// ====================================================================
//
// Note: the upstream `resolver_test.go` relies on `ParseFile`, which
// belongs to `parser.go` (porting in progress). The tests here exercise
// the resolver on hand-built ASTs.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BlockStmt, FuncType, Ident, ImportSpec, ValueSpec};
    use crate::position::FileSet;

    fn dummy_handle(fset: &Arc<FileSet>) -> Arc<PosFile> {
        fset.add_file("t.go", fset.base(), 1000)
    }

    fn vs(names: &[&str]) -> ValueSpec {
        ValueSpec {
            names: names.iter().map(|n| Ident::new_ident(*n)).collect(),
            ..Default::default()
        }
    }

    fn const_decl(names: &[&str]) -> Decl {
        Decl::GenDecl(GenDecl {
            tok: Some(Token::CONST),
            tok_pos: Pos(1),
            specs: vec![Spec::ValueSpec(vs(names))],
            ..Default::default()
        })
    }

    #[test]
    fn top_level_consts_go_into_pkg_scope() {
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls: vec![const_decl(&["A", "B"])],
            ..Default::default()
        };
        resolve_file(&mut file, &handle, None);
        let scope = file.scope.as_ref().expect("scope set");
        assert!(scope.lookup("A").is_some());
        assert!(scope.lookup("B").is_some());
        // Idents in the GenDecl now carry .obj
        if let Decl::GenDecl(g) = &file.decls[0] {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                assert!(v.names[0].obj.borrow().is_some());
                assert!(v.names[1].obj.borrow().is_some());
            }
        }
    }

    #[test]
    fn duplicate_declaration_reports_via_callback() {
        use std::cell::RefCell;
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls: vec![const_decl(&["X"]), const_decl(&["X"])],
            ..Default::default()
        };
        let errors: Arc<RefCell<Vec<String>>> = Arc::new(RefCell::new(Vec::new()));
        let inner = Arc::clone(&errors);
        let eh: Box<dyn Fn(Pos, &str)> = Box::new(move |_pos, msg| {
            inner.borrow_mut().push(msg.to_string());
        });
        resolve_file(&mut file, &handle, Some(eh));
        let errs = errors.borrow();
        assert!(
            errs.iter()
                .any(|m| m.contains("X redeclared in this block")),
            "expected redeclaration error, got {:?}",
            errs
        );
    }

    #[test]
    fn ident_referencing_another_const_resolves() {
        // const A = 0; const B = A
        let a_spec = ValueSpec {
            names: vec![Ident::new_ident("A")],
            ..Default::default()
        };
        let b_spec = ValueSpec {
            names: vec![Ident::new_ident("B")],
            values: vec![Expr::Ident(Ident::new_ident("A"))],
            ..Default::default()
        };
        let decls = vec![
            Decl::GenDecl(GenDecl {
                tok: Some(Token::CONST),
                tok_pos: Pos(1),
                specs: vec![Spec::ValueSpec(a_spec)],
                ..Default::default()
            }),
            Decl::GenDecl(GenDecl {
                tok: Some(Token::CONST),
                tok_pos: Pos(10),
                specs: vec![Spec::ValueSpec(b_spec)],
                ..Default::default()
            }),
        ];
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls,
            ..Default::default()
        };
        resolve_file(&mut file, &handle, None);
        // The `A` use in B's value must have resolved to the pkg-scope object.
        if let Decl::GenDecl(g) = &file.decls[1] {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                if let Expr::Ident(used) = &v.values[0] {
                    assert!(used.obj.borrow().is_some(), "use of A should resolve");
                }
            }
        }
        assert!(file.unresolved.is_empty());
    }

    #[test]
    fn unresolved_idents_remain_in_file_unresolved() {
        let spec = ValueSpec {
            names: vec![Ident::new_ident("X")],
            values: vec![Expr::Ident(Ident::new_ident("Nowhere"))],
            ..Default::default()
        };
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls: vec![Decl::GenDecl(GenDecl {
                tok: Some(Token::CONST),
                tok_pos: Pos(1),
                specs: vec![Spec::ValueSpec(spec)],
                ..Default::default()
            })],
            ..Default::default()
        };
        resolve_file(&mut file, &handle, None);
        assert_eq!(file.unresolved.len(), 1);
        assert_eq!(file.unresolved[0].name, "Nowhere");
    }

    #[test]
    fn func_decl_at_pkg_scope() {
        let f = FuncDecl {
            doc: None,
            recv: None,
            name: Ident::new_ident("main"),
            ty: FuncType {
                id: 0,
                func: Pos(1),
                type_params: None,
                params: Some(FieldList::default()),
                results: None,
            },
            body: Some(BlockStmt {
                lbrace: Pos::default(),
                list: vec![],
                rbrace: Pos(1),
                id: 0,
            }),
        };
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls: vec![Decl::FuncDecl(f)],
            ..Default::default()
        };
        resolve_file(&mut file, &handle, None);
        let scope = file.scope.as_ref().unwrap();
        assert!(scope.lookup("main").is_some());
    }

    #[test]
    fn init_func_is_not_declared_in_pkg_scope() {
        let f = FuncDecl {
            doc: None,
            recv: None,
            name: Ident::new_ident("init"),
            ty: FuncType {
                id: 0,
                func: Pos(1),
                type_params: None,
                params: Some(FieldList::default()),
                results: None,
            },
            body: Some(BlockStmt {
                lbrace: Pos::default(),
                list: vec![],
                rbrace: Pos(1),
                id: 0,
            }),
        };
        let fset = FileSet::new();
        let handle = dummy_handle(&fset);
        let mut file = File {
            decls: vec![Decl::FuncDecl(f)],
            ..Default::default()
        };
        resolve_file(&mut file, &handle, None);
        let scope = file.scope.as_ref().unwrap();
        assert!(
            scope.lookup("init").is_none(),
            "init should not be in pkg scope"
        );
    }

    // Silence unused-warnings for items only referenced by parser.rs.
    #[allow(dead_code)]
    fn _ensure_imports_referenced(_: &ImportSpec) {}
}
