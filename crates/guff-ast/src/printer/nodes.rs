// Copyright 2026 The guff Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.
//
// Port of Go's go/printer/nodes.go.
//
// The original file is Copyright 2009 The Go Authors. All rights reserved.
// It is governed by a BSD-style license.
//
// This file implements printing of AST nodes; specifically expressions,
// statements, declarations, and files.

use crate::ast::*;
use crate::token::{Token, HIGHEST_PREC, LOWEST_PREC, UNARY_PREC};
use crate::{Pos, NO_POS};

use super::printer::{
    ExprListMode, Item, ParamMode, Printer, WhiteSpace, NO_EXTRA_BLANK, NO_EXTRA_LINEBREAK,
};

const IGNORE: WhiteSpace = WhiteSpace(0);
const BLANK: WhiteSpace = WhiteSpace(b' ');
const VTAB: WhiteSpace = WhiteSpace(b'\x0b');
const NEWLINE: WhiteSpace = WhiteSpace(b'\n');
const FORMFEED: WhiteSpace = WhiteSpace(b'\x0c');
const INDENT: WhiteSpace = WhiteSpace(b'>');
const UNINDENT: WhiteSpace = WhiteSpace(b'<');
const FILTERED_MSG: &str = "contains filtered or unexported fields";
const INFINITY: usize = 1_000_000;

fn ws(ws: WhiteSpace) -> Item<'static> {
    Item::Ws(ws)
}
fn tok(tok: Token) -> Item<'static> {
    Item::Tok(tok)
}
fn text(s: &'static str) -> Item<'static> {
    Item::Str(s)
}

fn strip_parens_always(x: &Expr) -> &Expr {
    match x {
        Expr::ParenExpr(x) => strip_parens_always(&x.x),
        _ => x,
    }
}

fn is_type_name(x: &Expr) -> bool {
    match x {
        Expr::Ident(_) => true,
        Expr::SelectorExpr(x) => is_type_name(&x.x),
        _ => false,
    }
}

// This mirrors ast.Inspect in nodes.go. The Rust AST owns its children, so a
// small recursive predicate is less allocation-heavy than constructing Nodes.
fn has_unprotected_type_named_composite(x: &Expr) -> bool {
    match x {
        Expr::ParenExpr(_) => false,
        Expr::CompositeLit(x) => x.ty.as_deref().is_some_and(is_type_name),
        Expr::Ellipsis(x) => x
            .elt
            .as_deref()
            .is_some_and(has_unprotected_type_named_composite),
        Expr::FuncLit(_) | Expr::BasicLit(_) | Expr::Ident(_) | Expr::BadExpr(_) => false,
        Expr::SelectorExpr(x) => has_unprotected_type_named_composite(&x.x),
        Expr::IndexExpr(x) => {
            has_unprotected_type_named_composite(&x.x)
                || has_unprotected_type_named_composite(&x.index)
        }
        Expr::IndexListExpr(x) => {
            has_unprotected_type_named_composite(&x.x)
                || x.indices.iter().any(has_unprotected_type_named_composite)
        }
        Expr::SliceExpr(x) => {
            has_unprotected_type_named_composite(&x.x)
                || x.low
                    .as_deref()
                    .is_some_and(has_unprotected_type_named_composite)
                || x.high
                    .as_deref()
                    .is_some_and(has_unprotected_type_named_composite)
                || x.max
                    .as_deref()
                    .is_some_and(has_unprotected_type_named_composite)
        }
        Expr::TypeAssertExpr(x) => {
            has_unprotected_type_named_composite(&x.x)
                || x.ty
                    .as_deref()
                    .is_some_and(has_unprotected_type_named_composite)
        }
        Expr::CallExpr(x) => {
            has_unprotected_type_named_composite(&x.fun)
                || x.args.iter().any(has_unprotected_type_named_composite)
        }
        Expr::StarExpr(x) => has_unprotected_type_named_composite(&x.x),
        Expr::UnaryExpr(x) => has_unprotected_type_named_composite(&x.x),
        Expr::BinaryExpr(x) => {
            has_unprotected_type_named_composite(&x.x) || has_unprotected_type_named_composite(&x.y)
        }
        Expr::KeyValueExpr(x) => {
            has_unprotected_type_named_composite(&x.key)
                || has_unprotected_type_named_composite(&x.value)
        }
        Expr::ArrayType(x) => {
            x.len
                .as_deref()
                .is_some_and(has_unprotected_type_named_composite)
                || has_unprotected_type_named_composite(&x.elt)
        }
        Expr::StructType(_)
        | Expr::FuncType(_)
        | Expr::InterfaceType(_)
        | Expr::MapType(_)
        | Expr::ChanType(_) => false,
    }
}

fn strip_parens(x: &Expr) -> &Expr {
    if let Expr::ParenExpr(x) = x {
        if !has_unprotected_type_named_composite(&x.x) {
            return strip_parens(&x.x);
        }
    }
    x
}

fn is_type_elem(x: &Expr) -> bool {
    match x {
        Expr::ArrayType(_)
        | Expr::StructType(_)
        | Expr::FuncType(_)
        | Expr::InterfaceType(_)
        | Expr::MapType(_)
        | Expr::ChanType(_) => true,
        Expr::UnaryExpr(x) => x.op == Token::TILDE,
        Expr::BinaryExpr(x) => is_type_elem(&x.x) || is_type_elem(&x.y),
        Expr::ParenExpr(x) => is_type_elem(&x.x),
        _ => false,
    }
}

fn combines_with_name(x: &Expr) -> bool {
    match x {
        Expr::StarExpr(x) => !is_type_elem(&x.x),
        Expr::BinaryExpr(x) => combines_with_name(&x.x) && !is_type_elem(&x.y),
        Expr::ParenExpr(x) => !is_type_elem(&x.x),
        _ => false,
    }
}

fn is_binary(x: &Expr) -> bool {
    matches!(x, Expr::BinaryExpr(_))
}

fn diff_prec(x: &Expr, prec: i32) -> i32 {
    match x {
        Expr::BinaryExpr(x) if x.op.precedence() == prec => 0,
        _ => 1,
    }
}

fn reduce_depth(depth: i32) -> i32 {
    (depth - 1).max(1)
}

fn walk_binary(x: &BinaryExpr) -> (bool, bool, i32) {
    let mut has4 = x.op.precedence() == 4;
    let mut has5 = x.op.precedence() == 5;
    let mut max_problem = 0;
    if let Expr::BinaryExpr(left) = &*x.x {
        if left.op.precedence() >= x.op.precedence() {
            let (h4, h5, mp) = walk_binary(left);
            has4 |= h4;
            has5 |= h5;
            max_problem = max_problem.max(mp);
        }
    }
    match &*x.y {
        Expr::BinaryExpr(right) if right.op.precedence() > x.op.precedence() => {
            let (h4, h5, mp) = walk_binary(right);
            has4 |= h4;
            has5 |= h5;
            max_problem = max_problem.max(mp);
        }
        Expr::StarExpr(_) if x.op == Token::QUO => max_problem = 5,
        Expr::UnaryExpr(right) => match (x.op.as_str(), right.op.as_str()) {
            // Go: e.Op.String() + r.Op.String() ∈ {"/*","&&","&^"}
            ("/", "*") | ("&", "&") | ("&", "^") => max_problem = 5,
            ("+", "+") | ("-", "-") => max_problem = max_problem.max(4),
            _ => {}
        },
        _ => {}
    }
    (has4, has5, max_problem)
}

fn cutoff(x: &BinaryExpr, depth: i32) -> i32 {
    let (has4, has5, problem) = walk_binary(x);
    if problem > 0 {
        problem + 1
    } else if has4 && has5 {
        if depth == 1 {
            5
        } else {
            4
        }
    } else if depth == 1 {
        6
    } else {
        4
    }
}

/// Rewrite number prefixes and exponents into their canonical case.
///
/// Unlike Go's pointer-returning version, this always returns an owned value;
/// callers select it only when `NORMALIZE_NUMBERS` is enabled.
pub(crate) fn normalized_number(lit: &BasicLit) -> BasicLit {
    if !matches!(lit.kind, Some(Token::INT | Token::FLOAT | Token::IMAG)) || lit.value.len() < 2 {
        return lit.clone();
    }
    let mut value = lit.value.clone();
    match &value[..2] {
        "0X" => {
            value.replace_range(..2, "0x");
            if let Some(i) = value.rfind('P') {
                value.replace_range(i..=i, "p");
            }
        }
        "0x" => {
            let Some(i) = value.rfind('P') else {
                return lit.clone();
            };
            value.replace_range(i..=i, "p");
        }
        "0O" => value.replace_range(..2, "0o"),
        "0o" => return lit.clone(),
        "0B" => value.replace_range(..2, "0b"),
        "0b" => return lit.clone(),
        _ => {
            if let Some(i) = value.rfind('E') {
                value.replace_range(i..=i, "e");
            } else if value.ends_with('i') && !value.contains(['.', 'e']) {
                let trimmed = value.trim_start_matches(['0', '_']);
                value = if trimmed == "i" {
                    "0i".into()
                } else {
                    trimmed.into()
                };
            }
        }
    }
    let mut result = lit.clone();
    result.value = value;
    result
}

impl<'a> Printer<'a> {
    pub(crate) fn ident_list(&mut self, list: &[Ident], indent: bool) {
        let list: Vec<Expr> = list.iter().cloned().map(Expr::Ident).collect();
        let mode = if indent {
            ExprListMode::empty()
        } else {
            ExprListMode::NO_INDENT
        };
        self.expr_list(NO_POS, &list, 1, mode, NO_POS, false);
    }

    pub(crate) fn expr_list(
        &mut self,
        prev0: Pos,
        list: &[Expr],
        depth: i32,
        mode: ExprListMode,
        next0: Pos,
        incomplete: bool,
    ) {
        if list.is_empty() {
            if incomplete {
                let prev = self.pos_for(prev0);
                let next = self.pos_for(next0);
                if prev.is_valid() && prev.line == next.line {
                    self.print(&[text("/* contains filtered or unexported fields */")]);
                } else {
                    self.print(&[
                        ws(NEWLINE),
                        ws(INDENT),
                        text("// contains filtered or unexported fields"),
                        ws(UNINDENT),
                        ws(NEWLINE),
                    ]);
                }
            }
            return;
        }
        let prev = self.pos_for(prev0);
        let next = self.pos_for(next0);
        let mut line = self.line_for(list[0].pos());
        let end_line = self.line_for(list.last().unwrap().end());
        if prev.is_valid() && prev.line == line && line == end_line {
            for (i, x) in list.iter().enumerate() {
                if i > 0 {
                    self.set_pos(x.pos());
                    self.print(&[tok(Token::COMMA), ws(BLANK)]);
                }
                self.expr0(x, depth);
            }
            if incomplete {
                self.print(&[
                    tok(Token::COMMA),
                    ws(BLANK),
                    text("/* contains filtered or unexported fields */"),
                ]);
            }
            return;
        }
        let mut w = if mode.contains(ExprListMode::NO_INDENT) {
            IGNORE
        } else {
            INDENT
        };
        let mut prev_break: isize = -1;
        if prev.is_valid() && prev.line < line && self.linebreak(line, 0, w, true) > 0 {
            w = IGNORE;
            prev_break = 0;
        }
        let mut size = 0usize;
        let mut lnsum = 0.0f64;
        let mut count = 0usize;
        let mut prev_line = prev.line;
        for (i, x) in list.iter().enumerate() {
            line = self.line_for(x.pos());
            let prev_size = size;
            size = self.node_size(x, INFINITY);
            let pair = if let Expr::KeyValueExpr(pair) = x {
                Some(pair)
            } else {
                None
            };
            if size <= INFINITY && prev.is_valid() && next.is_valid() {
                if let Some(pair) = pair {
                    size = self.node_size(&pair.key, INFINITY);
                }
            } else {
                size = 0;
            }
            let mut use_ff = true;
            if prev_size > 0 && size > 0 {
                if count == 0 || (prev_size <= 40 && size <= 40) {
                    use_ff = false;
                } else {
                    let ratio = size as f64 / (lnsum / count as f64).exp();
                    // Match Go: use formfeed when the size ratio is extreme.
                    use_ff = 2.5 * ratio <= 1.0 || 2.5 <= ratio;
                }
            }
            let needs_break = prev_line > 0 && prev_line < line;
            if i > 0 {
                if !needs_break {
                    self.set_pos(x.pos());
                }
                self.print(&[tok(Token::COMMA)]);
                let mut blank = true;
                if needs_break {
                    let n = self.linebreak(line, 0, w, use_ff || prev_break + 1 < i as isize);
                    if n > 0 {
                        w = IGNORE;
                        prev_break = i as isize;
                        blank = false;
                    }
                    if n > 1 {
                        lnsum = 0.0;
                        count = 0;
                    }
                }
                if blank {
                    self.print(&[ws(BLANK)]);
                }
            }
            if list.len() > 1 && pair.is_some() && size > 0 && needs_break {
                let pair = pair.unwrap();
                self.expr(&pair.key);
                self.set_pos(pair.colon);
                self.print(&[tok(Token::COLON), ws(VTAB)]);
                self.expr(&pair.value);
            } else {
                self.expr0(x, depth);
            }
            if size > 0 {
                lnsum += (size as f64).ln();
                count += 1;
            }
            prev_line = line;
        }
        if mode.contains(ExprListMode::COMMA_TERM) && next.is_valid() && self.pos.line < next.line {
            self.print(&[tok(Token::COMMA)]);
            if incomplete {
                self.print(&[
                    ws(NEWLINE),
                    text("// contains filtered or unexported fields"),
                ]);
            }
            if w == IGNORE && !mode.contains(ExprListMode::NO_INDENT) {
                self.print(&[ws(UNINDENT)]);
            }
            self.print(&[ws(FORMFEED)]);
        } else {
            if incomplete {
                self.print(&[
                    tok(Token::COMMA),
                    ws(NEWLINE),
                    text("// contains filtered or unexported fields"),
                    ws(NEWLINE),
                ]);
            }
            if w == IGNORE && !mode.contains(ExprListMode::NO_INDENT) {
                self.print(&[ws(UNINDENT)]);
            }
        }
    }

    pub(crate) fn parameters(&mut self, fields: &FieldList, mode: ParamMode) {
        let (open, close) = if mode == ParamMode::FUNC_PARAM {
            (Token::LPAREN, Token::RPAREN)
        } else {
            (Token::LBRACK, Token::RBRACK)
        };
        self.set_pos(fields.opening);
        self.print(&[tok(open)]);
        if !fields.list.is_empty() {
            let mut prev_line = self.line_for(fields.opening);
            let mut w = INDENT;
            for (i, par) in fields.list.iter().enumerate() {
                let begin = self.line_for(par.pos());
                let end = self.line_for(par.end());
                let needs_break = prev_line > 0 && prev_line < begin;
                if i > 0 {
                    if !needs_break {
                        self.set_pos(par.pos());
                    }
                    self.print(&[tok(Token::COMMA)]);
                }
                if needs_break && self.linebreak(begin, 0, w, true) > 0 {
                    w = IGNORE;
                } else if i > 0 {
                    self.print(&[ws(BLANK)]);
                }
                if !par.names.is_empty() {
                    self.ident_list(&par.names, w == INDENT);
                    self.print(&[ws(BLANK)]);
                }
                if let Some(ty) = &par.ty {
                    self.expr(strip_parens_always(ty));
                }
                prev_line = end;
            }
            if prev_line > 0 && prev_line < self.line_for(fields.closing) {
                self.print(&[tok(Token::COMMA)]);
                self.linebreak(self.line_for(fields.closing), 0, IGNORE, true);
            } else if mode == ParamMode::TYPE_TPARAM
                && fields.num_fields() == 1
                && fields.list[0]
                    .ty
                    .as_ref()
                    .is_some_and(|x| combines_with_name(strip_parens_always(x)))
            {
                self.print(&[tok(Token::COMMA)]);
            }
            if w == IGNORE {
                self.print(&[ws(UNINDENT)]);
            }
        }
        self.set_pos(fields.closing);
        self.print(&[tok(close)]);
    }

    pub(crate) fn signature(&mut self, sig: &FuncType) {
        if let Some(x) = &sig.type_params {
            self.parameters(x, ParamMode::FUNC_TPARAM);
        }
        if let Some(x) = &sig.params {
            self.parameters(x, ParamMode::FUNC_PARAM);
        } else {
            self.print(&[tok(Token::LPAREN), tok(Token::RPAREN)]);
        }
        if let Some(res) = &sig.results {
            if res.num_fields() > 0 {
                self.print(&[ws(BLANK)]);
                if res.num_fields() == 1 && res.list[0].names.is_empty() {
                    if let Some(ty) = &res.list[0].ty {
                        self.expr(strip_parens_always(ty));
                    }
                } else {
                    self.parameters(res, ParamMode::FUNC_PARAM);
                }
            }
        }
    }

    fn is_one_line_field_list(&mut self, list: &[Field]) -> bool {
        if list.len() != 1 {
            return false;
        }
        let f = &list[0];
        if f.tag.is_some() || f.comment.is_some() {
            return false;
        }
        const MAX_SIZE: usize = 30;
        let mut names_size = 0usize;
        for (i, x) in f.names.iter().enumerate() {
            if i > 0 {
                names_size += 2; // ", "
            }
            names_size += x.name.chars().count();
            if names_size >= MAX_SIZE {
                break;
            }
        }
        if names_size > 0 {
            names_size = 1; // blank between names and type
        }
        let type_size = f
            .ty
            .as_ref()
            .map(|t| self.node_size(t, MAX_SIZE))
            .unwrap_or(0);
        names_size + type_size <= MAX_SIZE
    }

    fn field_list(&mut self, fields: &FieldList, is_struct: bool, incomplete: bool) {
        let has_comments = incomplete || self.comment_before(&self.pos_for(fields.closing));
        let one_line = fields.opening.is_valid()
            && fields.closing.is_valid()
            && self.line_for(fields.opening) == self.line_for(fields.closing);
        if !has_comments && one_line {
            if fields.list.is_empty() {
                self.set_pos(fields.opening);
                self.print(&[tok(Token::LBRACE)]);
                self.set_pos(fields.closing);
                self.print(&[tok(Token::RBRACE)]);
                return;
            } else if self.is_one_line_field_list(&fields.list) {
                self.set_pos(fields.opening);
                self.print(&[tok(Token::LBRACE), ws(BLANK)]);
                let f = &fields.list[0];
                if is_struct {
                    for (i, x) in f.names.iter().enumerate() {
                        if i > 0 {
                            self.print(&[tok(Token::COMMA), ws(BLANK)]);
                        }
                        self.set_pos(x.pos());
                        self.print(&[Item::Ident(x)]);
                    }
                    if !f.names.is_empty() {
                        self.print(&[ws(BLANK)]);
                    }
                    if let Some(ty) = &f.ty {
                        self.expr(ty);
                    }
                } else if let Some(name) = f.names.first() {
                    self.set_pos(name.pos());
                    self.print(&[Item::Ident(name)]);
                    if let Some(Expr::FuncType(ty)) = &f.ty {
                        self.signature(ty);
                    }
                } else if let Some(ty) = &f.ty {
                    self.expr(ty);
                }
                self.print(&[ws(BLANK)]);
                self.set_pos(fields.closing);
                self.print(&[tok(Token::RBRACE)]);
                return;
            }
        }
        self.print(&[ws(BLANK)]);
        self.set_pos(fields.opening);
        self.print(&[tok(Token::LBRACE), ws(INDENT)]);
        if has_comments || !fields.list.is_empty() {
            self.print(&[ws(FORMFEED)]);
        }
        if is_struct {
            // Match Go: single-field structs use blank as column separator;
            // multi-field use vtab so comments/tags align via tabwriter.
            let sep = if fields.list.len() == 1 { BLANK } else { VTAB };
            let mut line = 0;
            for (i, f) in fields.list.iter().enumerate() {
                if i > 0 {
                    self.linebreak(self.line_for(f.pos()), 1, IGNORE, self.lines_from(line) > 0);
                }
                let mut extra_tabs = 0;
                self.set_comment(f.doc.as_ref());
                self.record_line(&mut line);
                if !f.names.is_empty() {
                    // named fields
                    self.ident_list(&f.names, false);
                    self.print(&[ws(sep)]);
                    if let Some(ty) = &f.ty {
                        self.expr(ty);
                    }
                    extra_tabs = 1;
                } else {
                    // anonymous / embedded field
                    if let Some(ty) = &f.ty {
                        self.expr(ty);
                    }
                    extra_tabs = 2;
                }
                if let Some(tag) = &f.tag {
                    if !f.names.is_empty() && sep == VTAB {
                        self.print(&[ws(sep)]);
                    }
                    self.print(&[ws(sep)]);
                    self.print(&[Item::Lit(tag)]);
                    extra_tabs = 0;
                }
                if f.comment.is_some() {
                    for _ in 0..extra_tabs {
                        self.print(&[ws(sep)]);
                    }
                    self.set_comment(f.comment.as_ref());
                }
            }
            if incomplete {
                if !fields.list.is_empty() {
                    self.print(&[ws(FORMFEED)]);
                }
                self.flush(self.pos_for(fields.closing), Token::RBRACE);
                self.set_line_comment("// contains filtered or unexported fields");
            }
        } else {
            // interface
            let mut line = 0;
            // previous "type" identifier (Go uses pointer identity for type
            // lists; always cleared below, matching go/printer's dead TODO).
            let mut prev: Option<*const Ident> = None;
            for (i, f) in fields.list.iter().enumerate() {
                let name = f.names.first();
                if i > 0 {
                    // don't do a line break (min == 0) if we are printing a list of types
                    let min = match (prev, name) {
                        (Some(p), Some(n)) if std::ptr::eq(p, n as *const Ident) => 0,
                        _ => 1,
                    };
                    self.linebreak(self.line_for(f.pos()), min, IGNORE, self.lines_from(line) > 0);
                }
                self.set_comment(f.doc.as_ref());
                self.record_line(&mut line);
                if let Some(name) = name {
                    self.set_pos(name.pos());
                    self.print(&[Item::Ident(name)]);
                    if let Some(Expr::FuncType(ty)) = &f.ty {
                        self.signature(ty);
                    } else if let Some(ty) = &f.ty {
                        self.expr(ty);
                    }
                    prev = None;
                } else {
                    if let Some(ty) = &f.ty {
                        self.expr(ty);
                    }
                    prev = None;
                }
                self.set_comment(f.comment.as_ref());
            }
            if incomplete {
                if !fields.list.is_empty() {
                    self.print(&[ws(FORMFEED)]);
                }
                self.flush(self.pos_for(fields.closing), Token::RBRACE);
                self.set_line_comment("// contains filtered or unexported methods");
            }
        }
        self.print(&[ws(UNINDENT), ws(FORMFEED)]);
        self.set_pos(fields.closing);
        self.print(&[tok(Token::RBRACE)]);
    }

    fn binary_expr(&mut self, x: &BinaryExpr, prec1: i32, cutoff: i32, depth: i32) {
        let prec = x.op.precedence();
        if prec < prec1 {
            self.print(&[tok(Token::LPAREN)]);
            self.expr0(&Expr::BinaryExpr(x.clone()), reduce_depth(depth));
            self.print(&[tok(Token::RPAREN)]);
            return;
        }
        let mut blank = prec < cutoff;
        let mut w = INDENT;
        self.expr1(&x.x, prec, depth + diff_prec(&x.x, prec));
        if blank {
            self.print(&[ws(BLANK)]);
        }
        let xline = self.pos.line;
        let yline = self.line_for(x.y.pos());
        self.set_pos(x.op_pos);
        self.print(&[tok(x.op)]);
        if xline != yline && xline > 0 && yline > 0 && self.linebreak(yline, 1, w, true) > 0 {
            w = IGNORE;
            blank = false;
        }
        if blank {
            self.print(&[ws(BLANK)]);
        }
        self.expr1(&x.y, prec + 1, depth + 1);
        if w == IGNORE {
            self.print(&[ws(UNINDENT)]);
        }
    }

    pub(crate) fn expr1(&mut self, x: &Expr, prec1: i32, mut depth: i32) {
        self.set_pos(x.pos());
        match x {
            Expr::BadExpr(_) => self.print(&[text("BadExpr")]),
            Expr::Ident(x) => self.print(&[Item::Ident(x)]),
            Expr::BinaryExpr(x) => {
                if depth < 1 {
                    self.internal_error("depth < 1");
                    depth = 1;
                }
                self.binary_expr(x, prec1, cutoff(x, depth), depth);
            }
            Expr::KeyValueExpr(x) => {
                self.expr(&x.key);
                self.set_pos(x.colon);
                self.print(&[tok(Token::COLON), ws(BLANK)]);
                self.expr(&x.value);
            }
            Expr::StarExpr(x) => {
                if UNARY_PREC < prec1 {
                    self.print(&[tok(Token::LPAREN), tok(Token::MUL)]);
                    self.expr(&x.x);
                    self.print(&[tok(Token::RPAREN)]);
                } else {
                    self.print(&[tok(Token::MUL)]);
                    self.expr(&x.x);
                }
            }
            Expr::UnaryExpr(x) => {
                if UNARY_PREC < prec1 {
                    self.print(&[tok(Token::LPAREN)]);
                    // Re-enter with lowest precedence so the paren branch is not taken again
                    // (matches Go's `p.expr(x)` on the same *UnaryExpr).
                    self.expr(&Expr::UnaryExpr(x.clone()));
                    self.print(&[tok(Token::RPAREN)]);
                } else {
                    self.print(&[tok(x.op)]);
                    if x.op == Token::RANGE {
                        self.print(&[ws(BLANK)]);
                    }
                    self.expr1(&x.x, UNARY_PREC, depth);
                }
            }
            Expr::BasicLit(x) => {
                if self.config.mode & super::NORMALIZE_NUMBERS != 0 {
                    let x = normalized_number(x);
                    self.print(&[Item::Lit(&x)]);
                } else {
                    self.print(&[Item::Lit(x)]);
                }
            }
            Expr::FuncLit(x) => {
                self.set_pos(x.ty.pos());
                self.print(&[tok(Token::FUNC)]);
                let start = self.out.column - 4;
                self.signature(&x.ty);
                self.func_body(self.distance_from(x.ty.pos(), start), BLANK, &x.body);
            }
            Expr::ParenExpr(x) => {
                if matches!(&*x.x, Expr::ParenExpr(_)) {
                    self.expr0(&x.x, depth);
                } else {
                    self.print(&[tok(Token::LPAREN)]);
                    self.expr0(&x.x, reduce_depth(depth));
                    self.set_pos(x.rparen);
                    self.print(&[tok(Token::RPAREN)]);
                }
            }
            Expr::SelectorExpr(x) => {
                self.selector_expr(x, depth, false);
            }
            Expr::TypeAssertExpr(x) => {
                self.expr1(&x.x, HIGHEST_PREC, depth);
                self.print(&[tok(Token::PERIOD)]);
                self.set_pos(x.lparen);
                self.print(&[tok(Token::LPAREN)]);
                if let Some(ty) = &x.ty {
                    self.expr(ty);
                } else {
                    self.print(&[tok(Token::TYPE)]);
                }
                self.set_pos(x.rparen);
                self.print(&[tok(Token::RPAREN)]);
            }
            Expr::IndexExpr(x) => {
                self.expr1(&x.x, HIGHEST_PREC, 1);
                self.set_pos(x.lbrack);
                self.print(&[tok(Token::LBRACK)]);
                self.expr0(&x.index, depth + 1);
                self.set_pos(x.rbrack);
                self.print(&[tok(Token::RBRACK)]);
            }
            Expr::IndexListExpr(x) => {
                self.expr1(&x.x, HIGHEST_PREC, 1);
                self.set_pos(x.lbrack);
                self.print(&[tok(Token::LBRACK)]);
                self.expr_list(
                    x.lbrack,
                    &x.indices,
                    depth + 1,
                    ExprListMode::COMMA_TERM,
                    x.rbrack,
                    false,
                );
                self.set_pos(x.rbrack);
                self.print(&[tok(Token::RBRACK)]);
            }
            Expr::SliceExpr(x) => {
                self.expr1(&x.x, HIGHEST_PREC, 1);
                self.set_pos(x.lbrack);
                self.print(&[tok(Token::LBRACK)]);
                let indices = [x.low.as_deref(), x.high.as_deref(), x.max.as_deref()];
                let needs_blanks = depth <= 1
                    && indices.iter().flatten().count() > 1
                    && indices.iter().flatten().any(|x| is_binary(x));
                for (i, index) in
                    indices
                        .iter()
                        .enumerate()
                        .take(if x.max.is_some() { 3 } else { 2 })
                {
                    if i > 0 {
                        if indices[i - 1].is_some() && needs_blanks {
                            self.print(&[ws(BLANK)]);
                        }
                        self.print(&[tok(Token::COLON)]);
                        if index.is_some() && needs_blanks {
                            self.print(&[ws(BLANK)]);
                        }
                    }
                    if let Some(index) = index {
                        self.expr0(index, depth + 1);
                    }
                }
                self.set_pos(x.rbrack);
                self.print(&[tok(Token::RBRACK)]);
            }
            Expr::CallExpr(x) => {
                if x.args.len() > 1 {
                    depth += 1;
                }
                let paren = matches!(&*x.fun, Expr::FuncType(_))
                    || matches!(&*x.fun, Expr::ChanType(c) if c.dir == ChanDir::RECV);
                if paren {
                    self.print(&[tok(Token::LPAREN)]);
                }
                let indented = self.possible_selector_expr(&x.fun, HIGHEST_PREC, depth);
                if paren {
                    self.print(&[tok(Token::RPAREN)]);
                }
                self.set_pos(x.lparen);
                self.print(&[tok(Token::LPAREN)]);
                if x.ellipsis.is_valid() {
                    self.expr_list(
                        x.lparen,
                        &x.args,
                        depth,
                        ExprListMode::empty(),
                        x.ellipsis,
                        false,
                    );
                    self.set_pos(x.ellipsis);
                    self.print(&[tok(Token::ELLIPSIS)]);
                    if x.rparen.is_valid() && self.line_for(x.ellipsis) < self.line_for(x.rparen) {
                        self.print(&[tok(Token::COMMA), ws(FORMFEED)]);
                    }
                } else {
                    self.expr_list(
                        x.lparen,
                        &x.args,
                        depth,
                        ExprListMode::COMMA_TERM,
                        x.rparen,
                        false,
                    );
                }
                self.set_pos(x.rparen);
                self.print(&[tok(Token::RPAREN)]);
                if indented {
                    self.print(&[ws(UNINDENT)]);
                }
            }
            Expr::CompositeLit(x) => {
                if let Some(ty) = &x.ty {
                    self.expr1(ty, HIGHEST_PREC, depth);
                }
                self.level += 1;
                self.set_pos(x.lbrace);
                self.print(&[tok(Token::LBRACE)]);
                self.expr_list(
                    x.lbrace,
                    &x.elts,
                    1,
                    ExprListMode::COMMA_TERM,
                    x.rbrace,
                    x.incomplete,
                );
                let mut mode = NO_EXTRA_LINEBREAK;
                if !x.elts.is_empty() {
                    mode |= NO_EXTRA_BLANK;
                }
                self.print(&[ws(INDENT), ws(UNINDENT), Item::Mode(mode)]);
                self.set_pos(x.rbrace);
                self.print(&[tok(Token::RBRACE), Item::Mode(mode)]);
                self.level -= 1;
            }
            Expr::Ellipsis(x) => {
                self.print(&[tok(Token::ELLIPSIS)]);
                if let Some(elt) = &x.elt {
                    self.expr(elt);
                }
            }
            Expr::ArrayType(x) => {
                self.print(&[tok(Token::LBRACK)]);
                if let Some(len) = &x.len {
                    self.expr(len);
                }
                self.print(&[tok(Token::RBRACK)]);
                self.expr(&x.elt);
            }
            Expr::StructType(x) => {
                self.print(&[tok(Token::STRUCT)]);
                self.field_list(&x.fields, true, x.incomplete);
            }
            Expr::FuncType(x) => {
                self.print(&[tok(Token::FUNC)]);
                self.signature(x);
            }
            Expr::InterfaceType(x) => {
                self.print(&[tok(Token::INTERFACE)]);
                self.field_list(&x.methods, false, x.incomplete);
            }
            Expr::MapType(x) => {
                self.print(&[tok(Token::MAP), tok(Token::LBRACK)]);
                self.expr(&x.key);
                self.print(&[tok(Token::RBRACK)]);
                self.expr(&x.value);
            }
            Expr::ChanType(x) => {
                if x.dir == ChanDir(ChanDir::SEND.0 | ChanDir::RECV.0) {
                    self.print(&[tok(Token::CHAN)]);
                } else if x.dir == ChanDir::RECV {
                    self.print(&[tok(Token::ARROW), tok(Token::CHAN)]);
                } else {
                    self.print(&[tok(Token::CHAN)]);
                    self.set_pos(x.arrow);
                    self.print(&[tok(Token::ARROW)]);
                }
                self.print(&[ws(BLANK)]);
                self.expr(&x.value);
            }
        }
    }

    pub(crate) fn possible_selector_expr(&mut self, x: &Expr, prec: i32, depth: i32) -> bool {
        if let Expr::SelectorExpr(x) = x {
            self.selector_expr(x, depth, true)
        } else {
            self.expr1(x, prec, depth);
            false
        }
    }

    pub(crate) fn selector_expr(&mut self, x: &SelectorExpr, depth: i32, method: bool) -> bool {
        self.expr1(&x.x, HIGHEST_PREC, depth);
        self.print(&[tok(Token::PERIOD)]);
        let line = self.line_for(x.sel.pos());
        if self.pos.is_valid() && self.pos.line < line {
            self.print(&[ws(INDENT), ws(NEWLINE)]);
            self.set_pos(x.sel.pos());
            self.print(&[Item::Ident(&x.sel)]);
            if !method {
                self.print(&[ws(UNINDENT)]);
            }
            true
        } else {
            self.set_pos(x.sel.pos());
            self.print(&[Item::Ident(&x.sel)]);
            false
        }
    }

    pub(crate) fn expr0(&mut self, x: &Expr, depth: i32) {
        self.expr1(x, LOWEST_PREC, depth);
    }
    pub(crate) fn expr(&mut self, x: &Expr) {
        self.expr1(x, LOWEST_PREC, 1);
    }

    pub(crate) fn stmt_list(&mut self, list: &[Stmt], nindent: i32, next_is_rbrace: bool) {
        if nindent > 0 {
            self.print(&[ws(INDENT)]);
        }
        let mut line = 0;
        let mut i = 0usize;
        for s in list {
            if matches!(s, Stmt::EmptyStmt(_)) {
                continue;
            }
            if !self.output.is_empty() {
                self.linebreak(
                    self.line_for(s.pos()),
                    1,
                    IGNORE,
                    i == 0 || nindent == 0 || self.lines_from(line) > 0,
                );
            }
            self.record_line(&mut line);
            self.stmt(s, next_is_rbrace && i == list.len() - 1);
            let mut labeled = s;
            while let Stmt::LabeledStmt(x) = labeled {
                line += 1;
                labeled = &x.stmt;
            }
            i += 1;
        }
        if nindent > 0 {
            self.print(&[ws(UNINDENT)]);
        }
    }

    pub(crate) fn block(&mut self, x: &BlockStmt, nindent: i32) {
        self.set_pos(x.lbrace);
        self.print(&[tok(Token::LBRACE)]);
        self.stmt_list(&x.list, nindent, true);
        self.linebreak(self.line_for(x.rbrace), 1, IGNORE, true);
        self.set_pos(x.rbrace);
        self.print(&[tok(Token::RBRACE)]);
    }

    pub(crate) fn control_clause(
        &mut self,
        for_stmt: bool,
        init: Option<&Stmt>,
        expr: Option<&Expr>,
        post: Option<&Stmt>,
    ) {
        self.print(&[ws(BLANK)]);
        let mut blank = false;
        if init.is_none() && post.is_none() {
            if let Some(x) = expr {
                self.expr(strip_parens(x));
                blank = true;
            }
        } else {
            if let Some(x) = init {
                self.stmt(x, false);
            }
            self.print(&[tok(Token::SEMICOLON), ws(BLANK)]);
            if let Some(x) = expr {
                self.expr(strip_parens(x));
                blank = true;
            }
            if for_stmt {
                self.print(&[tok(Token::SEMICOLON), ws(BLANK)]);
                blank = false;
                if let Some(x) = post {
                    self.stmt(x, false);
                    blank = true;
                }
            }
        }
        if blank {
            self.print(&[ws(BLANK)]);
        }
    }

    fn indent_list(&self, list: &[Expr]) -> bool {
        if list.len() < 2 {
            return false;
        }
        let begin = self.line_for(list[0].pos());
        let end = self.line_for(list.last().unwrap().end());
        if begin <= 0 || begin >= end {
            return false;
        }
        let mut multiline = 0;
        let mut line = begin;
        for x in list {
            let xb = self.line_for(x.pos());
            let xe = self.line_for(x.end());
            if line < xb {
                return true;
            }
            if xb < xe {
                multiline += 1;
            }
            line = xe;
        }
        multiline > 1
    }

    pub(crate) fn stmt(&mut self, s: &Stmt, next_is_rbrace: bool) {
        self.set_pos(s.pos());
        match s {
            Stmt::BadStmt(_) => self.print(&[text("BadStmt")]),
            Stmt::DeclStmt(x) => self.decl(&x.decl),
            Stmt::EmptyStmt(_) => {}
            Stmt::LabeledStmt(x) => {
                self.print(&[ws(UNINDENT), Item::Ident(&x.label)]);
                self.set_pos(x.colon);
                self.print(&[tok(Token::COLON), ws(INDENT)]);
                if let Stmt::EmptyStmt(empty) = &*x.stmt {
                    if !next_is_rbrace {
                        self.print(&[ws(NEWLINE)]);
                        self.set_pos(empty.semicolon);
                        self.print(&[tok(Token::SEMICOLON)]);
                        return;
                    }
                } else {
                    self.linebreak(self.line_for(x.stmt.pos()), 1, IGNORE, true);
                }
                self.stmt(&x.stmt, next_is_rbrace);
            }
            Stmt::ExprStmt(x) => self.expr0(&x.x, 1),
            Stmt::SendStmt(x) => {
                self.expr0(&x.chan_, 1);
                self.print(&[ws(BLANK)]);
                self.set_pos(x.arrow);
                self.print(&[tok(Token::ARROW), ws(BLANK)]);
                self.expr0(&x.value, 1);
            }
            Stmt::IncDecStmt(x) => {
                self.expr0(&x.x, 2);
                self.set_pos(x.tok_pos);
                self.print(&[tok(x.tok)]);
            }
            Stmt::AssignStmt(x) => {
                let depth = if x.lhs.len() > 1 && x.rhs.len() > 1 {
                    2
                } else {
                    1
                };
                self.expr_list(
                    s.pos(),
                    &x.lhs,
                    depth,
                    ExprListMode::empty(),
                    x.tok_pos,
                    false,
                );
                self.print(&[ws(BLANK)]);
                self.set_pos(x.tok_pos);
                self.print(&[tok(x.tok.unwrap_or(Token::ILLEGAL)), ws(BLANK)]);
                self.expr_list(
                    x.tok_pos,
                    &x.rhs,
                    depth,
                    ExprListMode::empty(),
                    NO_POS,
                    false,
                );
            }
            Stmt::GoStmt(x) => {
                self.print(&[tok(Token::GO), ws(BLANK)]);
                self.expr(&Expr::CallExpr(x.call.clone()));
            }
            Stmt::DeferStmt(x) => {
                self.print(&[tok(Token::DEFER), ws(BLANK)]);
                self.expr(&Expr::CallExpr(x.call.clone()));
            }
            Stmt::ReturnStmt(x) => {
                self.print(&[tok(Token::RETURN)]);
                if !x.results.is_empty() {
                    self.print(&[ws(BLANK)]);
                    if self.indent_list(&x.results) {
                        self.print(&[ws(INDENT)]);
                        self.expr_list(
                            NO_POS,
                            &x.results,
                            1,
                            ExprListMode::NO_INDENT,
                            NO_POS,
                            false,
                        );
                        self.print(&[ws(UNINDENT)]);
                    } else {
                        self.expr_list(NO_POS, &x.results, 1, ExprListMode::empty(), NO_POS, false);
                    }
                }
            }
            Stmt::BranchStmt(x) => {
                self.print(&[tok(x.tok)]);
                if let Some(label) = &x.label {
                    self.print(&[ws(BLANK), Item::Ident(label)]);
                }
            }
            Stmt::BlockStmt(x) => self.block(x, 1),
            Stmt::IfStmt(x) => {
                self.print(&[tok(Token::IF)]);
                self.control_clause(false, x.init.as_deref(), Some(&x.cond), None);
                self.block(&x.body, 1);
                if let Some(else_) = &x.else_ {
                    self.print(&[ws(BLANK), tok(Token::ELSE), ws(BLANK)]);
                    if matches!(&**else_, Stmt::BlockStmt(_) | Stmt::IfStmt(_)) {
                        self.stmt(else_, next_is_rbrace);
                    } else {
                        self.print(&[tok(Token::LBRACE), ws(INDENT), ws(FORMFEED)]);
                        self.stmt(else_, true);
                        self.print(&[ws(UNINDENT), ws(FORMFEED), tok(Token::RBRACE)]);
                    }
                }
            }
            Stmt::CaseClause(x) => {
                if x.list.is_empty() {
                    self.print(&[tok(Token::DEFAULT)]);
                } else {
                    self.print(&[tok(Token::CASE), ws(BLANK)]);
                    self.expr_list(s.pos(), &x.list, 1, ExprListMode::empty(), x.colon, false);
                }
                self.set_pos(x.colon);
                self.print(&[tok(Token::COLON)]);
                self.stmt_list(&x.body, 1, next_is_rbrace);
            }
            Stmt::SwitchStmt(x) => {
                self.print(&[tok(Token::SWITCH)]);
                self.control_clause(false, x.init.as_deref(), x.tag.as_ref(), None);
                self.block(&x.body, 0);
            }
            Stmt::TypeSwitchStmt(x) => {
                self.print(&[tok(Token::SWITCH)]);
                if let Some(init) = &x.init {
                    self.print(&[ws(BLANK)]);
                    self.stmt(init, false);
                    self.print(&[tok(Token::SEMICOLON)]);
                }
                self.print(&[ws(BLANK)]);
                self.stmt(&x.assign, false);
                self.print(&[ws(BLANK)]);
                self.block(&x.body, 0);
            }
            Stmt::CommClause(x) => {
                if let Some(comm) = &x.comm {
                    self.print(&[tok(Token::CASE), ws(BLANK)]);
                    self.stmt(comm, false);
                } else {
                    self.print(&[tok(Token::DEFAULT)]);
                }
                self.set_pos(x.colon);
                self.print(&[tok(Token::COLON)]);
                self.stmt_list(&x.body, 1, next_is_rbrace);
            }
            Stmt::SelectStmt(x) => {
                self.print(&[tok(Token::SELECT), ws(BLANK)]);
                if x.body.list.is_empty() && !self.comment_before(&self.pos_for(x.body.rbrace)) {
                    self.set_pos(x.body.lbrace);
                    self.print(&[tok(Token::LBRACE)]);
                    self.set_pos(x.body.rbrace);
                    self.print(&[tok(Token::RBRACE)]);
                } else {
                    self.block(&x.body, 0);
                }
            }
            Stmt::ForStmt(x) => {
                self.print(&[tok(Token::FOR)]);
                self.control_clause(true, x.init.as_deref(), x.cond.as_ref(), x.post.as_deref());
                self.block(&x.body, 1);
            }
            Stmt::RangeStmt(x) => {
                self.print(&[tok(Token::FOR), ws(BLANK)]);
                if let Some(key) = &x.key {
                    self.expr(key);
                    if let Some(value) = &x.value {
                        self.set_pos(value.pos());
                        self.print(&[tok(Token::COMMA), ws(BLANK)]);
                        self.expr(value);
                    }
                    self.print(&[ws(BLANK)]);
                    self.set_pos(x.tok_pos);
                    self.print(&[tok(x.tok.unwrap_or(Token::ILLEGAL)), ws(BLANK)]);
                }
                self.print(&[tok(Token::RANGE), ws(BLANK)]);
                self.expr(strip_parens(&x.x));
                self.print(&[ws(BLANK)]);
                self.block(&x.body, 1);
            }
        }
    }

    pub(crate) fn value_spec(&mut self, x: &ValueSpec, keep_type: bool) {
        self.set_comment(x.doc.as_ref());
        self.ident_list(&x.names, false);
        let mut extra_tabs = 3;
        if x.ty.is_some() || keep_type {
            self.print(&[ws(VTAB)]);
            extra_tabs -= 1;
        }
        if let Some(ty) = &x.ty {
            self.expr(ty);
        }
        if !x.values.is_empty() {
            self.print(&[ws(VTAB), tok(Token::ASSIGN), ws(BLANK)]);
            self.expr_list(NO_POS, &x.values, 1, ExprListMode::empty(), NO_POS, false);
            extra_tabs -= 1;
        }
        if let Some(comment) = &x.comment {
            for _ in 0..extra_tabs {
                self.print(&[ws(VTAB)]);
            }
            self.set_comment(Some(comment));
        }
    }

    pub(crate) fn spec(&mut self, x: &Spec, n: usize, do_indent: bool) {
        match x {
            Spec::ImportSpec(x) => {
                self.set_comment(x.doc.as_ref());
                if let Some(name) = &x.name {
                    self.set_pos(name.pos());
                    self.print(&[Item::Ident(name), ws(BLANK)]);
                }
                self.set_pos(x.path.pos());
                self.print(&[Item::Lit(&x.path)]);
                self.set_comment(x.comment.as_ref());
                self.set_pos(x.end_pos);
            }
            Spec::ValueSpec(x) => {
                if n != 1 {
                    self.internal_error("expected n = 1");
                }
                self.set_comment(x.doc.as_ref());
                self.ident_list(&x.names, do_indent);
                if let Some(ty) = &x.ty {
                    self.print(&[ws(BLANK)]);
                    self.expr(ty);
                }
                if !x.values.is_empty() {
                    self.print(&[ws(BLANK), tok(Token::ASSIGN), ws(BLANK)]);
                    self.expr_list(NO_POS, &x.values, 1, ExprListMode::empty(), NO_POS, false);
                }
                self.set_comment(x.comment.as_ref());
            }
            Spec::TypeSpec(x) => {
                self.set_comment(x.doc.as_ref());
                self.set_pos(x.name.pos());
                self.print(&[Item::Ident(&x.name)]);
                if let Some(params) = &x.type_params {
                    self.parameters(params, ParamMode::TYPE_TPARAM);
                }
                self.print(&[ws(if n == 1 { BLANK } else { VTAB })]);
                if x.assign.is_valid() {
                    self.print(&[tok(Token::ASSIGN), ws(BLANK)]);
                }
                self.expr(&x.ty);
                self.set_comment(x.comment.as_ref());
            }
        }
    }

    pub(crate) fn gen_decl(&mut self, x: &GenDecl) {
        self.set_comment(x.doc.as_ref());
        self.set_pos(x.tok_pos);
        let decl_tok = x.tok.unwrap_or(Token::ILLEGAL);
        self.print(&[tok(decl_tok), ws(BLANK)]);
        if x.lparen.is_valid() || x.specs.len() != 1 {
            self.set_pos(x.lparen);
            self.print(&[tok(Token::LPAREN)]);
            if !x.specs.is_empty() {
                self.print(&[ws(INDENT), ws(FORMFEED)]);
                let mut line = 0;
                let keep = keep_type_column(&x.specs);
                for (i, spec) in x.specs.iter().enumerate() {
                    if i > 0 {
                        self.linebreak(
                            self.line_for(spec.pos()),
                            1,
                            IGNORE,
                            self.lines_from(line) > 0,
                        );
                    }
                    self.record_line(&mut line);
                    if x.specs.len() > 1 && matches!(decl_tok, Token::CONST | Token::VAR) {
                        if let Spec::ValueSpec(spec) = spec {
                            self.value_spec(spec, keep[i]);
                        } else {
                            self.spec(spec, x.specs.len(), false);
                        }
                    } else {
                        self.spec(spec, x.specs.len(), false);
                    }
                }
                self.print(&[ws(UNINDENT), ws(FORMFEED)]);
            }
            self.set_pos(x.rparen);
            self.print(&[tok(Token::RPAREN)]);
        } else if let Some(spec) = x.specs.first() {
            self.spec(spec, 1, true);
        }
    }

    pub(crate) fn func_body(&mut self, header_size: usize, sep: WhiteSpace, body: &BlockStmt) {
        let level = self.level;
        self.level = 0;
        if header_size + self.body_size(body, 100) <= 100 {
            self.print(&[ws(sep)]);
            self.set_pos(body.lbrace);
            self.print(&[tok(Token::LBRACE)]);
            if !body.list.is_empty() {
                self.print(&[ws(BLANK)]);
                for (i, s) in body.list.iter().enumerate() {
                    if i > 0 {
                        self.print(&[tok(Token::SEMICOLON), ws(BLANK)]);
                    }
                    self.stmt(s, i == body.list.len() - 1);
                }
                self.print(&[ws(BLANK)]);
            }
            self.print(&[Item::Mode(NO_EXTRA_LINEBREAK)]);
            self.set_pos(body.rbrace);
            self.print(&[tok(Token::RBRACE), Item::Mode(NO_EXTRA_LINEBREAK)]);
        } else {
            if sep != IGNORE {
                self.print(&[ws(BLANK)]);
            }
            self.block(body, 1);
        }
        self.level = level;
    }

    pub(crate) fn func_decl(&mut self, x: &FuncDecl) {
        self.set_comment(x.doc.as_ref());
        self.set_pos(x.ty.pos());
        self.print(&[tok(Token::FUNC), ws(BLANK)]);
        let start = self.out.column - 5;
        if let Some(recv) = &x.recv {
            self.parameters(recv, ParamMode::FUNC_PARAM);
            self.print(&[ws(BLANK)]);
        }
        self.print(&[Item::Ident(&x.name)]);
        self.signature(&x.ty);
        if let Some(body) = &x.body {
            self.func_body(self.distance_from(x.ty.pos(), start), VTAB, body);
        }
    }

    pub(crate) fn decl(&mut self, x: &Decl) {
        match x {
            Decl::BadDecl(x) => {
                self.set_pos(x.from);
                self.print(&[text("BadDecl")]);
            }
            Decl::GenDecl(x) => self.gen_decl(x),
            Decl::FuncDecl(x) => self.func_decl(x),
        }
    }

    pub(crate) fn decl_list(&mut self, list: &[Decl]) {
        let mut previous = Token::ILLEGAL;
        for x in list {
            let current = decl_token(x);
            if !self.output.is_empty() {
                let min = if previous != current || get_doc(x).is_some() {
                    2
                } else {
                    1
                };
                self.linebreak(
                    self.line_for(x.pos()),
                    min,
                    IGNORE,
                    current == Token::FUNC && self.num_lines(x) > 1,
                );
            }
            self.decl(x);
            previous = current;
        }
    }

    pub(crate) fn file(&mut self, src: &File) {
        self.set_comment(src.doc.as_ref());
        self.set_pos(src.pos());
        self.print(&[tok(Token::PACKAGE), ws(BLANK), Item::Ident(&src.name)]);
        self.decl_list(&src.decls);
        self.print(&[ws(NEWLINE)]);
    }
}

fn keep_type_column(specs: &[Spec]) -> Vec<bool> {
    let mut result = vec![false; specs.len()];
    let mut begin = None;
    let mut keep = false;
    for (i, spec) in specs.iter().enumerate() {
        let (values, ty) = match spec {
            Spec::ValueSpec(x) => (!x.values.is_empty(), x.ty.is_some()),
            _ => (false, false),
        };
        if values && begin.is_none() {
            begin = Some(i);
            keep = false;
        }
        if !values {
            if let Some(start) = begin.take() {
                if keep {
                    result[start..i].fill(true);
                }
            }
        }
        if ty {
            keep = true;
        }
    }
    if let Some(start) = begin {
        if keep {
            result[start..].fill(true);
        }
    }
    result
}

fn decl_token(x: &Decl) -> Token {
    match x {
        Decl::GenDecl(x) => x.tok.unwrap_or(Token::ILLEGAL),
        Decl::FuncDecl(_) => Token::FUNC,
        Decl::BadDecl(_) => Token::ILLEGAL,
    }
}

fn get_doc(x: &Decl) -> Option<&CommentGroup> {
    match x {
        Decl::GenDecl(x) => x.doc.as_ref(),
        Decl::FuncDecl(x) => x.doc.as_ref(),
        Decl::BadDecl(_) => None,
    }
}
