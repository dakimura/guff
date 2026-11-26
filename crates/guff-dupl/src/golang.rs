//! Go AST → uniform syntax tree (port of `syntax/golang/golang.go`).

use guff::ast::{
    Decl, Expr, File, FuncDecl, Spec, Stmt,
};
use guff::position::{FileSet, Pos};
use guff::token::Token;

use crate::node_type::*;
use crate::syntax::SyntaxNode;

struct Transformer<'a> {
    fset: &'a FileSet,
    filename: String,
}

impl<'a> Transformer<'a> {
    fn offsets(&self, pos: Pos, end: Pos) -> (i32, i32) {
        let file = self.fset.file(pos).expect("position in file");
        (file.offset(pos) as i32, file.offset(end) as i32)
    }

    fn node(&self, node_type: i32, pos: Pos, end: Pos) -> SyntaxNode {
        let (pos, end) = self.offsets(pos, end);
        SyntaxNode::new(node_type, self.filename.clone(), pos, end)
    }

    fn trans_file(&self, file: &File) -> SyntaxNode {
        let mut o = self.node(FILE, file.pos(), file.end());
        for decl in &file.decls {
            if let Decl::GenDecl(g) = decl {
                if g.tok == Some(Token::IMPORT) {
                    continue;
                }
            }
            o.children.push(self.trans_decl(decl));
        }
        o
    }

    fn trans_decl(&self, decl: &Decl) -> SyntaxNode {
        match decl {
            Decl::BadDecl(d) => self.node(BAD_NODE, d.from, d.to),
            Decl::GenDecl(d) => {
                let end = if d.rparen.is_valid() {
                    Pos(d.rparen.0 + 1)
                } else {
                    d.specs.first().map(|s| s.end()).unwrap_or(d.tok_pos)
                };
                let mut o = self.node(GEN_DECL, d.tok_pos, end);
                for spec in &d.specs {
                    o.children.push(self.trans_spec(spec));
                }
                o
            }
            Decl::FuncDecl(d) => self.trans_func_decl(d),
        }
    }

    fn trans_func_decl(&self, d: &FuncDecl) -> SyntaxNode {
        let end = d
            .body
            .as_ref()
            .map(|b| b.end())
            .unwrap_or_else(|| d.ty.end());
        let mut o = self.node(FUNC_DECL, d.ty.pos(), end);
        if let Some(recv) = &d.recv {
            o.children.push(self.trans_field_list(recv));
        }
        o.children.push(self.trans_ident(&d.name));
        o.children.push(self.trans_func_type(&d.ty));
        if let Some(body) = &d.body {
            o.children.push(self.trans_block(body));
        }
        o
    }

    fn trans_spec(&self, spec: &Spec) -> SyntaxNode {
        match spec {
            Spec::ImportSpec(s) => {
                let pos = s
                    .name
                    .as_ref()
                    .map(|n| n.pos())
                    .unwrap_or(s.path.value_pos);
                let end = if s.end_pos.0 != 0 {
                    s.end_pos
                } else {
                    s.path.end()
                };
                self.node(BAD_NODE, pos, end)
            }
            Spec::ValueSpec(s) => {
                let pos = s.names.first().map(|n| n.pos()).unwrap_or_default();
                let end = if let Some(last) = s.values.last() {
                    last.end()
                } else if let Some(ty) = &s.ty {
                    ty.end()
                } else {
                    s.names.last().map(|n| n.end()).unwrap_or(pos)
                };
                let mut o = self.node(VALUE_SPEC, pos, end);
                for name in &s.names {
                    o.children.push(self.trans_ident(name));
                }
                if let Some(ty) = &s.ty {
                    o.children.push(self.trans_expr(ty));
                }
                for val in &s.values {
                    o.children.push(self.trans_expr(val));
                }
                o
            }
            Spec::TypeSpec(s) => {
                let mut o = self.node(TYPE_SPEC, s.name.pos(), s.ty.end());
                o.children.push(self.trans_ident(&s.name));
                if let Some(tp) = &s.type_params {
                    o.children.push(self.trans_field_list(tp));
                }
                o.children.push(self.trans_expr(&s.ty));
                o
            }
        }
    }

    fn trans_stmt(&self, stmt: &Stmt) -> SyntaxNode {
        match stmt {
            Stmt::BadStmt(s) => self.node(BAD_NODE, s.from, s.to),
            Stmt::DeclStmt(s) => {
                let mut o = self.node(DECL_STMT, s.decl.pos(), s.decl.end());
                o.children.push(self.trans_decl(&s.decl));
                o
            }
            Stmt::EmptyStmt(s) => {
                let end = if s.implicit {
                    s.semicolon
                } else {
                    Pos(s.semicolon.0 + 1)
                };
                self.node(EMPTY_STMT, s.semicolon, end)
            }
            Stmt::LabeledStmt(s) => {
                let mut o = self.node(LABELED_STMT, s.label.pos(), s.stmt.end());
                o.children.push(self.trans_ident(&s.label));
                o.children.push(self.trans_stmt(&s.stmt));
                o
            }
            Stmt::ExprStmt(s) => {
                let mut o = self.node(EXPR_STMT, s.x.pos(), s.x.end());
                o.children.push(self.trans_expr(&s.x));
                o
            }
            Stmt::SendStmt(s) => {
                let mut o = self.node(SEND_STMT, s.chan_.pos(), s.value.end());
                o.children.push(self.trans_expr(&s.chan_));
                o.children.push(self.trans_expr(&s.value));
                o
            }
            Stmt::IncDecStmt(s) => {
                let mut o = self.node(INCDEC_STMT, s.x.pos(), Pos(s.tok_pos.0 + 2));
                o.children.push(self.trans_expr(&s.x));
                o
            }
            Stmt::AssignStmt(s) => {
                let pos = s.lhs.first().map(|e| e.pos()).unwrap_or(s.tok_pos);
                let end = s.rhs.last().map(|e| e.end()).unwrap_or(pos);
                let mut o = self.node(ASSIGN_STMT, pos, end);
                for e in &s.rhs {
                    o.children.push(self.trans_expr(e));
                }
                for e in &s.lhs {
                    o.children.push(self.trans_expr(e));
                }
                o
            }
            Stmt::GoStmt(s) => {
                let mut o = self.node(GO_STMT, s.go_, s.call.end());
                o.children.push(self.trans_call(&s.call));
                o
            }
            Stmt::DeferStmt(s) => {
                let mut o = self.node(DEFER_STMT, s.defer_, s.call.end());
                o.children.push(self.trans_call(&s.call));
                o
            }
            Stmt::ReturnStmt(s) => {
                let end = s
                    .results
                    .last()
                    .map(|e| e.end())
                    .unwrap_or(Pos(s.return_.0 + 6));
                let mut o = self.node(RETURN_STMT, s.return_, end);
                for e in &s.results {
                    o.children.push(self.trans_expr(e));
                }
                o
            }
            Stmt::BranchStmt(s) => {
                let end = if let Some(label) = &s.label {
                    label.end()
                } else {
                    let len = s.tok.as_str().len() as i64;
                    Pos(s.tok_pos.0 + len)
                };
                let mut o = self.node(BRANCH_STMT, s.tok_pos, end);
                if let Some(label) = &s.label {
                    o.children.push(self.trans_ident(label));
                }
                o
            }
            Stmt::BlockStmt(s) => self.trans_block(s),
            Stmt::IfStmt(s) => {
                let end = s
                    .else_
                    .as_ref()
                    .map(|e| e.end())
                    .unwrap_or_else(|| s.body.end());
                let mut o = self.node(IF_STMT, s.if_, end);
                if let Some(init) = &s.init {
                    o.children.push(self.trans_stmt(init));
                }
                o.children.push(self.trans_expr(&s.cond));
                o.children.push(self.trans_block(&s.body));
                if let Some(else_) = &s.else_ {
                    o.children.push(self.trans_stmt(else_));
                }
                o
            }
            Stmt::CaseClause(s) => {
                let end = s
                    .body
                    .last()
                    .map(|b| b.end())
                    .unwrap_or_else(|| Pos(s.colon.0 + 1));
                let mut o = self.node(CASE_CLAUSE, s.case, end);
                for e in &s.list {
                    o.children.push(self.trans_expr(e));
                }
                for stmt in &s.body {
                    o.children.push(self.trans_stmt(stmt));
                }
                o
            }
            Stmt::SwitchStmt(s) => {
                let mut o = self.node(SWITCH_STMT, s.switch, s.body.end());
                if let Some(init) = &s.init {
                    o.children.push(self.trans_stmt(init));
                }
                if let Some(tag) = &s.tag {
                    o.children.push(self.trans_expr(tag));
                }
                o.children.push(self.trans_block(&s.body));
                o
            }
            Stmt::TypeSwitchStmt(s) => {
                let mut o = self.node(TYPE_SWITCH_STMT, s.switch, s.body.end());
                if let Some(init) = &s.init {
                    o.children.push(self.trans_stmt(init));
                }
                o.children.push(self.trans_stmt(&s.assign));
                o.children.push(self.trans_block(&s.body));
                o
            }
            Stmt::CommClause(s) => {
                let end = s
                    .body
                    .last()
                    .map(|b| b.end())
                    .unwrap_or_else(|| Pos(s.colon.0 + 1));
                let mut o = self.node(COMM_CLAUSE, s.case, end);
                if let Some(comm) = &s.comm {
                    o.children.push(self.trans_stmt(comm));
                }
                for stmt in &s.body {
                    o.children.push(self.trans_stmt(stmt));
                }
                o
            }
            Stmt::SelectStmt(s) => {
                let mut o = self.node(SELECT_STMT, s.select_, s.body.end());
                o.children.push(self.trans_block(&s.body));
                o
            }
            Stmt::ForStmt(s) => {
                let mut o = self.node(FOR_STMT, s.for_, s.body.end());
                if let Some(init) = &s.init {
                    o.children.push(self.trans_stmt(init));
                }
                if let Some(cond) = &s.cond {
                    o.children.push(self.trans_expr(cond));
                }
                if let Some(post) = &s.post {
                    o.children.push(self.trans_stmt(post));
                }
                o.children.push(self.trans_block(&s.body));
                o
            }
            Stmt::RangeStmt(s) => {
                let mut o = self.node(RANGE_STMT, s.for_, s.body.end());
                if let Some(key) = &s.key {
                    o.children.push(self.trans_expr(key));
                }
                if let Some(value) = &s.value {
                    o.children.push(self.trans_expr(value));
                }
                o.children.push(self.trans_expr(&s.x));
                o.children.push(self.trans_block(&s.body));
                o
            }
        }
    }

    fn trans_block(&self, b: &guff::ast::BlockStmt) -> SyntaxNode {
        let mut o = self.node(BLOCK_STMT, b.lbrace, b.end());
        for stmt in &b.list {
            o.children.push(self.trans_stmt(stmt));
        }
        o
    }

    fn trans_call(&self, c: &guff::ast::CallExpr) -> SyntaxNode {
        let mut o = self.node(CALL_EXPR, c.fun.pos(), c.end());
        o.children.push(self.trans_expr(&c.fun));
        for arg in &c.args {
            o.children.push(self.trans_expr(arg));
        }
        o
    }

    fn trans_ident(&self, id: &guff::ast::Ident) -> SyntaxNode {
        self.node(IDENT, id.pos(), id.end())
    }

    fn trans_field_list(&self, fl: &guff::ast::FieldList) -> SyntaxNode {
        let mut o = self.node(FIELD_LIST, fl.pos(), fl.end());
        for field in &fl.list {
            o.children.push(self.trans_field(field));
        }
        o
    }

    fn trans_field(&self, f: &guff::ast::Field) -> SyntaxNode {
        let mut o = self.node(FIELD, f.pos(), f.end());
        for name in &f.names {
            o.children.push(self.trans_ident(name));
        }
        if let Some(ty) = &f.ty {
            o.children.push(self.trans_expr(ty));
        }
        o
    }

    fn trans_func_type(&self, ft: &guff::ast::FuncType) -> SyntaxNode {
        let mut o = self.node(FUNC_TYPE, ft.pos(), ft.end());
        if let Some(tp) = &ft.type_params {
            o.children.push(self.trans_field_list(tp));
        }
        if let Some(params) = &ft.params {
            o.children.push(self.trans_field_list(params));
        }
        if let Some(results) = &ft.results {
            o.children.push(self.trans_field_list(results));
        }
        o
    }

    fn trans_expr(&self, expr: &Expr) -> SyntaxNode {
        match expr {
            Expr::BadExpr(e) => self.node(BAD_NODE, e.from, e.to),
            Expr::Ident(e) => self.trans_ident(e),
            Expr::BasicLit(e) => self.node(BASIC_LIT, e.value_pos, e.end()),
            Expr::BinaryExpr(e) => {
                let mut o = self.node(BINARY_EXPR, e.x.pos(), e.y.end());
                o.children.push(self.trans_expr(&e.x));
                o.children.push(self.trans_expr(&e.y));
                o
            }
            Expr::UnaryExpr(e) => {
                let mut o = self.node(UNARY_EXPR, e.op_pos, e.x.end());
                o.children.push(self.trans_expr(&e.x));
                o
            }
            Expr::CallExpr(e) => self.trans_call(e),
            Expr::ParenExpr(e) => {
                let mut o = self.node(PAREN_EXPR, e.lparen, Pos(e.rparen.0 + 1));
                o.children.push(self.trans_expr(&e.x));
                o
            }
            Expr::SelectorExpr(e) => {
                let mut o = self.node(SELECTOR_EXPR, e.x.pos(), e.sel.end());
                o.children.push(self.trans_expr(&e.x));
                o.children.push(self.trans_ident(&e.sel));
                o
            }
            Expr::IndexExpr(e) => {
                let mut o = self.node(INDEX_EXPR, e.x.pos(), Pos(e.rbrack.0 + 1));
                o.children.push(self.trans_expr(&e.x));
                o.children.push(self.trans_expr(&e.index));
                o
            }
            Expr::IndexListExpr(e) => {
                let mut o = self.node(INDEX_LIST_EXPR, e.x.pos(), Pos(e.rbrack.0 + 1));
                o.children.push(self.trans_expr(&e.x));
                for idx in &e.indices {
                    o.children.push(self.trans_expr(idx));
                }
                o
            }
            Expr::SliceExpr(e) => {
                let mut o = self.node(SLICE_EXPR, e.x.pos(), Pos(e.rbrack.0 + 1));
                o.children.push(self.trans_expr(&e.x));
                if let Some(low) = &e.low {
                    o.children.push(self.trans_expr(low));
                }
                if let Some(high) = &e.high {
                    o.children.push(self.trans_expr(high));
                }
                if let Some(max) = &e.max {
                    o.children.push(self.trans_expr(max));
                }
                o
            }
            Expr::TypeAssertExpr(e) => {
                let mut o = self.node(TYPE_ASSERT_EXPR, e.x.pos(), Pos(e.rparen.0 + 1));
                o.children.push(self.trans_expr(&e.x));
                if let Some(ty) = &e.ty {
                    o.children.push(self.trans_expr(ty));
                }
                o
            }
            Expr::StarExpr(e) => {
                let mut o = self.node(STAR_EXPR, e.star, e.x.end());
                o.children.push(self.trans_expr(&e.x));
                o
            }
            Expr::KeyValueExpr(e) => {
                let mut o = self.node(KEY_VALUE_EXPR, e.key.pos(), e.value.end());
                o.children.push(self.trans_expr(&e.key));
                o.children.push(self.trans_expr(&e.value));
                o
            }
            Expr::CompositeLit(e) => {
                let pos = e.ty.as_ref().map(|t| t.pos()).unwrap_or(e.lbrace);
                let mut o = self.node(COMPOSITE_LIT, pos, Pos(e.rbrace.0 + 1));
                if let Some(ty) = &e.ty {
                    o.children.push(self.trans_expr(ty));
                }
                for elt in &e.elts {
                    o.children.push(self.trans_expr(elt));
                }
                o
            }
            Expr::FuncLit(e) => {
                let mut o = self.node(FUNC_LIT, e.ty.pos(), e.body.end());
                o.children.push(self.trans_func_type(&e.ty));
                o.children.push(self.trans_block(&e.body));
                o
            }
            Expr::Ellipsis(e) => {
                let end = e
                    .elt
                    .as_ref()
                    .map(|x| x.end())
                    .unwrap_or(Pos(e.ellipsis.0 + 3));
                let mut o = self.node(ELLIPSIS, e.ellipsis, end);
                if let Some(elt) = &e.elt {
                    o.children.push(self.trans_expr(elt));
                }
                o
            }
            Expr::ArrayType(e) => {
                let mut o = self.node(ARRAY_TYPE, e.lbrack, e.elt.end());
                if let Some(len) = &e.len {
                    o.children.push(self.trans_expr(len));
                }
                o.children.push(self.trans_expr(&e.elt));
                o
            }
            Expr::StructType(e) => {
                let mut o = self.node(STRUCT_TYPE, e.struct_, e.fields.end());
                o.children.push(self.trans_field_list(&e.fields));
                o
            }
            Expr::FuncType(e) => self.trans_func_type(e),
            Expr::InterfaceType(e) => {
                let mut o = self.node(INTERFACE_TYPE, e.interface_, e.methods.end());
                o.children.push(self.trans_field_list(&e.methods));
                o
            }
            Expr::MapType(e) => {
                let mut o = self.node(MAP_TYPE, e.map_, e.value.end());
                o.children.push(self.trans_expr(&e.key));
                o.children.push(self.trans_expr(&e.value));
                o
            }
            Expr::ChanType(e) => {
                let mut o = self.node(CHAN_TYPE, e.begin, e.value.end());
                o.children.push(self.trans_expr(&e.value));
                o
            }
        }
    }
}

/// Transform a parsed Go file into a uniform syntax tree for clone detection.
pub fn transform_file(fset: &FileSet, filename: &str, file: &File) -> SyntaxNode {
    let t = Transformer {
        fset,
        filename: filename.to_string(),
    };
    t.trans_file(file)
}
