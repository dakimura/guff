//! Port of [`github.com/alexkohler/prealloc`](https://github.com/alexkohler/prealloc)
//! v1.1.0 (golangci-lint wrapper in `pkg/golinters/prealloc`).
//!
//! Defaults match golangci-lint: `simple=true`, `range-loops=true`, `for-loops=false`.
//!
//! The visitor is a statement-order walk that mirrors upstream's `ast.Walk`
//! exactly, because the decision to report is carried in mutable per-declaration
//! state (`exclude` / `hasReturn` / `level` / `detached`) that is only sound if
//! nodes are visited in the same order. An earlier approximation that just
//! looked for `append` inside range bodies reported four grafana slices that
//! upstream skips: a slice appended one block deeper than its declaration, a
//! range over a channel, and two ranges over `iter.Seq2` functions.
//!
//! Capacity expressions are built as [`Cap`] rather than as AST nodes, and
//! rendered with go/printer's binary-operator spacing rules so the message text
//! matches golangci-lint's.
//!
//! DEFERRED: upstream's `forLoopCount` (trip count of a three-clause `for`).
//! `for-loops` is off by default and no corpus config turns it on; with it on,
//! guff reports those loops without a capacity instead of computing one. This
//! matches the behaviour of the analyzer this file replaced.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::OnceLock;

use guff::ast::{
    AssignStmt, BlockStmt, CallExpr, Decl, Expr, ForStmt, GenDecl, RangeStmt, Spec, Stmt,
};
use guff::token::Token;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use guff_types::arena::TypeData;
use guff_types::TypeId;

use crate::options::PreallocOptions;

// ---------------------------------------------------------------------------
// Capacity expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Add,
    Sub,
    Mul,
}

impl Op {
    fn prec(self) -> u8 {
        match self {
            Op::Add | Op::Sub => 4,
            Op::Mul => 5,
        }
    }

    fn text(self) -> &'static str {
        match self {
            Op::Add => "+",
            Op::Sub => "-",
            Op::Mul => "*",
        }
    }
}

/// An expression lifted verbatim out of the source.
#[derive(Debug)]
struct Leaf {
    /// Rendered Go source, used in the diagnostic message.
    text: String,
    /// Identifier names reachable per upstream's `hasVarReference` (which skips
    /// selector fields, callee names, and composite-literal keys).
    refs: HashSet<String>,
    /// Rendered text of every sub-expression, used for `hasAny` against the
    /// loop variables.
    subs: HashSet<String>,
    /// Constant value per upstream's `intValue`, if any.
    int: Option<i64>,
}

#[derive(Clone, Debug)]
enum Cap {
    Int(i64),
    Leaf(Rc<Leaf>),
    Bin(Rc<Cap>, Op, Rc<Cap>),
    Neg(Rc<Cap>),
    Len(Rc<Cap>),
}

impl Cap {
    fn int_value(&self) -> Option<i64> {
        match self {
            Cap::Int(n) => Some(*n),
            Cap::Leaf(l) => l.int,
            Cap::Neg(x) => x.int_value().map(|n| -n),
            _ => None,
        }
    }

    fn refs_name(&self, name: &str) -> bool {
        match self {
            Cap::Int(_) => false,
            Cap::Leaf(l) => l.refs.contains(name),
            Cap::Bin(x, _, y) => x.refs_name(name) || y.refs_name(name),
            Cap::Neg(x) | Cap::Len(x) => x.refs_name(name),
        }
    }

    fn mentions_any(&self, texts: &[String]) -> bool {
        match self {
            Cap::Int(_) => false,
            Cap::Leaf(l) => texts.iter().any(|t| l.subs.contains(t)),
            Cap::Bin(x, _, y) => x.mentions_any(texts) || y.mentions_any(texts),
            Cap::Neg(x) | Cap::Len(x) => x.mentions_any(texts),
        }
    }
}

fn cap_int(n: i64) -> Option<Cap> {
    Some(Cap::Int(n))
}

/// `addIntExpr`.
fn add_cap(x: Option<Cap>, y: Option<Cap>) -> Option<Cap> {
    let (x, y) = (x?, y?);
    let (xi, yi) = (x.int_value(), y.int_value());
    if let (Some(a), Some(b)) = (xi, yi) {
        return cap_int(a + b);
    }
    if let Some(a) = xi {
        if a == 0 {
            return Some(y);
        }
        if a < 0 {
            return Some(Cap::Bin(Rc::new(y), Op::Sub, Rc::new(Cap::Int(-a))));
        }
    }
    if let Some(b) = yi {
        if b == 0 {
            return Some(x);
        }
        if b < 0 {
            return Some(Cap::Bin(Rc::new(x), Op::Sub, Rc::new(Cap::Int(-b))));
        }
    }
    if let Cap::Neg(inner) = &y {
        return Some(Cap::Bin(Rc::new(x), Op::Sub, inner.clone()));
    }
    Some(Cap::Bin(Rc::new(x), Op::Add, Rc::new(y)))
}

/// `subIntExpr`.
fn sub_cap(x: Option<Cap>, y: Option<Cap>) -> Option<Cap> {
    let y = y?;
    if let Some(Cap::Bin(lhs, Op::Add, rhs)) = x.as_ref() {
        if cap_eq(rhs, &y) {
            return Some((**lhs).clone());
        }
        if cap_eq(lhs, &y) {
            return Some((**rhs).clone());
        }
    }
    let neg = match &y {
        Cap::Neg(inner) => (**inner).clone(),
        other => Cap::Neg(Rc::new(other.clone())),
    };
    add_cap(x, Some(neg))
}

/// `mulIntExpr`.
fn mul_cap(x: Option<Cap>, y: Option<Cap>) -> Option<Cap> {
    let (x, y) = (x?, y?);
    let (xi, yi) = (x.int_value(), y.int_value());
    if let (Some(a), Some(b)) = (xi, yi) {
        return cap_int(a * b);
    }
    if let Some(a) = xi {
        if a == 0 {
            return cap_int(0);
        }
        if a == 1 {
            return Some(y);
        }
    }
    if let Some(b) = yi {
        if b == 0 {
            return cap_int(0);
        }
        if b == 1 {
            return Some(x);
        }
    }
    Some(Cap::Bin(Rc::new(x), Op::Mul, Rc::new(y)))
}

/// Structural equality, standing in for upstream's `exprEqual`.
fn cap_eq(a: &Cap, b: &Cap) -> bool {
    match (a, b) {
        (Cap::Int(x), Cap::Int(y)) => x == y,
        (Cap::Leaf(x), Cap::Leaf(y)) => x.text == y.text,
        (Cap::Bin(ax, ao, ay), Cap::Bin(bx, bo, by)) => {
            ao == bo
                && ((cap_eq(ax, bx) && cap_eq(ay, by))
                    // ADD and MUL are commutative (upstream `binaryExprEqual`).
                    || (matches!(ao, Op::Add | Op::Mul)
                        && cap_eq(ax, by)
                        && cap_eq(ay, bx)))
        }
        (Cap::Neg(x), Cap::Neg(y)) | (Cap::Len(x), Cap::Len(y)) => cap_eq(x, y),
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Rendering (go/printer spacing rules)
// ---------------------------------------------------------------------------

/// `go/printer.walkBinary` — whether the tree contains precedence-4 and/or
/// precedence-5 operators below `cap`, stopping where parentheses are inserted.
fn walk_binary(x: &Cap, op: Op, y: &Cap, self_op: Op) -> (bool, bool) {
    let mut has4 = self_op.prec() == 4;
    let mut has5 = self_op.prec() == 5;
    let _ = op;
    if let Cap::Bin(lx, lo, ly) = x {
        if lo.prec() >= self_op.prec() {
            let (a, b) = walk_binary(lx, *lo, ly, *lo);
            has4 |= a;
            has5 |= b;
        }
    }
    if let Cap::Bin(rx, ro, ry) = y {
        if ro.prec() > self_op.prec() {
            let (a, b) = walk_binary(rx, *ro, ry, *ro);
            has4 |= a;
            has5 |= b;
        }
    }
    (has4, has5)
}

/// `go/printer.cutoff`.
fn cutoff(x: &Cap, op: Op, y: &Cap, depth: u32) -> u8 {
    let (has4, has5) = walk_binary(x, op, y, op);
    if has4 && has5 {
        return if depth == 1 { 5 } else { 4 };
    }
    if depth == 1 {
        return 6;
    }
    if has4 || has5 {
        return 5;
    }
    4
}

fn render(cap: &Cap) -> String {
    let mut out = String::new();
    render1(cap, 0, 1, &mut out);
    out
}

fn render1(cap: &Cap, prec1: u8, depth: u32, out: &mut String) {
    match cap {
        Cap::Int(n) => out.push_str(&n.to_string()),
        Cap::Leaf(l) => out.push_str(&l.text),
        Cap::Len(x) => {
            out.push_str("len(");
            render1(x, 0, 1, out);
            out.push(')');
        }
        Cap::Neg(x) => {
            out.push('-');
            // Unary operands print at precedence 6.
            render1(x, 6, 1, out);
        }
        Cap::Bin(x, op, y) => {
            let prec = op.prec();
            if prec < prec1 {
                out.push('(');
                render1(cap, 0, depth.saturating_sub(1).max(1), out);
                out.push(')');
                return;
            }
            let print_blank = prec < cutoff(x, *op, y, depth);
            // `diffPrec`: an operand of the same precedence keeps the depth.
            let ldepth = match x.as_ref() {
                Cap::Bin(_, lop, _) if lop.prec() == prec => depth,
                _ => depth + 1,
            };
            render1(x, prec, ldepth, out);
            if print_blank {
                out.push(' ');
            }
            out.push_str(op.text());
            if print_blank {
                out.push(' ');
            }
            render1(y, prec + 1, depth + 1, out);
        }
    }
}

// ---------------------------------------------------------------------------
// Source expression helpers
// ---------------------------------------------------------------------------

fn unparen(e: &Expr) -> &Expr {
    match e {
        Expr::ParenExpr(p) => unparen(&p.x),
        other => other,
    }
}

fn render_expr(e: &Expr) -> String {
    match e {
        Expr::Ident(id) => id.name.clone(),
        Expr::BasicLit(l) => l.value.clone(),
        Expr::ParenExpr(p) => format!("({})", render_expr(&p.x)),
        Expr::UnaryExpr(u) => format!("{}{}", token_text(u.op), render_expr(&u.x)),
        Expr::BinaryExpr(b) => format!(
            "{} {} {}",
            render_expr(&b.x),
            token_text(b.op),
            render_expr(&b.y)
        ),
        Expr::SelectorExpr(s) => format!("{}.{}", render_expr(&s.x), s.sel.name),
        Expr::StarExpr(s) => format!("*{}", render_expr(&s.x)),
        Expr::IndexExpr(i) => format!("{}[{}]", render_expr(&i.x), render_expr(&i.index)),
        Expr::SliceExpr(s) => {
            let low = s.low.as_ref().map(|e| render_expr(e)).unwrap_or_default();
            let high = s.high.as_ref().map(|e| render_expr(e)).unwrap_or_default();
            format!("{}[{low}:{high}]", render_expr(&s.x))
        }
        Expr::CallExpr(c) => {
            let mut s = render_expr(&c.fun);
            s.push('(');
            for (i, a) in c.args.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&render_expr(a));
            }
            if c.ellipsis.is_valid() {
                s.push_str("...");
            }
            s.push(')');
            s
        }
        Expr::ArrayType(a) => match &a.len {
            Some(l) => format!("[{}]{}", render_expr(l), render_expr(&a.elt)),
            None => format!("[]{}", render_expr(&a.elt)),
        },
        Expr::MapType(m) => format!("map[{}]{}", render_expr(&m.key), render_expr(&m.value)),
        Expr::CompositeLit(c) => {
            let ty = c.ty.as_ref().map(|t| render_expr(t)).unwrap_or_default();
            let elts: Vec<String> = c.elts.iter().map(render_expr).collect();
            format!("{ty}{{{}}}", elts.join(", "))
        }
        Expr::KeyValueExpr(kv) => format!("{}: {}", render_expr(&kv.key), render_expr(&kv.value)),
        Expr::TypeAssertExpr(t) => match &t.ty {
            Some(ty) => format!("{}.({})", render_expr(&t.x), render_expr(ty)),
            None => format!("{}.(type)", render_expr(&t.x)),
        },
        _ => String::new(),
    }
}

fn token_text(t: Token) -> &'static str {
    match t {
        Token::ADD => "+",
        Token::SUB => "-",
        Token::MUL => "*",
        Token::QUO => "/",
        Token::REM => "%",
        Token::AND => "&",
        Token::OR => "|",
        Token::XOR => "^",
        Token::SHL => "<<",
        Token::SHR => ">>",
        Token::LAND => "&&",
        Token::LOR => "||",
        Token::ARROW => "<-",
        Token::NOT => "!",
        Token::EQL => "==",
        Token::NEQ => "!=",
        Token::LSS => "<",
        Token::LEQ => "<=",
        Token::GTR => ">",
        Token::GEQ => ">=",
        _ => "?",
    }
}

const INT_CONVERSIONS: &[&str] = &[
    "byte", "rune", "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32",
    "uint64", "uintptr",
];

/// `intValue`.
fn expr_int_value(e: &Expr) -> Option<i64> {
    let mut expr = e;
    let mut negate = false;
    loop {
        match expr {
            Expr::UnaryExpr(u) if u.op == Token::SUB => {
                negate = !negate;
                expr = &u.x;
            }
            Expr::CallExpr(c) if c.args.len() == 1 => {
                let Expr::Ident(id) = unparen(&c.fun) else {
                    break;
                };
                if !INT_CONVERSIONS.contains(&id.name.as_str()) {
                    break;
                }
                expr = &c.args[0];
            }
            _ => break,
        }
    }
    let Expr::BasicLit(lit) = expr else {
        return None;
    };
    if lit.kind != Some(Token::INT) {
        return None;
    }
    let n: i64 = lit.value.parse().ok()?;
    Some(if negate { -n } else { n })
}

/// `hasVarReference`: identifiers that name a variable, skipping selector
/// fields, callee names, and composite-literal keys.
fn collect_refs(e: &Expr, out: &mut HashSet<String>) {
    match e {
        Expr::Ident(id) => {
            out.insert(id.name.clone());
        }
        Expr::SelectorExpr(s) => collect_refs(&s.x, out),
        Expr::CallExpr(c) => {
            for a in &c.args {
                collect_refs(a, out);
            }
        }
        Expr::KeyValueExpr(kv) => collect_refs(&kv.value, out),
        Expr::ParenExpr(p) => collect_refs(&p.x, out),
        Expr::UnaryExpr(u) => collect_refs(&u.x, out),
        Expr::StarExpr(s) => collect_refs(&s.x, out),
        Expr::BinaryExpr(b) => {
            collect_refs(&b.x, out);
            collect_refs(&b.y, out);
        }
        Expr::IndexExpr(i) => {
            collect_refs(&i.x, out);
            collect_refs(&i.index, out);
        }
        Expr::SliceExpr(s) => {
            collect_refs(&s.x, out);
            for e in [&s.low, &s.high, &s.max].into_iter().flatten() {
                collect_refs(e, out);
            }
        }
        Expr::CompositeLit(c) => {
            for e in &c.elts {
                collect_refs(e, out);
            }
        }
        Expr::TypeAssertExpr(t) => collect_refs(&t.x, out),
        _ => {}
    }
}

/// Rendered text of every sub-expression, for `hasAny`.
fn collect_subs(e: &Expr, out: &mut HashSet<String>) {
    out.insert(render_expr(e));
    match e {
        Expr::ParenExpr(p) => collect_subs(&p.x, out),
        Expr::UnaryExpr(u) => collect_subs(&u.x, out),
        Expr::StarExpr(s) => collect_subs(&s.x, out),
        Expr::SelectorExpr(s) => collect_subs(&s.x, out),
        Expr::BinaryExpr(b) => {
            collect_subs(&b.x, out);
            collect_subs(&b.y, out);
        }
        Expr::CallExpr(c) => {
            collect_subs(&c.fun, out);
            for a in &c.args {
                collect_subs(a, out);
            }
        }
        Expr::IndexExpr(i) => {
            collect_subs(&i.x, out);
            collect_subs(&i.index, out);
        }
        Expr::SliceExpr(s) => {
            collect_subs(&s.x, out);
            for e in [&s.low, &s.high, &s.max].into_iter().flatten() {
                collect_subs(e, out);
            }
        }
        Expr::CompositeLit(c) => {
            for e in &c.elts {
                collect_subs(e, out);
            }
        }
        Expr::KeyValueExpr(kv) => {
            collect_subs(&kv.key, out);
            collect_subs(&kv.value, out);
        }
        Expr::TypeAssertExpr(t) => collect_subs(&t.x, out),
        _ => {}
    }
}

fn leaf(e: &Expr) -> Cap {
    let mut refs = HashSet::new();
    collect_refs(e, &mut refs);
    let mut subs = HashSet::new();
    collect_subs(e, &mut subs);
    Cap::Leaf(Rc::new(Leaf {
        text: render_expr(e),
        refs,
        subs,
        int: expr_int_value(e),
    }))
}

/// `hasCall` — any call other than a type conversion, `len`/`cap`/`real`/
/// `imag`/`min`/`max`/`complex`, or an argument-less method.
fn has_call(e: &Expr) -> bool {
    let mut found = false;
    fn walk(e: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        if let Expr::CallExpr(c) = e {
            let cheap = match unparen(&c.fun) {
                Expr::ArrayType(_) | Expr::MapType(_) => true,
                Expr::Ident(id) => match id.name.as_str() {
                    "bool" | "error" | "string" | "any" | "byte" | "rune" | "int" | "int8"
                    | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16" | "uint32"
                    | "uint64" | "uintptr" | "float32" | "float64" | "complex64" | "complex128" => {
                        c.args.len() == 1
                    }
                    "len" | "cap" | "real" | "imag" | "min" | "max" | "complex" => true,
                    _ => false,
                },
                Expr::SelectorExpr(_) => c.args.is_empty(),
                _ => false,
            };
            if !cheap {
                *found = true;
                return;
            }
        }
        for_each_child(e, &mut |c| walk(c, found));
    }
    walk(e, &mut found);
    found
}

fn children(e: &Expr) -> Vec<&Expr> {
    let mut out = Vec::new();
    for_each_child(e, &mut |c| out.push(c));
    out
}

fn for_each_child<'e>(e: &'e Expr, f: &mut dyn FnMut(&'e Expr)) {
    match e {
        Expr::ParenExpr(p) => f(&p.x),
        Expr::UnaryExpr(u) => f(&u.x),
        Expr::StarExpr(s) => f(&s.x),
        Expr::SelectorExpr(s) => f(&s.x),
        Expr::BinaryExpr(b) => {
            f(&b.x);
            f(&b.y);
        }
        Expr::CallExpr(c) => {
            f(&c.fun);
            for a in &c.args {
                f(a);
            }
        }
        Expr::IndexExpr(i) => {
            f(&i.x);
            f(&i.index);
        }
        Expr::SliceExpr(s) => {
            f(&s.x);
            for e in [&s.low, &s.high, &s.max].into_iter().flatten() {
                f(e);
            }
        }
        Expr::CompositeLit(c) => {
            for e in &c.elts {
                f(e);
            }
        }
        Expr::KeyValueExpr(kv) => {
            f(&kv.key);
            f(&kv.value);
        }
        Expr::TypeAssertExpr(t) => f(&t.x),
        _ => {}
    }
}

fn expr_mentions_any(e: &Expr, texts: &[String]) -> bool {
    if texts.is_empty() {
        return false;
    }
    let mut subs = HashSet::new();
    collect_subs(e, &mut subs);
    texts.iter().any(|t| subs.contains(t))
}

fn is_append_call(c: &CallExpr) -> bool {
    matches!(unparen(&c.fun), Expr::Ident(id) if id.name == "append")
}

// ---------------------------------------------------------------------------
// Visitor
// ---------------------------------------------------------------------------

struct SliceDecl {
    name: String,
    pos: u32,
    level: i32,
    len_expr: Option<Cap>,
    exclude: bool,
    has_return: bool,
    assigning: bool,
    detached: bool,
}

struct SliceAppend {
    index: usize,
    count: Option<Cap>,
}

struct Visitor<'a, 'p> {
    pass: &'a Pass<'p>,
    options: PreallocOptions,
    decls: Vec<SliceDecl>,
    appends: Vec<Option<SliceAppend>>,
    loop_vars: Vec<String>,
    level: i32,
    has_return: bool,
    has_goto: bool,
    has_branch: bool,
    pending: Vec<(u32, String)>,
}

impl<'a, 'p> Visitor<'a, 'p> {
    fn type_of(&self, e: &Expr) -> Option<TypeId> {
        self.pass.types_info()?.types.get(&e.id()).map(|tv| tv.typ)
    }

    fn underlying(&self, e: &Expr) -> Option<(TypeId, TypeId)> {
        let t = self.type_of(e)?;
        let a = self.pass.pkg().type_artifacts.as_ref()?;
        Some((t, t.underlying(&a.types)))
    }

    fn is_slice_or_array_type_expr(&self, ty: &Expr) -> bool {
        let Some((_, u)) = self.underlying(ty) else {
            // Fall back to syntax when the checker has no type for the node.
            return matches!(ty, Expr::ArrayType(_));
        };
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return false;
        };
        matches!(a.types.get(u), TypeData::Array(_) | TypeData::Slice(_))
    }

    // -- declarations -------------------------------------------------------

    /// `isCreateArray`.
    fn create_array(&self, e: &Expr) -> Option<Cap> {
        match e {
            Expr::CompositeLit(cl) => {
                let (_, u) = self.underlying(e)?;
                let a = self.pass.pkg().type_artifacts.as_ref()?;
                match a.types.get(u) {
                    TypeData::Array(_) | TypeData::Slice(_) => Some(Cap::Int(cl.elts.len() as i64)),
                    _ => None,
                }
            }
            Expr::CallExpr(c) => match c.args.len() {
                1 => {
                    // `[]T(nil)`
                    if !matches!(unparen(&c.args[0]), Expr::Ident(id) if id.name == "nil") {
                        return None;
                    }
                    let (_, u) = self.underlying(&c.fun)?;
                    let a = self.pass.pkg().type_artifacts.as_ref()?;
                    matches!(a.types.get(u), TypeData::Slice(_)).then_some(Cap::Int(0))
                }
                2 => {
                    // `make([]T, n)`
                    matches!(unparen(&c.fun), Expr::Ident(id) if id.name == "make")
                        .then(|| leaf(&c.args[1]))
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn find_decl(&self, name: &str) -> Option<usize> {
        self.decls.iter().rposition(|d| d.name == name)
    }

    // -- statements ---------------------------------------------------------

    fn walk_block(&mut self, b: &BlockStmt) {
        let decl_idx = self.decls.len();
        let append_idx = self.appends.len();
        self.level += 1;
        for stmt in &b.list {
            self.walk_stmt(stmt);
        }
        self.level -= 1;

        for i in decl_idx..self.decls.len() {
            if self.decls[i].exclude || self.has_goto {
                continue;
            }
            let mut cap_expr = self.decls[i].len_expr.clone();
            let mut any = false;
            for j in append_idx..self.appends.len() {
                let Some(app) = &self.appends[j] else { continue };
                if app.index != i {
                    continue;
                }
                any = true;
                cap_expr = add_cap(cap_expr, app.count.clone());
            }
            if !any {
                continue;
            }
            if let Some(v) = cap_expr.as_ref().and_then(|c| c.int_value()) {
                if v <= 0 {
                    continue;
                }
            }
            let name = &self.decls[i].name;
            let message = match cap_expr.as_ref() {
                Some(c) => format!("Consider preallocating {name} with capacity {}", render(c)),
                None => format!("Consider preallocating {name}"),
            };
            self.pending.push((self.decls[i].pos, message));
        }

        // Discard declarations (and their appends) leaving scope.
        self.decls.truncate(decl_idx);
        let mut keep = append_idx;
        for i in append_idx..self.appends.len() {
            if let Some(app) = &self.appends[i] {
                if app.index >= decl_idx {
                    self.appends[i] = None;
                } else {
                    keep = i + 1;
                }
            }
        }
        self.appends.truncate(keep);
    }

    fn walk_value_spec(&mut self, vs: &guff::ast::ValueSpec) {
        let is_slice = vs
            .ty
            .as_ref()
            .is_some_and(|t| self.is_slice_or_array_type_expr(t));
        for (i, name) in vs.names.iter().enumerate() {
            let len_expr = match vs.values.get(i) {
                None => {
                    if !is_slice {
                        continue;
                    }
                    Cap::Int(0)
                }
                Some(v) => match self.create_array(v) {
                    Some(c) => c,
                    None => {
                        if !matches!(unparen(v), Expr::Ident(id) if id.name == "nil") {
                            continue;
                        }
                        Cap::Int(0)
                    }
                },
            };
            self.decls.push(SliceDecl {
                name: name.name.clone(),
                pos: name.name_pos.0 as u32,
                level: self.level,
                len_expr: Some(len_expr),
                exclude: false,
                has_return: false,
                assigning: false,
                detached: false,
            });
        }
        for v in &vs.values {
            self.walk_expr(v);
        }
    }

    fn walk_assign(&mut self, s: &AssignStmt) {
        if !self.loop_vars.is_empty() {
            if s.lhs.len() == s.rhs.len() {
                for (lhs, rhs) in s.lhs.iter().zip(s.rhs.iter()) {
                    if expr_mentions_any(rhs, &self.loop_vars) {
                        self.loop_vars.push(render_expr(lhs));
                    }
                }
            } else if s.rhs.len() == 1 && expr_mentions_any(&s.rhs[0], &self.loop_vars) {
                for lhs in &s.lhs {
                    self.loop_vars.push(render_expr(lhs));
                }
            }
        }
        if s.lhs.len() != s.rhs.len() {
            // Upstream returns nil: children are not walked.
            return;
        }
        for (i, lhs) in s.lhs.iter().enumerate() {
            let Expr::Ident(ident) = lhs else { continue };
            let pos = lhs.pos().0 as u32;
            if let Some(len_expr) = self.create_array(&s.rhs[i]) {
                self.decls.push(SliceDecl {
                    name: ident.name.clone(),
                    pos,
                    level: self.level,
                    len_expr: Some(len_expr),
                    exclude: false,
                    has_return: false,
                    assigning: false,
                    detached: false,
                });
                continue;
            }
            let Some(idx) = self.find_decl(&ident.name) else {
                continue;
            };
            match unparen(&s.rhs[i]) {
                Expr::Ident(id) if id.name == "nil" && s.tok == Some(Token::ASSIGN) => {
                    self.decls.push(SliceDecl {
                        name: ident.name.clone(),
                        pos,
                        level: self.level,
                        len_expr: Some(Cap::Int(0)),
                        exclude: false,
                        has_return: false,
                        assigning: false,
                        detached: false,
                    });
                    continue;
                }
                Expr::CallExpr(c)
                    if c.args.len() >= 2
                        && !self.decls[idx].has_return
                        && self.decls[idx].level == self.level
                        && is_append_call(c)
                        && matches!(unparen(&c.args[0]), Expr::Ident(a) if a.name == ident.name) =>
                {
                    self.decls[idx].assigning = true;
                    continue;
                }
                _ => {}
            }
            self.decls[idx].exclude = true;
        }
        for e in &s.lhs {
            self.walk_expr(e);
        }
        for e in &s.rhs {
            self.walk_expr(e);
        }
    }

    /// `appendCount`.
    fn append_count(&self, c: &CallExpr) -> Option<Cap> {
        if c.ellipsis.is_valid() {
            return self.slice_length(c.args.get(1)?);
        }
        Some(Cap::Int(c.args.len() as i64 - 1))
    }

    /// `sliceLength`.
    fn slice_length(&self, e: &Expr) -> Option<Cap> {
        let mut expr = e;
        if let Expr::CallExpr(c) = unparen(e) {
            if c.args.len() == 1 {
                if matches!(unparen(&c.fun), Expr::ArrayType(_)) {
                    expr = &c.args[0];
                }
            } else if c.args.len() >= 2 && is_append_call(c) {
                return add_cap(self.slice_length(&c.args[0]), self.append_count(c));
            }
        }
        let (_, u) = self.underlying(expr)?;
        let a = self.pass.pkg().type_artifacts.as_ref()?;
        match a.types.get(u) {
            TypeData::Array(_) | TypeData::Slice(_) => {
                if let Expr::CompositeLit(lit) = unparen(expr) {
                    return Some(Cap::Int(lit.elts.len() as i64));
                }
            }
            TypeData::Basic(_) => {
                if !guff_types::predicates::is_string(&a.types, u) {
                    return None;
                }
                if let Expr::BasicLit(lit) = unparen(expr) {
                    if lit.kind == Some(Token::STRING) {
                        return Some(Cap::Int(unquote_len(&lit.value)? as i64));
                    }
                }
            }
            _ => return None,
        }
        if has_call(expr) {
            return None;
        }
        if let Expr::SliceExpr(s) = unparen(expr) {
            let high = match &s.high {
                Some(h) => leaf(h),
                None => Cap::Len(Rc::new(leaf(&s.x))),
            };
            return match &s.low {
                Some(l) => sub_cap(Some(high), Some(leaf(l))),
                None => Some(high),
            };
        }
        Some(Cap::Len(Rc::new(leaf(expr))))
    }

    fn walk_call(&mut self, c: &CallExpr) {
        if is_append_call(c) && c.args.len() >= 2 {
            if let Expr::Ident(target) = unparen(&c.args[0]) {
                if let Some(idx) = self.find_decl(&target.name) {
                    if !self.decls[idx].exclude {
                        if self.decls[idx].has_return
                            || self.decls[idx].level != self.level
                            || self.decls[idx].detached
                        {
                            self.decls[idx].exclude = true;
                        } else {
                            let count = self.append_count(c);
                            let bad = count.as_ref().is_some_and(|cnt| {
                                cnt.mentions_any(&self.loop_vars)
                                    || cnt.refs_name(&self.decls[idx].name)
                            });
                            if bad {
                                self.decls[idx].exclude = true;
                            } else {
                                if self.decls[idx].assigning {
                                    self.decls[idx].assigning = false;
                                } else {
                                    self.decls[idx].detached = true;
                                }
                                self.appends.push(Some(SliceAppend { index: idx, count }));
                            }
                        }
                    }
                }
            }
        }
        self.walk_expr(&c.fun);
        for a in &c.args {
            self.walk_expr(a);
        }
    }

    fn walk_expr(&mut self, e: &Expr) {
        match e {
            Expr::CallExpr(c) => self.walk_call(c),
            Expr::FuncLit(f) => {
                let was_return = self.has_return;
                let was_goto = self.has_goto;
                self.has_return = false;
                self.walk_block(&f.body);
                self.has_return = was_return;
                self.has_goto = was_goto;
            }
            other => {
                for k in children(other) {
                    self.walk_expr(k);
                }
            }
        }
    }

    /// `rangeLoopCount` — `(count, supported)`.
    fn range_loop_count(&self, s: &RangeStmt) -> (Option<Cap>, bool) {
        let mut x = &s.x;
        if expr_mentions_any(x, &self.loop_vars) {
            return (None, false);
        }
        if let Expr::CallExpr(c) = unparen(x) {
            if c.args.len() == 1 {
                if matches!(unparen(&c.fun), Expr::ArrayType(_)) {
                    x = &c.args[0];
                }
            } else if c.args.len() >= 2 && is_append_call(c) {
                return (
                    add_cap(self.slice_length(&c.args[0]), self.append_count(c)),
                    true,
                );
            }
        }

        let Some((_, u)) = self.underlying(x) else {
            return (None, true);
        };
        let Some(a) = self.pass.pkg().type_artifacts.as_ref() else {
            return (None, true);
        };
        let mut basic_is_integer = false;
        match a.types.get(u) {
            TypeData::Chan(_) | TypeData::Signature(_) => return (None, false),
            TypeData::Array(arr) => {
                if matches!(unparen(&s.x), Expr::CompositeLit(_)) && arr.len() >= 0 {
                    return (Some(Cap::Int(arr.len())), true);
                }
            }
            TypeData::Slice(_) => {
                if let Expr::CompositeLit(lit) = unparen(&s.x) {
                    return (Some(Cap::Int(lit.elts.len() as i64)), true);
                }
            }
            TypeData::Map(_) => {
                if let Expr::CompositeLit(lit) = unparen(x) {
                    return (Some(Cap::Int(lit.elts.len() as i64)), true);
                }
            }
            TypeData::Pointer(p) => {
                let pe = p.elem().underlying(&a.types);
                let TypeData::Array(arr) = a.types.get(pe) else {
                    return (None, true);
                };
                if let Expr::UnaryExpr(un) = unparen(x) {
                    if un.op == Token::AND
                        && matches!(unparen(&un.x), Expr::CompositeLit(_))
                        && arr.len() >= 0
                    {
                        return (Some(Cap::Int(arr.len())), true);
                    }
                }
            }
            TypeData::Basic(_) => {
                if guff_types::predicates::is_string(&a.types, u) {
                    if let Expr::BasicLit(lit) = unparen(x) {
                        if lit.kind == Some(Token::STRING) {
                            if let Some(n) = unquote_len(&lit.value) {
                                return (Some(Cap::Int(n as i64)), true);
                            }
                        }
                    }
                } else if guff_types::predicates::is_integer(&a.types, u) {
                    basic_is_integer = true;
                } else {
                    return (None, true);
                }
            }
            _ => return (None, true),
        }

        if has_call(x) {
            return (None, true);
        }
        if basic_is_integer {
            // `for i := range n` — the bound is the value itself.
            return (Some(leaf(x)), true);
        }
        if let Expr::SliceExpr(sl) = unparen(x) {
            let high = match &sl.high {
                Some(h) => leaf(h),
                None => Cap::Len(Rc::new(leaf(&sl.x))),
            };
            return match &sl.low {
                Some(l) => (sub_cap(Some(high), Some(leaf(l))), true),
                None => (Some(high), true),
            };
        }
        (Some(Cap::Len(Rc::new(leaf(x)))), true)
    }

    fn walk_range(&mut self, s: &RangeStmt) {
        if self.decls.is_empty() {
            self.walk_stmt_children_of_range(s);
            return;
        }
        let append_idx = self.appends.len();
        let had_branch = self.has_branch;
        self.has_branch = false;
        self.level -= 1;
        let vars_idx = self.loop_vars.len();
        if let Some(k) = &s.key {
            self.loop_vars.push(render_expr(k));
        }
        if let Some(v) = &s.value {
            self.loop_vars.push(render_expr(v));
        }
        self.walk_block(&s.body);
        self.level += 1;
        self.loop_vars.truncate(vars_idx);

        let mut exclude =
            !self.options.range_loops || self.has_return || self.has_goto || self.has_branch;
        let mut loop_count = None;
        if !exclude {
            let (count, ok) = self.range_loop_count(s);
            loop_count = count;
            exclude = !ok;
        }
        self.finish_loop(append_idx, exclude, loop_count);
        self.has_branch = had_branch;
    }

    /// Upstream keeps walking the range's children when there are no slice
    /// declarations yet (`return v`), so nested declarations are still seen.
    fn walk_stmt_children_of_range(&mut self, s: &RangeStmt) {
        if let Some(k) = &s.key {
            self.walk_expr(k);
        }
        if let Some(v) = &s.value {
            self.walk_expr(v);
        }
        self.walk_expr(&s.x);
        self.walk_block(&s.body);
    }

    fn finish_loop(&mut self, append_idx: usize, exclude: bool, loop_count: Option<Cap>) {
        if exclude {
            for i in append_idx..self.appends.len() {
                if let Some(app) = &self.appends[i] {
                    let idx = app.index;
                    self.decls[idx].exclude = true;
                }
            }
            return;
        }
        for i in 0..self.decls.len() {
            if self.decls[i].exclude {
                continue;
            }
            let mut prev: Option<usize> = None;
            for j in (append_idx..self.appends.len()).rev() {
                let Some(app) = &self.appends[j] else { continue };
                if app.index != i {
                    continue;
                }
                match prev {
                    None => {
                        match loop_count.as_ref() {
                            None => {
                                if let Some(a) = self.appends[j].as_mut() {
                                    a.count = None;
                                }
                            }
                            Some(lc) => {
                                if lc.refs_name(&self.decls[i].name) {
                                    self.decls[i].exclude = true;
                                    break;
                                }
                            }
                        }
                    }
                    Some(p) => {
                        let merged = add_cap(
                            self.appends[j].as_ref().and_then(|a| a.count.clone()),
                            self.appends[p].as_ref().and_then(|a| a.count.clone()),
                        );
                        if let Some(a) = self.appends[j].as_mut() {
                            a.count = merged;
                        }
                        self.appends[p] = None;
                    }
                }
                prev = Some(j);
            }
            if let Some(p) = prev {
                let merged = mul_cap(
                    self.appends[p].as_ref().and_then(|a| a.count.clone()),
                    loop_count.clone(),
                );
                if let Some(a) = self.appends[p].as_mut() {
                    a.count = merged;
                }
            }
        }
    }

    fn walk_for(&mut self, s: &ForStmt) {
        if self.decls.is_empty() {
            if let Some(init) = &s.init {
                self.walk_stmt(init);
            }
            if let Some(cond) = &s.cond {
                self.walk_expr(cond);
            }
            if let Some(post) = &s.post {
                self.walk_stmt(post);
            }
            self.walk_block(&s.body);
            return;
        }
        let append_idx = self.appends.len();
        let had_branch = self.has_branch;
        self.has_branch = false;
        self.level -= 1;
        let vars_idx = self.loop_vars.len();
        if let Some(Stmt::AssignStmt(a)) = s.init.as_deref() {
            for lhs in &a.lhs {
                self.loop_vars.push(render_expr(lhs));
            }
        }
        self.walk_block(&s.body);
        self.level += 1;
        self.loop_vars.truncate(vars_idx);

        let exclude =
            !self.options.for_loops || self.has_return || self.has_goto || self.has_branch;
        // `for-loops` is off by default; without an ported `forLoopCount` the
        // loop trip count is treated as indeterminate, which upstream renders
        // as a message without a capacity.
        self.finish_loop(append_idx, exclude, None);
        self.has_branch = had_branch;
    }

    fn walk_case_bodies(&mut self, body: &BlockStmt) {
        let had_branch = self.has_branch;
        self.has_branch = false;
        self.walk_block(body);
        self.has_branch = had_branch;
    }

    fn walk_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::BlockStmt(b) => self.walk_block(b),
            Stmt::DeclStmt(d) => {
                if let Decl::GenDecl(g) = &d.decl {
                    self.walk_gendecl(g);
                }
            }
            Stmt::AssignStmt(a) => self.walk_assign(a),
            Stmt::RangeStmt(r) => self.walk_range(r),
            Stmt::ForStmt(f) => self.walk_for(f),
            Stmt::SwitchStmt(s) => self.walk_case_bodies(&s.body),
            Stmt::TypeSwitchStmt(s) => self.walk_case_bodies(&s.body),
            Stmt::SelectStmt(s) => self.walk_case_bodies(&s.body),
            Stmt::ReturnStmt(r) => {
                if self.options.simple {
                    self.has_return = true;
                    for i in 0..self.appends.len() {
                        if let Some(app) = &self.appends[i] {
                            let idx = app.index;
                            self.decls[idx].has_return = true;
                        }
                    }
                    for e in &r.results {
                        self.walk_expr(e);
                    }
                }
            }
            Stmt::BranchStmt(b) => {
                if self.options.simple {
                    match b.tok {
                        Token::GOTO => self.has_goto = true,
                        Token::BREAK | Token::CONTINUE | Token::FALLTHROUGH => {
                            if b.label.is_some() {
                                self.has_goto = true;
                            } else {
                                self.has_branch = true;
                            }
                        }
                        _ => {}
                    }
                }
            }
            Stmt::IfStmt(i) => {
                if let Some(init) = &i.init {
                    self.walk_stmt(init);
                }
                self.walk_expr(&i.cond);
                self.walk_block(&i.body);
                if let Some(e) = &i.else_ {
                    self.walk_stmt(e);
                }
            }
            Stmt::LabeledStmt(l) => self.walk_stmt(&l.stmt),
            Stmt::ExprStmt(e) => self.walk_expr(&e.x),
            Stmt::IncDecStmt(i) => self.walk_expr(&i.x),
            Stmt::SendStmt(s) => {
                self.walk_expr(&s.chan_);
                self.walk_expr(&s.value);
            }
            Stmt::GoStmt(g) => self.walk_call(&g.call),
            Stmt::DeferStmt(d) => self.walk_call(&d.call),
            Stmt::CaseClause(c) => {
                for e in &c.list {
                    self.walk_expr(e);
                }
                for s in &c.body {
                    self.walk_stmt(s);
                }
            }
            Stmt::CommClause(c) => {
                if let Some(comm) = &c.comm {
                    self.walk_stmt(comm);
                }
                for s in &c.body {
                    self.walk_stmt(s);
                }
            }
            _ => {}
        }
    }

    fn walk_gendecl(&mut self, g: &GenDecl) {
        if g.tok != Some(Token::VAR) {
            return;
        }
        for spec in &g.specs {
            if let Spec::ValueSpec(vs) = spec {
                self.walk_value_spec(vs);
            }
        }
    }

    fn walk_func_body(&mut self, body: &BlockStmt) {
        self.level = 0;
        self.has_return = false;
        self.has_goto = false;
        self.walk_block(body);
    }
}

/// Length of a Go string literal's value, as `strconv.Unquote` + `len` would
/// report. Falls back to `None` for escapes we do not decode.
fn unquote_len(lit: &str) -> Option<usize> {
    let bytes = lit.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let (open, close) = (bytes[0], bytes[bytes.len() - 1]);
    let inner = &lit[1..lit.len() - 1];
    if open == b'`' && close == b'`' {
        return Some(inner.len());
    }
    if open != b'"' || close != b'"' {
        return None;
    }
    if inner.contains('\\') {
        return None;
    }
    Some(inner.len())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "prealloc requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<PreallocOptions>("prealloc")
        .copied()
        .unwrap_or_default();

    // One visitor for the whole package, as upstream's `Check` does. A
    // package-level `var x = []T{…}` is never truncated (it is not inside a
    // BlockStmt), so it keeps `sliceDeclarations` non-empty for every later
    // file — which is what makes `walkRange` track loop variables there at all.
    // Per-file visitors dropped that and re-reported grafana's
    // `cloudmigrationimpl` slices, whose inner loop bound references the outer
    // loop's variable.
    let mut v = Visitor {
        pass,
        options,
        decls: Vec::new(),
        appends: Vec::new(),
        loop_vars: Vec::new(),
        level: 0,
        has_return: false,
        has_goto: false,
        has_branch: false,
        pending: Vec::new(),
    };
    for file in pass.files() {
        for decl in &file.decls {
            match decl {
                Decl::FuncDecl(f) => {
                    if let Some(body) = &f.body {
                        v.walk_func_body(body);
                    }
                }
                Decl::GenDecl(g) => v.walk_gendecl(g),
                _ => {}
            }
        }
    }
    let pending = std::mem::take(&mut v.pending);

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "prealloc",
        doc: "Find slice declarations that could potentially be pre-allocated",
        url: "https://github.com/alexkohler/prealloc",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_go_printer_spacing() {
        let len_a = Cap::Len(Rc::new(Cap::Int(1)));
        let sum = Cap::Bin(Rc::new(Cap::Int(1)), Op::Add, Rc::new(len_a.clone()));
        assert_eq!(render(&sum), "1 + len(1)");
        let prod = Cap::Bin(Rc::new(Cap::Int(2)), Op::Mul, Rc::new(len_a.clone()));
        assert_eq!(render(&prod), "2 * len(1)");
        // A multiplication nested under an addition loses its blanks.
        let mixed = Cap::Bin(Rc::new(Cap::Int(1)), Op::Add, Rc::new(prod));
        assert_eq!(render(&mixed), "1 + 2*len(1)");
    }

    #[test]
    fn folds_constants() {
        assert_eq!(
            add_cap(cap_int(2), cap_int(3)).and_then(|c| c.int_value()),
            Some(5)
        );
        assert_eq!(
            mul_cap(cap_int(1), Some(Cap::Len(Rc::new(Cap::Int(0)))))
                .map(|c| render(&c))
                .as_deref(),
            Some("len(0)")
        );
    }
}
