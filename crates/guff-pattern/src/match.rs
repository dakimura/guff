//! Pattern matching against guff AST nodes.
//!
//! Port of `honnef.co/go/tools/pattern/match.go`.

use std::collections::HashMap;

use guff::ast::{
    self, BadExpr, BadStmt, BlockStmt, Expr, Ident as AstIdent, Stmt,
};
use guff::token::Token;
use guff::walk::NodeRef;
use guff_constant::{int64_val, Kind};
use guff_types::arena::{ObjectArena, ObjectData, PackageArena, TypeArena};
use guff_types::signature::signature_recv;
use guff_types::{Info, ObjectId};

use crate::parser::token_from_str;
use crate::pattern::{
    AssignStmt as PAssignStmt, BasicLit as PBasicLit, BinaryExpr as PBinaryExpr, Binding, Builtin, CallExpr as PCallExpr,
    CaseClause as PCaseClause, CommClause as PCommClause, CompositeLit as PCompositeLit,
    DeferStmt as PDeferStmt, ForStmt as PForStmt, GoStmt as PGoStmt, Ident as PIdent,
    IfStmt as PIfStmt, IncDecStmt as PIncDecStmt, IndexExpr as PIndexExpr,
    IndexListExpr as PIndexListExpr, IntegerLiteral, List, Node, Not, Object as PObject, Or,
    RangeStmt as PRangeStmt, ReturnStmt as PReturnStmt, SelectStmt as PSelectStmt,
    SelectorExpr as PSelectorExpr, SendStmt as PSendStmt, SliceExpr as PSliceExpr,
    StarExpr as PStarExpr, StructType as _, SwitchStmt as PSwitchStmt, Symbol,
    TrulyConstantExpression, TypeAssertExpr as PTypeAssertExpr, UnaryExpr as PUnaryExpr,
    Pattern,
};

/// Environment for type-aware pattern matching.
pub struct MatchEnv<'a> {
    pub types: Option<&'a Info>,
    pub type_arena: Option<&'a TypeArena>,
    pub objects: Option<&'a ObjectArena>,
    pub packages: Option<&'a PackageArena>,
}

/// Binding values produced by a successful match.
#[derive(Debug, Clone)]
pub enum MatchValue<'a> {
    Node(NodeRef<'a>),
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Ident(&'a AstIdent),
    BasicLit(&'a ast::BasicLit),
    CallExpr(&'a ast::CallExpr),
    Token(Token),
    Object(ObjectId),
    Exprs(Vec<&'a Expr>),
    Stmts(Vec<&'a Stmt>),
    String(String),
}

impl<'a> MatchValue<'a> {
    pub fn as_expr(&self) -> Option<&'a Expr> {
        match self {
            Self::Expr(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_ident(&self) -> Option<&'a AstIdent> {
        match self {
            Self::Ident(i) => Some(i),
            Self::Node(NodeRef::Ident(i)) => Some(i),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<ObjectId> {
        match self {
            Self::Object(o) => Some(*o),
            _ => None,
        }
    }

    pub fn as_token(&self) -> Option<Token> {
        match self {
            Self::Token(t) => Some(*t),
            _ => None,
        }
    }

    pub fn as_exprs(&self) -> Option<Vec<&'a Expr>> {
        match self {
            Self::Exprs(v) => Some(v.clone()),
            Self::Expr(e) => Some(vec![e]),
            _ => None,
        }
    }
}

/// Matcher state after a successful pattern match.
/// Matcher state after a successful pattern match.
pub struct Matcher<'a> {
    pub state: HashMap<String, MatchValue<'a>>,
    env: MatchEnv<'a>,
    bindings_mapping: Vec<String>,
    set_bindings: Vec<u64>,
}

impl<'a> Matcher<'a> {
    pub fn new(env: MatchEnv<'a>) -> Self {
        Self {
            state: HashMap::new(),
            env,
            bindings_mapping: Vec::new(),
            set_bindings: Vec::new(),
        }
    }

    pub fn match_pattern(&mut self, pat: &Pattern, node: NodeRef<'a>) -> bool {
        self.bindings_mapping = pat.bindings.clone();
        self.state.clear();
        self.push();
        let ok = self.match_node(&pat.root, node).is_some();
        self.merge();
        ok
    }

    fn push(&mut self) {
        self.set_bindings.push(0);
    }

    fn pop(&mut self) {
        let set = self.set_bindings.pop().unwrap_or(0);
        if set != 0 {
            for (i, name) in self.bindings_mapping.iter().enumerate() {
                if (set & (1u64 << i)) != 0 {
                    self.state.remove(name);
                }
            }
        }
    }

    fn merge(&mut self) {
        self.set_bindings.pop();
    }

    fn set_binding(&mut self, idx: usize, name: &str, value: MatchValue<'a>) {
        self.state.insert(name.to_string(), value);
        if let Some(top) = self.set_bindings.last_mut() {
            *top |= 1u64 << idx;
        }
    }

    fn match_node(&mut self, pat: &Node, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        // Unwrap wrappers on AST side
        let node = unwrap_node_ref(node);
        self.match_node_inner(pat, node)
    }

    fn match_node_inner(&mut self, pat: &Node, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        match pat {
            Node::Any => Some(MatchValue::Node(node)),
            Node::Nil => {
                if is_nil_node(node) {
                    Some(MatchValue::Node(node))
                } else {
                    None
                }
            }
            Node::String(s) => self.match_string(s, node),
            Node::Tok(t) => self.match_token(t.0, node),
            Node::Binding(b) => self.match_binding(b, node),
            Node::List(l) => self.match_list(l, node),
            Node::Or(or) => self.match_or(or, node),
            Node::Not(n) => {
                if self.match_node(&n.node, node).is_some() {
                    None
                } else {
                    Some(MatchValue::Node(node))
                }
            }
            Node::Builtin(b) => self.match_builtin(b, node),
            Node::Object(o) => self.match_object(o, node),
            Node::Symbol(s) => self.match_symbol(s, node),
            Node::IntegerLiteral(l) => self.match_integer_literal(l, node),
            Node::TrulyConstantExpression(t) => self.match_truly_constant(t, node),
            Node::RangeStmt(p) => self.match_range_stmt(p, node),
            Node::AssignStmt(p) => self.match_assign_stmt(p, node),
            Node::IndexExpr(p) => self.match_index_expr(p, node),
            Node::IndexListExpr(p) => self.match_index_list_expr(p, node),
            Node::Ident(p) => self.match_ident_pat(p, node),
            Node::BinaryExpr(p) => self.match_binary_expr(p, node),
            Node::ForStmt(p) => self.match_for_stmt(p, node),
            Node::CallExpr(p) => self.match_call_expr(p, node),
            Node::SliceExpr(p) => self.match_slice_expr(p, node),
            Node::UnaryExpr(p) => self.match_unary_expr(p, node),
            Node::IfStmt(p) => self.match_if_stmt(p, node),
            Node::ReturnStmt(p) => self.match_return_stmt(p, node),
            Node::IncDecStmt(p) => self.match_inc_dec_stmt(p, node),
            Node::BasicLit(p) => self.match_basic_lit_pat(p, node),
            Node::SelectorExpr(p) => self.match_selector_expr(p, node),
            Node::SelectStmt(p) => self.match_select_stmt(p, node),
            Node::CommClause(p) => self.match_comm_clause(p, node),
            Node::StarExpr(p) => self.match_star_expr(p, node),
            Node::CompositeLit(p) => self.match_composite_lit(p, node),
            Node::TypeAssertExpr(p) => self.match_type_assert_expr(p, node),
            Node::DeferStmt(p) => self.match_defer_stmt(p, node),
            Node::GoStmt(p) => self.match_go_stmt(p, node),
            Node::SendStmt(p) => self.match_send_stmt(p, node),
            Node::SwitchStmt(p) => self.match_switch_stmt(p, node),
            Node::CaseClause(p) => self.match_case_clause(p, node),
            _ => self.match_struct_fields(pat, node),
        }
    }

    fn match_struct_fields(&mut self, _pat: &Node, _node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        None
    }

    fn match_string(&mut self, s: &str, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        if let Some(tok) = token_from_str(s) {
            return self.match_token(tok, node);
        }
        match node {
            NodeRef::Ident(id) if id.name == s => Some(MatchValue::Ident(id)),
            NodeRef::BasicLit(lit) if lit.value == s || unquote(&lit.value) == s => {
                Some(MatchValue::BasicLit(lit))
            }
            _ => None,
        }
    }

    fn match_token(&mut self, tok: Token, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        match node {
            NodeRef::AssignStmt(stmt) if stmt.tok == Some(tok) => Some(MatchValue::Token(tok)),
            NodeRef::RangeStmt(rs) if rs.tok == Some(tok) => Some(MatchValue::Token(tok)),
            NodeRef::UnaryExpr(u) if u.op == tok => Some(MatchValue::Token(tok)),
            NodeRef::BinaryExpr(b) if b.op == tok => Some(MatchValue::Token(tok)),
            NodeRef::IncDecStmt(i) if i.tok == tok => Some(MatchValue::Token(tok)),
            NodeRef::BranchStmt(b) if b.tok == tok => Some(MatchValue::Token(tok)),
            NodeRef::BasicLit(lit) if lit.kind == Some(tok) => Some(MatchValue::Token(tok)),
            _ => None,
        }
    }

    fn match_binding(&mut self, b: &Binding, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        if let Some(v) = self.state.get(&b.name) {
            let v = v.clone();
            return self.match_value(&b.node.as_deref().unwrap_or(&Node::Any), v, node);
        }
        let pat = b.node.as_deref().unwrap_or(&Node::Any);
        let ret = self.match_node_inner(pat, node)?;
        self.set_binding(b.idx, &b.name, ret.clone());
        Some(ret)
    }

    fn match_value(
        &mut self,
        pat: &Node,
        lhs: MatchValue<'a>,
        rhs: NodeRef<'a>,
    ) -> Option<MatchValue<'a>> {
        match lhs {
            MatchValue::Node(n) => self.match_node_inner(pat, n),
            MatchValue::Expr(e) => self.match_expr_pat(pat, e),
            MatchValue::Ident(i) => self
                .match_ident_node(pat, i)
                .map(|_| MatchValue::Ident(i)),
            MatchValue::Token(t) => self.match_token(t, rhs),
            MatchValue::Object(o) => self.match_object_id(pat, o, rhs),
            _ => None,
        }
    }

    fn match_list(&mut self, l: &List, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let (items, kind) = node_as_list(node)?;
        if l.head.is_none() {
            return if items.is_empty() {
                Some(MatchValue::Node(node))
            } else {
                None
            };
        }
        if items.is_empty() {
            return None;
        }
        let head_pat = l.head.as_deref().unwrap();
        let first = items[0];
        self.match_node_inner(head_pat, first)?;
        if let Some(tail) = &l.tail {
            self.match_slice_pat(tail, &items[1..])?;
        } else if items.len() > 1 {
            return None;
        }
        Some(MatchValue::Node(node))
    }

    fn match_or(&mut self, or: &Or, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        for opt in &or.nodes {
            self.push();
            if let Some(ret) = self.match_node_inner(opt, node) {
                self.merge();
                return Some(ret);
            }
            self.pop();
        }
        None
    }

    fn match_builtin(&mut self, b: &Builtin, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::Ident(id) = node else {
            return None;
        };
        let want = builtin_name(b.name.as_ref())?;
        if !is_universe_builtin(&self.env, id, want) {
            return None;
        }
        Some(MatchValue::Ident(id))
    }

    fn match_object(&mut self, o: &PObject, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        // Upstream's `Object.Match` delegates to `Ident`, so **only** a bare
        // identifier binds — `r.buf` does not. Note the asymmetry with
        // [`Self::match_object_id`]: recalling an already-bound object *does*
        // accept a selector, comparing the object of its `Sel`
        // (`pattern/match.go`, the `types.Object` arm of `match`). Accepting a
        // selector here too made `(SliceExpr x@(Object _) low (CallExpr
        // (Builtin "len") [x]) nil)` fire on `r.buf[i:len(r.buf)]`, which
        // upstream S1010 leaves alone (prometheus `tsdb/wlog/live_reader.go`).
        let id = match node {
            NodeRef::Ident(i) => i,
            _ => return None,
        };
        // The node must denote an object (Go's `Object` pattern).
        let obj = object_of(&self.env, id)?;
        // Match the name sub-pattern. `_` (Any) accepts any object; a string
        // requires an exact name match. Without the Any arm, `(Object _)`
        // never matches.
        match o.name.as_ref() {
            Node::Any => Some(MatchValue::Object(obj)),
            _ => {
                let want = object_name(o.name.as_ref())?;
                if id.name != want {
                    return None;
                }
                Some(MatchValue::Object(obj))
            }
        }
    }

    fn match_symbol(&mut self, s: &Symbol, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let name = symbol_name_from_node(&self.env, node)?;
        if !symbol_pattern_matches(s.name.as_ref(), &name) {
            return None;
        }
        Some(MatchValue::Node(node))
    }

    fn match_integer_literal(&mut self, lit: &IntegerLiteral, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        // The node must be a constant integer expression.
        let val = integer_literal_value(&self.env, node)?;
        // Match the value sub-pattern. `_` (Any) accepts any integer literal;
        // a string requires an exact value match. Without the Any arm,
        // `(IntegerLiteral _)` never matches.
        match lit.value.as_ref() {
            Node::Any => Some(MatchValue::Node(node)),
            Node::String(s) => {
                if val.to_string() == *s {
                    Some(MatchValue::Node(node))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn match_truly_constant(&mut self, t: &TrulyConstantExpression, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        // Port of `TrulyConstantExpression.Match`: constant value, no Idents
        // in the expression tree, then match `Value` against the TypeAndValue
        // (wildcard `_` / `Any` accepts any constant).
        let id = node_expr_id(node)?;
        let info = self.env.types?;
        let tav = info.types.get(&id)?;
        if tav.val.is_none() {
            return None;
        }
        if expr_contains_ident(node) {
            return None;
        }
        match t.value.as_ref() {
            Node::Any => Some(MatchValue::Node(node)),
            Node::String(want) => {
                let val = tav.val.as_ref()?;
                if val.kind() != Kind::String {
                    return None;
                }
                // Byte comparison: the pattern's literal is Rust text, but the
                // constant is a Go byte string and may not be valid UTF-8.
                if guff_constant::string_val(val) != want.as_bytes() {
                    return None;
                }
                Some(MatchValue::Node(node))
            }
            // DEFERRED: match IntegerLiteral / other Value patterns against TypeAndValue
            // the way upstream `match(m, texpr.Value, tv)` does. Wildcard covers ST1017.
            _ => None,
        }
    }

    fn match_range_stmt(&mut self, p: &PRangeStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::RangeStmt(rs) = node else {
            return None;
        };
        self.match_optional_expr(&p.key, rs.key.as_ref())?;
        self.match_optional_expr(&p.value, rs.value.as_ref())?;
        self.match_token_node(&p.tok, rs.tok)?;
        self.match_expr_node(&p.x, &rs.x)?;
        self.match_block_or_list(&p.body, &rs.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_assign_stmt(&mut self, p: &PAssignStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::AssignStmt(stmt) = node else {
            return None;
        };
        self.match_expr_list(&p.lhs, &stmt.lhs)?;
        self.match_token_node(&p.tok, stmt.tok)?;
        self.match_expr_list(&p.rhs, &stmt.rhs)?;
        Some(MatchValue::Node(node))
    }

    fn match_index_expr(&mut self, p: &PIndexExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::IndexExpr(ix) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &ix.x)?;
        self.match_expr_node(&p.index, &ix.index)?;
        Some(MatchValue::Node(node))
    }

    fn match_index_list_expr(&mut self, p: &PIndexListExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::IndexListExpr(ix) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &ix.x)?;
        self.match_expr_list(&p.indices, &ix.indices)?;
        Some(MatchValue::Node(node))
    }

    fn match_ident_pat(&mut self, p: &PIdent, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::Ident(id) = node else {
            return None;
        };
        self.match_node_inner(&p.name, node)?;
        Some(MatchValue::Ident(id))
    }

    fn match_binary_expr(&mut self, p: &PBinaryExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::BinaryExpr(b) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &b.x)?;
        self.match_token_node(&p.op, Some(b.op))?;
        self.match_expr_node(&p.y, &b.y)?;
        Some(MatchValue::Node(node))
    }

    fn match_for_stmt(&mut self, p: &PForStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::ForStmt(fs) = node else {
            return None;
        };
        self.match_optional_stmt(&p.init, fs.init.as_deref())?;
        self.match_optional_expr(&p.cond, fs.cond.as_ref())?;
        self.match_optional_stmt(&p.post, fs.post.as_deref())?;
        self.match_block_or_list(&p.body, &fs.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_call_expr(&mut self, p: &PCallExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::CallExpr(call) = node else {
            return None;
        };
        self.match_expr_node(&p.fun, &call.fun)?;
        self.match_expr_list(&p.args, &call.args)?;
        Some(MatchValue::Node(node))
    }

    fn match_slice_expr(&mut self, p: &PSliceExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::SliceExpr(se) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &se.x)?;
        self.match_optional_expr(&p.low, se.low.as_deref())?;
        self.match_optional_expr(&p.high, se.high.as_deref())?;
        self.match_optional_expr(&p.max, se.max.as_deref())?;
        Some(MatchValue::Node(node))
    }

    fn match_unary_expr(&mut self, p: &PUnaryExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::UnaryExpr(u) = node else {
            return None;
        };
        self.match_token_node(&p.op, Some(u.op))?;
        self.match_expr_node(&p.x, &u.x)?;
        Some(MatchValue::Node(node))
    }

    fn match_if_stmt(&mut self, p: &PIfStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::IfStmt(i) = node else {
            return None;
        };
        self.match_optional_stmt_box(&p.init, i.init.as_deref())?;
        self.match_expr_node(&p.cond, &i.cond)?;
        self.match_block_or_list(&p.body, &i.body)?;
        self.match_optional_else(&p.else_, i.else_.as_deref())?;
        Some(MatchValue::Node(node))
    }

    fn match_return_stmt(&mut self, p: &PReturnStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::ReturnStmt(r) = node else {
            return None;
        };
        self.match_expr_list(&p.results, &r.results)?;
        Some(MatchValue::Node(node))
    }

    fn match_inc_dec_stmt(&mut self, p: &PIncDecStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::IncDecStmt(i) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &i.x)?;
        self.match_token_node(&p.tok, Some(i.tok))?;
        Some(MatchValue::Node(node))
    }

    fn match_basic_lit_pat(&mut self, p: &PBasicLit, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::BasicLit(lit) = node else {
            return None;
        };
        if let Node::String(s) = p.kind.as_ref() {
            if let Some(tok) = token_from_str(s) {
                if lit.kind != Some(tok) {
                    return None;
                }
            }
        } else {
            self.match_node_inner(&p.kind, node)?;
        }
        if !matches!(&*p.value, Node::Any) {
            self.match_node_inner(&p.value, node)?;
        }
        Some(MatchValue::BasicLit(lit))
    }

    fn match_selector_expr(&mut self, p: &PSelectorExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::SelectorExpr(sel) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &sel.x)?;
        self.match_ident_node(&p.sel, &sel.sel)?;
        Some(MatchValue::Node(node))
    }

    fn match_select_stmt(&mut self, p: &PSelectStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::SelectStmt(s) = node else {
            return None;
        };
        self.match_block_or_list(&p.body, &s.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_comm_clause(&mut self, p: &PCommClause, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::CommClause(c) = node else {
            return None;
        };
        self.match_optional_stmt_box(&p.comm, c.comm.as_deref())?;
        self.match_stmt_list(&p.body, &c.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_star_expr(&mut self, p: &PStarExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::StarExpr(s) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &s.x)?;
        Some(MatchValue::Node(node))
    }

    fn match_composite_lit(&mut self, p: &PCompositeLit, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::CompositeLit(c) = node else {
            return None;
        };
        if !matches!(&*p.ty, Node::Any | Node::Nil) {
            if let Some(ty) = &c.ty {
                self.match_expr_node(&p.ty, ty)?;
            } else if !matches!(&*p.ty, Node::Nil) {
                return None;
            }
        }
        self.match_expr_list(&p.elts, &c.elts)?;
        Some(MatchValue::Node(node))
    }

    fn match_type_assert_expr(&mut self, p: &PTypeAssertExpr, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::TypeAssertExpr(t) = node else {
            return None;
        };
        self.match_expr_node(&p.x, &t.x)?;
        if !matches!(&*p.ty, Node::Any | Node::Nil) {
            if let Some(ty) = &t.ty {
                self.match_expr_node(&p.ty, ty)?;
            } else if !matches!(&*p.ty, Node::Nil) {
                return None;
            }
        }
        Some(MatchValue::Node(node))
    }

    fn match_defer_stmt(&mut self, p: &PDeferStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::DeferStmt(d) = node else {
            return None;
        };
        self.match_node_inner(&p.call, NodeRef::CallExpr(&d.call))?;
        Some(MatchValue::Node(node))
    }

    fn match_go_stmt(&mut self, p: &PGoStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::GoStmt(g) = node else {
            return None;
        };
        self.match_node_inner(&p.call, NodeRef::CallExpr(&g.call))?;
        Some(MatchValue::Node(node))
    }

    fn match_send_stmt(&mut self, p: &PSendStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::SendStmt(s) = node else {
            return None;
        };
        self.match_expr_node(&p.chan_, &s.chan_)?;
        self.match_expr_node(&p.value, &s.value)?;
        Some(MatchValue::Node(node))
    }

    fn match_switch_stmt(&mut self, p: &PSwitchStmt, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::SwitchStmt(s) = node else {
            return None;
        };
        self.match_optional_stmt_box(&p.init, s.init.as_deref())?;
        self.match_optional_expr(&p.tag, s.tag.as_ref())?;
        self.match_block_or_list(&p.body, &s.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_case_clause(&mut self, p: &PCaseClause, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let NodeRef::CaseClause(c) = node else {
            return None;
        };
        self.match_expr_list(&p.list, &c.list)?;
        self.match_stmt_list(&p.body, &c.body)?;
        Some(MatchValue::Node(node))
    }

    fn match_expr_node(&mut self, pat: &Node, expr: &'a Expr) -> Option<()> {
        self.match_node_inner(pat, expr_node_ref(expr)?)?;
        Some(())
    }

    fn match_ident_node(&mut self, pat: &Node, id: &'a AstIdent) -> Option<()> {
        self.match_node_inner(pat, NodeRef::Ident(id))?;
        Some(())
    }

    fn match_expr_pat(&mut self, pat: &Node, expr: &'a Expr) -> Option<MatchValue<'a>> {
        self.match_node_inner(pat, expr_node_ref(expr)?)
    }

    fn match_object_id(&mut self, pat: &Node, obj: ObjectId, node: NodeRef<'a>) -> Option<MatchValue<'a>> {
        let id = match node {
            NodeRef::Ident(i) => i,
            NodeRef::SelectorExpr(sel) => &sel.sel,
            _ => return None,
        };
        let got = object_of(&self.env, id)?;
        if got != obj {
            return None;
        }
        self.match_node_inner(pat, node)
    }

    fn match_optional_expr(&mut self, pat: &Node, expr: Option<&'a Expr>) -> Option<()> {
        match pat {
            Node::Nil => {
                if expr.is_some() {
                    None
                } else {
                    Some(())
                }
            }
            Node::Any => Some(()),
            _ => {
                let e = expr?;
                self.match_expr_node(pat, e)
            }
        }
    }

    fn match_optional_stmt(&mut self, pat: &Node, stmt: Option<&'a Stmt>) -> Option<()> {
        match pat {
            Node::Nil => {
                if stmt.is_some() {
                    None
                } else {
                    Some(())
                }
            }
            Node::Any => Some(()),
            _ => {
                let s = stmt?;
                self.match_node_inner(pat, stmt_node_ref(s)?)?;
                Some(())
            }
        }
    }

    fn match_optional_stmt_box(&mut self, pat: &Node, stmt: Option<&'a Stmt>) -> Option<()> {
        self.match_optional_stmt(pat, stmt)
    }

    fn match_optional_else(&mut self, pat: &Node, else_: Option<&'a Stmt>) -> Option<()> {
        match pat {
            Node::Nil => {
                if else_.is_some() {
                    None
                } else {
                    Some(())
                }
            }
            Node::Any => Some(()),
            _ => {
                let s = else_?;
                self.match_node_inner(pat, stmt_node_ref(s)?)?;
                Some(())
            }
        }
    }

    fn match_token_node(&mut self, pat: &Node, tok: Option<Token>) -> Option<()> {
        match pat {
            Node::Nil => {
                if tok.is_some() {
                    None
                } else {
                    Some(())
                }
            }
            Node::Any => Some(()),
            Node::String(s) => {
                let want = token_from_str(s)?;
                if tok == Some(want) {
                    Some(())
                } else {
                    None
                }
            }
            Node::Tok(t) => {
                if tok == Some(t.0) {
                    Some(())
                } else {
                    None
                }
            }
            Node::Or(or) => {
                for n in &or.nodes {
                    if self.match_token_node(n, tok).is_some() {
                        return Some(());
                    }
                }
                None
            }
            Node::Binding(b) => {
                let Some(tok_val) = tok else {
                    return None;
                };
                if let Some(v) = self.state.get(&b.name) {
                    let MatchValue::Token(bound) = v else {
                        return None;
                    };
                    return if *bound == tok_val { Some(()) } else { None };
                }
                let inner = b.node.as_deref().unwrap_or(&Node::Any);
                self.match_token_node(inner, tok)?;
                self.set_binding(b.idx, &b.name, MatchValue::Token(tok_val));
                Some(())
            }
            _ => None,
        }
    }

    fn match_slice_pat(&mut self, pat: &Node, items: &[NodeRef<'a>]) -> Option<()> {
        match pat {
            Node::List(l) => {
                if l.head.is_none() {
                    return if items.is_empty() { Some(()) } else { None };
                }
                if items.is_empty() {
                    return None;
                }
                self.match_node_inner(l.head.as_deref().unwrap(), items[0])?;
                if let Some(tail) = &l.tail {
                    self.match_slice_pat(tail, &items[1..])
                } else {
                    Some(())
                }
            }
            Node::Binding(b) => {
                if b.node.is_none() {
                    // Binding without constraint on list — store as exprs/stmts later in match_expr_list
                    return Some(());
                }
                if items.len() == 1 {
                    return self.match_node_inner(b.node.as_deref().unwrap(), items[0]).map(|_| ());
                }
                None
            }
            _ => {
                if items.len() == 1 {
                    self.match_node_inner(pat, items[0]).map(|_| ())
                } else {
                    None
                }
            }
        }
    }

    fn match_expr_list(&mut self, pat: &Node, exprs: &'a [Expr]) -> Option<()> {
        if let Node::Binding(b) = pat {
            if b.node.is_none() {
                self.set_binding(
                    b.idx,
                    &b.name,
                    MatchValue::Exprs(exprs.iter().collect()),
                );
                return Some(());
            }
        }
        let refs: Vec<NodeRef<'a>> = exprs.iter().filter_map(expr_node_ref).collect();
        self.match_slice_pat(pat, &refs)
    }

    fn match_stmt_list(&mut self, pat: &Node, stmts: &'a [Stmt]) -> Option<()> {
        let refs: Vec<NodeRef<'a>> = stmts.iter().filter_map(stmt_node_ref).collect();
        self.match_slice_pat(pat, &refs)
    }

    fn match_block_or_list(&mut self, pat: &Node, block: &'a BlockStmt) -> Option<()> {
        if matches!(pat, Node::List(_)) {
            let refs: Vec<NodeRef<'a>> = block.list.iter().filter_map(stmt_node_ref).collect();
            self.match_slice_pat(pat, &refs)
        } else {
            self.match_node_inner(pat, NodeRef::BlockStmt(block)).map(|_| ())
        }
    }
}

#[derive(Clone, Copy)]
enum ListKind {
    Expr,
    Stmt,
}

fn unwrap_node_ref<'a>(node: NodeRef<'a>) -> NodeRef<'a> {
    match node {
        NodeRef::ParenExpr(p) => unwrap_node_ref(expr_node_ref(&p.x).unwrap_or(node)),
        NodeRef::ExprStmt(e) => unwrap_node_ref(expr_node_ref(&e.x).unwrap_or(node)),
        NodeRef::DeclStmt(d) => match &d.decl {
            ast::Decl::GenDecl(g) => NodeRef::GenDecl(g),
            ast::Decl::FuncDecl(f) => NodeRef::FuncDecl(f),
            ast::Decl::BadDecl(b) => NodeRef::BadDecl(b),
        },
        NodeRef::LabeledStmt(l) => unwrap_node_ref(stmt_node_ref(&l.stmt).unwrap_or(node)),
        NodeRef::BlockStmt(b) if b.list.len() == 1 => {
            unwrap_node_ref(stmt_node_ref(&b.list[0]).unwrap_or(node))
        }
        other => other,
    }
}

fn is_nil_node(node: NodeRef<'_>) -> bool {
    matches!(
        node,
        NodeRef::Ident(i) if i.name.is_empty() || i.name == "<nil>"
    )
}

fn unquote(s: &str) -> String {
    if let Some(inner) = s.strip_prefix('`').and_then(|x| x.strip_suffix('`')) {
        return inner.to_string();
    }
    if let Some(inner) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return inner.to_string();
    }
    s.to_string()
}

fn integer_literal_value(env: &MatchEnv<'_>, node: NodeRef<'_>) -> Option<i64> {
    let id = node_expr_id(node)?;
    let info = env.types?;
    let tav = info.types.get(&id)?;
    let val = tav.val.as_ref()?;
    if val.kind() != Kind::Int {
        return None;
    }
    let (n, ok) = int64_val(val);
    ok.then_some(n)
}

fn node_expr_id(node: NodeRef<'_>) -> Option<u32> {
    match node {
        NodeRef::BasicLit(l) => Some(l.id),
        NodeRef::Ident(i) => Some(i.id),
        NodeRef::UnaryExpr(u) => Some(u.id),
        NodeRef::BinaryExpr(b) => Some(b.id),
        NodeRef::CallExpr(c) => Some(c.id),
        NodeRef::ParenExpr(p) => Some(p.id),
        NodeRef::IndexExpr(e) => Some(e.id),
        NodeRef::IndexListExpr(e) => Some(e.id),
        NodeRef::SliceExpr(e) => Some(e.id),
        NodeRef::CompositeLit(c) => Some(c.id),
        NodeRef::SelectorExpr(s) => Some(s.id),
        NodeRef::StarExpr(s) => Some(s.id),
        NodeRef::KeyValueExpr(k) => Some(k.id),
        _ => None,
    }
}

/// Reports whether `node`'s expression tree contains any `Ident`
/// (named constants / variables make an expression not "truly" constant).
fn expr_contains_ident(node: NodeRef<'_>) -> bool {
    let mut found = false;
    guff::walk::preorder(node, |n| {
        if matches!(n, NodeRef::Ident(_)) {
            found = true;
            false
        } else {
            true
        }
    });
    found
}

fn symbol_pattern_matches(pat: &Node, name: &str) -> bool {
    match pat {
        Node::String(s) => s == name,
        Node::Or(or) => or.nodes.iter().any(|n| symbol_pattern_matches(n, name)),
        Node::Any => true,
        _ => false,
    }
}

fn symbol_pattern_name(pat: &Node) -> Option<String> {
    match pat {
        Node::String(s) => Some(s.clone()),
        Node::Or(or) => {
            for n in &or.nodes {
                if let Node::String(s) = n {
                    return Some(s.clone());
                }
            }
            None
        }
        Node::Binding(b) => b
            .node
            .as_deref()
            .and_then(symbol_pattern_name)
            .or_else(|| Some(b.name.clone())),
        _ => None,
    }
}

fn symbol_name_from_node(env: &MatchEnv<'_>, node: NodeRef<'_>) -> Option<String> {
    match node {
        NodeRef::Ident(id) => {
            let obj = object_of(env, id)?;
            symbol_name_for_object(env, obj, None)
        }
        NodeRef::SelectorExpr(sel) => {
            let obj = object_of(env, &sel.sel)?;
            symbol_name_for_object(env, obj, Some(sel))
        }
        NodeRef::CallExpr(call) => symbol_name_from_node(env, expr_node_ref(&call.fun)?),
        NodeRef::IndexExpr(ix) => symbol_name_from_node(env, expr_node_ref(&ix.x)?),
        NodeRef::IndexListExpr(ix) => symbol_name_from_node(env, expr_node_ref(&ix.x)?),
        _ => None,
    }
}

fn symbol_name_for_object(
    env: &MatchEnv<'_>,
    obj_id: ObjectId,
    sel: Option<&ast::SelectorExpr>,
) -> Option<String> {
    let objects = env.objects?;
    let packages = env.packages?;
    let type_arena = env.type_arena?;

    match objects.get(obj_id) {
        ObjectData::Func(_) => {
            // A method (function with a receiver) is named `(RecvType).Method`.
            // A package-level function accessed via a selector (`pkg.Func`) also
            // arrives here with `sel = Some`, but has no receiver — in that case
            // fall through to the package-qualified name instead of bailing out.
            if let Some(sel) = sel {
                let sig = obj_id.typ(objects)?;
                if let Some(recv) = signature_recv(type_arena, sig) {
                    let recv_type = recv.typ(objects)?;
                    let recv_str = guff_types::typestring::type_string(
                        type_arena, objects, packages, recv_type, None,
                    );
                    return Some(format!("({recv_str}).{}", sel.sel.name));
                }
            }
            let name = obj_id.name(objects);
            match obj_id.pkg(objects) {
                Some(pkg) => {
                    let path = packages.get(pkg).path();
                    if path.is_empty() {
                        Some(name.to_string())
                    } else {
                        Some(format!("{path}.{name}"))
                    }
                }
                None => Some(name.to_string()),
            }
        }
        ObjectData::Builtin(b) => Some(b.name().to_string()),
        ObjectData::TypeName(_) => {
            let typ = obj_id.typ(objects)?;
            Some(guff_types::typestring::type_string(
                type_arena, objects, packages, typ, None,
            ))
        }
        _ => None,
    }
}

fn expr_node_ref<'a>(expr: &'a Expr) -> Option<NodeRef<'a>> {
    Some(match expr {
        Expr::Ident(i) => NodeRef::Ident(i),
        Expr::BasicLit(b) => NodeRef::BasicLit(b),
        Expr::BinaryExpr(b) => NodeRef::BinaryExpr(b),
        Expr::CallExpr(c) => NodeRef::CallExpr(c),
        Expr::SelectorExpr(s) => NodeRef::SelectorExpr(s),
        Expr::IndexExpr(i) => NodeRef::IndexExpr(i),
        Expr::SliceExpr(s) => NodeRef::SliceExpr(s),
        Expr::UnaryExpr(u) => NodeRef::UnaryExpr(u),
        Expr::StarExpr(s) => NodeRef::StarExpr(s),
        Expr::CompositeLit(c) => NodeRef::CompositeLit(c),
        Expr::TypeAssertExpr(t) => NodeRef::TypeAssertExpr(t),
        Expr::ParenExpr(p) => NodeRef::ParenExpr(p),
        Expr::FuncLit(f) => NodeRef::FuncLit(f),
        Expr::IndexListExpr(i) => NodeRef::IndexListExpr(i),
        Expr::KeyValueExpr(k) => NodeRef::KeyValueExpr(k),
        Expr::ArrayType(a) => NodeRef::ArrayType(a),
        Expr::StructType(s) => NodeRef::StructType(s),
        Expr::FuncType(f) => NodeRef::FuncType(f),
        Expr::InterfaceType(i) => NodeRef::InterfaceType(i),
        Expr::MapType(m) => NodeRef::MapType(m),
        Expr::ChanType(c) => NodeRef::ChanType(c),
        Expr::Ellipsis(e) => NodeRef::Ellipsis(e),
        Expr::BadExpr(b) => NodeRef::BadExpr(b),
    })
}

fn stmt_node_ref<'a>(stmt: &'a Stmt) -> Option<NodeRef<'a>> {
    Some(match stmt {
        Stmt::AssignStmt(s) => NodeRef::AssignStmt(s),
        Stmt::RangeStmt(s) => NodeRef::RangeStmt(s),
        Stmt::ForStmt(s) => NodeRef::ForStmt(s),
        Stmt::IfStmt(s) => NodeRef::IfStmt(s),
        Stmt::ReturnStmt(s) => NodeRef::ReturnStmt(s),
        Stmt::IncDecStmt(s) => NodeRef::IncDecStmt(s),
        Stmt::ExprStmt(s) => NodeRef::ExprStmt(s),
        Stmt::BlockStmt(s) => NodeRef::BlockStmt(s),
        Stmt::SwitchStmt(s) => NodeRef::SwitchStmt(s),
        Stmt::SelectStmt(s) => NodeRef::SelectStmt(s),
        Stmt::GoStmt(s) => NodeRef::GoStmt(s),
        Stmt::DeferStmt(s) => NodeRef::DeferStmt(s),
        Stmt::SendStmt(s) => NodeRef::SendStmt(s),
        Stmt::BranchStmt(s) => NodeRef::BranchStmt(s),
        Stmt::DeclStmt(s) => NodeRef::DeclStmt(s),
        Stmt::LabeledStmt(s) => NodeRef::LabeledStmt(s),
        Stmt::EmptyStmt(s) => NodeRef::EmptyStmt(s),
        Stmt::CaseClause(s) => NodeRef::CaseClause(s),
        Stmt::CommClause(s) => NodeRef::CommClause(s),
        Stmt::TypeSwitchStmt(s) => NodeRef::TypeSwitchStmt(s),
        Stmt::BadStmt(s) => NodeRef::BadStmt(s),
    })
}

fn node_as_list<'a>(node: NodeRef<'a>) -> Option<(Vec<NodeRef<'a>>, ListKind)> {
    match node {
        NodeRef::BlockStmt(b) => {
            let refs: Vec<_> = b.list.iter().filter_map(stmt_node_ref).collect();
            Some((refs, ListKind::Stmt))
        }
        NodeRef::AssignStmt(a) => {
            let refs: Vec<_> = a.lhs.iter().filter_map(expr_node_ref).collect();
            if !refs.is_empty() {
                return Some((refs, ListKind::Expr));
            }
            None
        }
        NodeRef::ReturnStmt(r) => {
            let refs: Vec<_> = r.results.iter().filter_map(expr_node_ref).collect();
            Some((refs, ListKind::Expr))
        }
        NodeRef::CallExpr(c) => {
            let refs: Vec<_> = c.args.iter().filter_map(expr_node_ref).collect();
            Some((refs, ListKind::Expr))
        }
        _ => None,
    }
}

fn node_pat_str(pat: &Node) -> Option<&str> {
    match pat {
        Node::String(s) => Some(s),
        _ => None,
    }
}

fn builtin_name(pat: &Node) -> Option<&str> {
    match pat {
        Node::String(s) => Some(s),
        Node::Or(or) => {
            for n in &or.nodes {
                if let Node::String(s) = n {
                    return Some(s);
                }
            }
            None
        }
        _ => None,
    }
}

fn object_name(pat: &Node) -> Option<&str> {
    match pat {
        Node::String(s) => Some(s),
        Node::Or(or) => {
            for n in &or.nodes {
                if let Node::String(s) = n {
                    return Some(s);
                }
            }
            None
        }
        Node::Any => None,
        _ => None,
    }
}

fn object_of(env: &MatchEnv<'_>, ident: &AstIdent) -> Option<ObjectId> {
    let info = env.types?;
    info.uses
        .get(&ident.id)
        .copied()
        .or_else(|| info.defs.get(&ident.id).and_then(|o| *o))
}

fn is_universe_builtin(env: &MatchEnv<'_>, ident: &AstIdent, name: &str) -> bool {
    if ident.name != name {
        return false;
    }
    let Some(info) = env.types else {
        return name == "true" || name == "false" || name == "nil";
    };
    let Some(obj) = info.uses.get(&ident.id).copied() else {
        return false;
    };
    let Some(objects) = env.objects else {
        return false;
    };
    matches!(objects.get(obj), ObjectData::Builtin(b) if b.name() == name)
}

pub fn match_node<'a>(
    env: MatchEnv<'a>,
    pat: &Pattern,
    node: NodeRef<'a>,
) -> Option<Matcher<'a>> {
    let mut m = Matcher::new(env);
    if m.match_pattern(pat, node) {
        Some(m)
    } else {
        None
    }
}
