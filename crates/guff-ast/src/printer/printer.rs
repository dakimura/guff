// Port of Go's go/printer/printer.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Layout mirrors the Go file. Node-printing methods live in `nodes.rs`
// as additional `impl Printer` blocks.

use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

use crate::ast::{
    BasicLit, Comment, CommentGroup, Decl, Expr, Field, File, FuncDecl, GenDecl, Ident,
    ImportSpec, Spec, Stmt, TypeSpec, ValueSpec,
};
use crate::constraint;
use crate::position::{FileSet, Pos, Position};
use crate::tabwriter::{self, Writer as TabWriter};
use crate::token::Token;

use super::format_doc_comment;

pub(crate) const MAX_NEWLINES: i64 = 2;
pub(crate) const DEBUG: bool = false;
pub(crate) const INFINITY: i32 = 1 << 30;

/// Delayed whitespace / formatting control character.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WhiteSpace(pub u8);

pub(crate) const IGNORE: WhiteSpace = WhiteSpace(0);
pub(crate) const BLANK: WhiteSpace = WhiteSpace(b' ');
pub(crate) const VTAB: WhiteSpace = WhiteSpace(b'\x0b');
pub(crate) const NEWLINE: WhiteSpace = WhiteSpace(b'\n');
pub(crate) const FORMFEED: WhiteSpace = WhiteSpace(b'\x0c');
pub(crate) const INDENT: WhiteSpace = WhiteSpace(b'>');
pub(crate) const UNINDENT: WhiteSpace = WhiteSpace(b'<');

/// Extra printer mode toggled around composite-literal closers, etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PMode(pub u32);

pub(crate) const NO_EXTRA_BLANK: PMode = PMode(1 << 0);
pub(crate) const NO_EXTRA_LINEBREAK: PMode = PMode(1 << 1);

impl std::ops::BitOr for PMode {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        PMode(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for PMode {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
impl std::ops::BitAnd for PMode {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self {
        PMode(self.0 & rhs.0)
    }
}
impl std::ops::BitXorAssign for PMode {
    fn bitxor_assign(&mut self, rhs: Self) {
        self.0 ^= rhs.0;
    }
}

/// Argument to [`Printer::print`] — Go's variadic `print(args ...any)`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Item<'a> {
    Mode(PMode),
    Ws(WhiteSpace),
    Ident(&'a Ident),
    Lit(&'a BasicLit),
    Tok(Token),
    Str(&'a str),
}

impl From<PMode> for Item<'_> {
    fn from(m: PMode) -> Self {
        Item::Mode(m)
    }
}
impl From<WhiteSpace> for Item<'_> {
    fn from(w: WhiteSpace) -> Self {
        Item::Ws(w)
    }
}
impl From<Token> for Item<'_> {
    fn from(t: Token) -> Self {
        Item::Tok(t)
    }
}
impl<'a> From<&'a Ident> for Item<'a> {
    fn from(i: &'a Ident) -> Self {
        Item::Ident(i)
    }
}
impl<'a> From<&'a BasicLit> for Item<'a> {
    fn from(l: &'a BasicLit) -> Self {
        Item::Lit(l)
    }
}
impl<'a> From<&'a str> for Item<'a> {
    fn from(s: &'a str) -> Self {
        Item::Str(s)
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct CommentInfo {
    cindex: usize,
    /// Index into `comments` of the current group, or None.
    comment_idx: Option<usize>,
    comment_offset: i32,
    comment_newline: bool,
}

/// Cache key for `node_sizes`. Uses source span + a type tag so we never
/// rely on pointer identity of cloned AST values.
#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub(crate) struct SizeKey {
    pub tag: u8,
    pub pos: i64,
    pub end: i64,
}

pub(crate) mod size_tag {
    pub const EXPR: u8 = 1;
    pub const STMT: u8 = 2;
    pub const DECL: u8 = 3;
    pub const SPEC: u8 = 4;
    pub const FIELD: u8 = 5;
    pub const BLOCK: u8 = 6;
    pub const IDENT: u8 = 7;
    pub const FILE: u8 = 8;
}

/// Public printer mode flags (Go `printer.Mode`).
pub type Mode = u32;

pub const RAW_FORMAT: Mode = 1 << 0;
pub const TAB_INDENT: Mode = 1 << 1;
pub const USE_SPACES: Mode = 1 << 2;
pub const SOURCE_POS: Mode = 1 << 3;
/// Canonicalize number literal prefixes/exponents (gofmt / go/format).
pub const NORMALIZE_NUMBERS: Mode = 1 << 30;

/// Configuration for [`fprint`] / [`Config::fprint`].
#[derive(Clone, Debug)]
pub struct Config {
    pub mode: Mode,
    pub tabwidth: i32,
    pub indent: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: 0,
            tabwidth: 8,
            indent: 0,
        }
    }
}

/// Bundle an AST node with an explicit comment list (Go `CommentedNode`).
pub struct CommentedNode<'a> {
    pub node: Box<PrintNode<'a>>,
    pub comments: &'a [CommentGroup],
}

/// What [`Config::fprint`] can print.
pub enum PrintNode<'a> {
    File(&'a File),
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Decl(&'a Decl),
    Spec(&'a Spec),
    Stmts(&'a [Stmt]),
    Decls(&'a [Decl]),
    Commented(Box<CommentedNode<'a>>),
}

/// Flags for [`Printer::expr_list`] (Go `exprListMode`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExprListMode(u8);

impl ExprListMode {
    pub const COMMA_TERM: ExprListMode = ExprListMode(1 << 0);
    pub const NO_INDENT: ExprListMode = ExprListMode(1 << 1);

    pub const fn empty() -> Self {
        ExprListMode(0)
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for ExprListMode {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        ExprListMode(self.0 | rhs.0)
    }
}

/// How [`Printer::parameters`] should print a field list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamMode {
    FuncParam,
    FuncTParam,
    TypeTParam,
}

impl ParamMode {
    pub const FUNC_PARAM: ParamMode = ParamMode::FuncParam;
    pub const FUNC_TPARAM: ParamMode = ParamMode::FuncTParam;
    pub const TYPE_TPARAM: ParamMode = ParamMode::TypeTParam;
}

/// The printer state machine (Go `printer` struct).
pub struct Printer<'a> {
    pub(crate) config: Config,
    pub(crate) fset: &'a Arc<FileSet>,

    pub(crate) output: Vec<u8>,
    pub(crate) indent: i32,
    pub(crate) level: i32,
    pub(crate) mode: PMode,
    pub(crate) end_alignment: bool,
    pub(crate) implied_semi: bool,
    pub(crate) last_tok: Token,
    pub(crate) prev_open: Token,
    pub(crate) wsbuf: Vec<WhiteSpace>,
    pub(crate) go_build: Vec<usize>,
    pub(crate) plus_build: Vec<usize>,

    pub(crate) pos: Position,
    pub(crate) out: Position,
    pub(crate) last: Position,
    pub(crate) line_ptr: Option<*mut i64>,
    pub(crate) source_pos_err: Option<String>,

    /// All source comments for this print (owned copies — avoids tying
    /// comment lifetimes to the AST borrow beyond the print call, and
    /// lets `set_comment` install synthetic groups).
    pub(crate) comments: Vec<CommentGroup>,
    pub(crate) use_node_comments: bool,

    comment: CommentInfo,

    pub(crate) node_sizes: HashMap<SizeKey, i32>,

    cached_pos: Pos,
    cached_line: i64,
}

impl<'a> Printer<'a> {
    pub(crate) fn new(cfg: Config, fset: &'a Arc<FileSet>, node_sizes: HashMap<SizeKey, i32>) -> Self {
        Self {
            config: cfg,
            fset,
            output: Vec::with_capacity(16 << 10),
            indent: 0,
            level: 0,
            mode: PMode(0),
            end_alignment: false,
            implied_semi: false,
            last_tok: Token::ILLEGAL,
            prev_open: Token::ILLEGAL,
            wsbuf: Vec::with_capacity(16),
            go_build: Vec::new(),
            plus_build: Vec::new(),
            pos: Position {
                line: 1,
                column: 1,
                ..Default::default()
            },
            out: Position {
                line: 1,
                column: 1,
                ..Default::default()
            },
            last: Position::default(),
            line_ptr: None,
            source_pos_err: None,
            comments: Vec::new(),
            use_node_comments: false,
            comment: CommentInfo {
                comment_offset: INFINITY,
                ..Default::default()
            },
            node_sizes,
            cached_pos: Pos(-1),
            cached_line: 0,
        }
    }

    pub(crate) fn internal_error(&self, msg: &str) {
        if DEBUG {
            eprintln!("{}: {}", self.pos, msg);
            panic!("go/printer");
        }
    }

    fn comments_have_newline(&self, list: &[Comment]) -> bool {
        if list.is_empty() {
            return false;
        }
        let line = self.line_for(list[0].pos());
        for (i, c) in list.iter().enumerate() {
            if i > 0 && self.line_for(list[i].pos()) != line {
                return true;
            }
            let t = c.text.as_str();
            if t.len() >= 2 && (t.as_bytes()[1] == b'/' || t.contains('\n')) {
                return true;
            }
        }
        false
    }

    pub(crate) fn next_comment(&mut self) {
        while self.comment.cindex < self.comments.len() {
            let idx = self.comment.cindex;
            self.comment.cindex += 1;
            if !self.comments[idx].list.is_empty() {
                self.comment.comment_idx = Some(idx);
                self.comment.comment_offset =
                    self.pos_for(self.comments[idx].list[0].pos()).offset as i32;
                self.comment.comment_newline =
                    self.comments_have_newline(&self.comments[idx].list);
                return;
            }
        }
        self.comment.comment_idx = None;
        self.comment.comment_offset = INFINITY;
    }

    fn current_comment(&self) -> Option<&CommentGroup> {
        let idx = self.comment.comment_idx?;
        self.comments.get(idx)
    }

    pub(crate) fn comment_before(&self, next: &Position) -> bool {
        self.comment.comment_offset < next.offset as i32
            && (!self.implied_semi || !self.comment.comment_newline)
    }

    pub(crate) fn comment_size_before(&mut self, next: Position) -> i32 {
        let saved = self.comment;
        let mut size = 0i32;
        while self.comment_before(&next) {
            if let Some(c) = self.current_comment() {
                for cm in &c.list {
                    size += cm.text.len() as i32;
                }
            }
            self.next_comment();
        }
        self.comment = saved;
        size
    }

    pub(crate) fn record_line(&mut self, line_ptr: &mut i64) {
        self.line_ptr = Some(line_ptr as *mut i64);
    }

    pub(crate) fn lines_from(&self, line: i64) -> i64 {
        self.out.line - line
    }

    /// Print as many newlines as necessary (but at least `min`) to reach `line`.
    pub(crate) fn linebreak(
        &mut self,
        line: i64,
        min: i64,
        ws: WhiteSpace,
        new_section: bool,
    ) -> i64 {
        let mut n = nlimit(line - self.pos.line).max(min);
        let mut nbreaks = 0i64;
        if n > 0 {
            self.print(&[Item::Ws(ws)]);
            if new_section {
                self.print(&[Item::Ws(FORMFEED)]);
                n -= 1;
                nbreaks = 2;
            }
            nbreaks += n;
            while n > 0 {
                self.print(&[Item::Ws(NEWLINE)]);
                n -= 1;
            }
        }
        nbreaks
    }

    pub(crate) fn set_line_comment(&mut self, text: &'static str) {
        self.set_comment(Some(&CommentGroup {
            list: vec![Comment {
                slash: crate::NO_POS,
                text: text.to_string(),
            }],
        }));
    }

    pub(crate) fn distance_from(&self, start_pos: Pos, start_out_col: i64) -> usize {
        if start_pos.is_valid()
            && self.pos.is_valid()
            && self.pos_for(start_pos).line == self.pos.line
        {
            (self.out.column - start_out_col).max(0) as usize
        } else {
            INFINITY as usize
        }
    }

    pub(crate) fn num_lines(&self, n: &Decl) -> i64 {
        let from = n.pos();
        if from.is_valid() {
            let to = n.end();
            if to.is_valid() {
                return self.line_for(to) - self.line_for(from) + 1;
            }
        }
        INFINITY as i64
    }

    pub(crate) fn node_size(&mut self, n: &Expr, max_size: usize) -> usize {
        let key = SizeKey {
            tag: size_tag::EXPR,
            pos: n.pos().0,
            end: n.end().0,
        };
        if let Some(&size) = self.node_sizes.get(&key) {
            return size as usize;
        }
        // Assume it doesn't fit until proven otherwise (breaks recursion).
        self.node_sizes.insert(key, (max_size + 1) as i32);
        let cfg = Config {
            mode: RAW_FORMAT,
            tabwidth: self.config.tabwidth,
            indent: 0,
        };
        // Move the memo map into the sub-printer (Go shares it by reference) and
        // move the enriched map back — nested sizes computed here are retained,
        // and there is no per-call clone of the (potentially large) map.
        let sizes = std::mem::take(&mut self.node_sizes);
        let mut counter = SizeCounter {
            size: 0,
            has_newline: false,
        };
        if let Ok(sizes) = cfg.fprint_with_sizes(&mut counter, self.fset, PrintNode::Expr(n), sizes)
        {
            self.node_sizes = sizes;
        }
        if counter.size as usize <= max_size && !counter.has_newline {
            self.node_sizes.insert(key, counter.size);
            counter.size as usize
        } else {
            max_size + 1
        }
    }

    pub(crate) fn node_size_stmt(&mut self, n: &Stmt, max_size: usize) -> usize {
        let key = SizeKey {
            tag: size_tag::STMT,
            pos: n.pos().0,
            end: n.end().0,
        };
        if let Some(&size) = self.node_sizes.get(&key) {
            return size as usize;
        }
        self.node_sizes.insert(key, (max_size + 1) as i32);
        let cfg = Config {
            mode: RAW_FORMAT,
            tabwidth: self.config.tabwidth,
            indent: 0,
        };
        let sizes = std::mem::take(&mut self.node_sizes);
        let mut counter = SizeCounter {
            size: 0,
            has_newline: false,
        };
        if let Ok(sizes) = cfg.fprint_with_sizes(&mut counter, self.fset, PrintNode::Stmt(n), sizes)
        {
            self.node_sizes = sizes;
        }
        if counter.size as usize <= max_size && !counter.has_newline {
            self.node_sizes.insert(key, counter.size);
            counter.size as usize
        } else {
            max_size + 1
        }
    }

    pub(crate) fn body_size(&mut self, b: &crate::ast::BlockStmt, max_size: usize) -> usize {
        let pos1 = b.pos();
        let pos2 = b.rbrace;
        if pos1.is_valid() && pos2.is_valid() && self.line_for(pos1) != self.line_for(pos2) {
            return max_size + 1;
        }
        if b.list.len() > 5 {
            return max_size + 1;
        }
        let mut body_size = self.comment_size_before(self.pos_for(pos2)) as usize;
        for (i, s) in b.list.iter().enumerate() {
            if body_size > max_size {
                break;
            }
            if i > 0 {
                body_size += 2;
            }
            body_size += self.node_size_stmt(s, max_size);
        }
        body_size
    }

    pub(crate) fn pos_for(&self, pos: Pos) -> Position {
        self.fset.position_for(pos, false)
    }

    pub(crate) fn line_for(&self, pos: Pos) -> i64 {
        // cached — need interior mutability; use cell-like update via raw
        // We can't mutate through &self; use a simplified uncached path for
        // &self callers and cached for &mut self.
        self.fset.line_for(pos, false)
    }

    pub(crate) fn line_for_mut(&mut self, pos: Pos) -> i64 {
        if pos != self.cached_pos {
            self.cached_pos = pos;
            self.cached_line = self.fset.line_for(pos, false);
        }
        self.cached_line
    }

    fn write_line_directive(&mut self, pos: &Position) {
        if pos.is_valid()
            && (self.out.line != pos.line || self.out.filename != pos.filename)
        {
            if pos.filename.contains('\r') || pos.filename.contains('\n') {
                if self.source_pos_err.is_none() {
                    self.source_pos_err = Some(format!(
                        "go/printer: source filename contains unexpected newline character: {:?}",
                        pos.filename
                    ));
                }
                return;
            }
            self.output.push(tabwriter::ESCAPE);
            let line = format!("//line {}:{}", pos.filename, pos.line);
            self.output.extend_from_slice(line.as_bytes());
            self.output.push(b'\n');
            self.output.push(tabwriter::ESCAPE);
            self.out.filename = pos.filename.clone();
            self.out.line = pos.line;
        }
    }

    fn write_indent(&mut self) {
        let n = self.config.indent + self.indent;
        for _ in 0..n {
            self.output.push(b'\t');
        }
        self.pos.offset += n as i64;
        self.pos.column += n as i64;
        self.out.column += n as i64;
    }

    pub(crate) fn write_byte(&mut self, mut ch: u8, n: i32) {
        if self.end_alignment {
            match ch {
                b'\t' | b'\x0b' => ch = b' ',
                b'\n' | b'\x0c' => {
                    ch = b'\x0c';
                    self.end_alignment = false;
                }
                _ => {}
            }
        }
        if self.out.column == 1 {
            self.write_indent();
        }
        for _ in 0..n {
            self.output.push(ch);
        }
        self.pos.offset += n as i64;
        if ch == b'\n' || ch == b'\x0c' {
            self.pos.line += n as i64;
            self.out.line += n as i64;
            self.pos.column = 1;
            self.out.column = 1;
            return;
        }
        self.pos.column += n as i64;
        self.out.column += n as i64;
    }

    pub(crate) fn write_string(&mut self, pos: Position, s: &str, is_lit: bool) {
        if self.out.column == 1 {
            if self.config.mode & SOURCE_POS != 0 {
                self.write_line_directive(&pos);
            }
            self.write_indent();
        }
        if pos.is_valid() {
            self.pos = pos;
        }
        if is_lit {
            self.output.push(tabwriter::ESCAPE);
        }
        self.output.extend_from_slice(s.as_bytes());

        let mut nlines = 0i64;
        let mut li = 0usize;
        for (i, &ch) in s.as_bytes().iter().enumerate() {
            if ch == b'\n' || ch == b'\x0c' {
                nlines += 1;
                li = i;
                self.end_alignment = true;
            }
        }
        self.pos.offset += s.len() as i64;
        if nlines > 0 {
            self.pos.line += nlines;
            self.out.line += nlines;
            let c = (s.len() - li) as i64;
            self.pos.column = c;
            self.out.column = c;
        } else {
            self.pos.column += s.len() as i64;
            self.out.column += s.len() as i64;
        }
        if is_lit {
            self.output.push(tabwriter::ESCAPE);
        }
        self.last = self.pos.clone();
    }

    fn write_comment_prefix(
        &mut self,
        pos: Position,
        next: Position,
        prev: Option<&Comment>,
        tok: Token,
    ) {
        if self.output.is_empty() {
            return;
        }
        if pos.is_valid() && pos.filename != self.last.filename {
            self.write_byte(b'\x0c', MAX_NEWLINES as i32);
            return;
        }

        if pos.line == self.last.line
            && (prev.is_none() || prev.unwrap().text.as_bytes().get(1) != Some(&b'/'))
        {
            let mut has_sep = false;
            if prev.is_none() {
                let mut j = 0usize;
                for i in 0..self.wsbuf.len() {
                    match self.wsbuf[i] {
                        BLANK => {
                            self.wsbuf[i] = IGNORE;
                            continue;
                        }
                        VTAB => {
                            has_sep = true;
                            continue;
                        }
                        INDENT => continue,
                        _ => {
                            j = i;
                            break;
                        }
                    }
                }
                self.write_whitespace(j);
            }
            if !has_sep {
                let mut sep = b'\t';
                if pos.line == next.line {
                    sep = b' ';
                }
                self.write_byte(sep, 1);
            }
        } else {
            let mut dropped_linebreak = false;
            let mut j = 0usize;
            for i in 0..self.wsbuf.len() {
                match self.wsbuf[i] {
                    BLANK | VTAB => {
                        self.wsbuf[i] = IGNORE;
                        continue;
                    }
                    INDENT => continue,
                    UNINDENT => {
                        if i + 1 < self.wsbuf.len() && self.wsbuf[i + 1] == UNINDENT {
                            continue;
                        }
                        if tok != Token::RBRACE && pos.column == next.column {
                            continue;
                        }
                    }
                    NEWLINE | FORMFEED => {
                        self.wsbuf[i] = IGNORE;
                        dropped_linebreak = prev.is_none();
                    }
                    _ => {}
                }
                j = i;
                break;
            }
            self.write_whitespace(j);

            let mut n = 0i64;
            if pos.is_valid() && self.last.is_valid() {
                n = pos.line - self.last.line;
                if n < 0 {
                    n = 0;
                }
            }
            if self.indent == 0 && dropped_linebreak {
                n += 1;
            }
            if n == 0 && prev.is_some() && prev.unwrap().text.as_bytes().get(1) == Some(&b'/') {
                n = 1;
            }
            if n > 0 {
                self.write_byte(b'\x0c', nlimit(n) as i32);
            }
        }
    }

    fn write_comment(&mut self, comment: &Comment) {
        let text = comment.text.as_str();
        let mut pos = self.pos_for(comment.pos());

        const LINE_PREFIX: &str = "//line ";
        let saved_indent = if text.starts_with(LINE_PREFIX) && (!pos.is_valid() || pos.column == 1) {
            let ind = self.indent;
            self.indent = 0;
            Some(ind)
        } else {
            None
        };

        if text.as_bytes().get(1) == Some(&b'/') {
            if constraint::is_go_build(text) {
                self.go_build.push(self.output.len());
            } else if constraint::is_plus_build(text) {
                self.plus_build.push(self.output.len());
            }
            self.write_string(pos, trim_right(text), true);
            if let Some(ind) = saved_indent {
                self.indent = ind;
            }
            return;
        }

        let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        if pos.is_valid() && pos.column == 1 && self.indent > 0 {
            for line in lines.iter_mut().skip(1) {
                *line = format!("   {line}");
            }
        }
        strip_common_prefix(&mut lines);
        for (i, line) in lines.iter().enumerate() {
            if i > 0 {
                self.write_byte(b'\x0c', 1);
                pos = self.pos.clone();
            }
            if !line.is_empty() {
                self.write_string(pos.clone(), trim_right(line), true);
            }
        }
        if let Some(ind) = saved_indent {
            self.indent = ind;
        }
    }

    fn write_comment_suffix(&mut self, mut needs_linebreak: bool) -> (bool, bool) {
        let mut wrote_newline = false;
        let mut dropped_ff = false;
        for i in 0..self.wsbuf.len() {
            match self.wsbuf[i] {
                BLANK | VTAB => self.wsbuf[i] = IGNORE,
                INDENT | UNINDENT => {}
                NEWLINE | FORMFEED => {
                    if needs_linebreak {
                        needs_linebreak = false;
                        wrote_newline = true;
                    } else {
                        if self.wsbuf[i] == FORMFEED {
                            dropped_ff = true;
                        }
                        self.wsbuf[i] = IGNORE;
                    }
                }
                _ => {}
            }
        }
        let n = self.wsbuf.len();
        self.write_whitespace(n);
        if needs_linebreak {
            self.write_byte(b'\n', 1);
            wrote_newline = true;
        }
        (wrote_newline, dropped_ff)
    }

    fn contains_linebreak(&self) -> bool {
        self.wsbuf.iter().any(|ch| *ch == NEWLINE || *ch == FORMFEED)
    }

    fn intersperse_comments(&mut self, next: Position, tok: Token) -> (bool, bool) {
        let mut last: Option<Comment> = None;
        while self.comment_before(&next) {
            let Some(group) = self.current_comment().cloned() else {
                break;
            };
            let mut list = group.list.clone();
            let mut changed = false;
            let group_pos = self.pos_for(group.pos());
            let group_end_plus = Pos(group.end().0 + 1);
            if self.last_tok != Token::IMPORT
                && group_pos.column == 1
                && self.pos_for(group_end_plus) == next
            {
                list = format_doc_comment(&list);
                changed = true;
                if !group.list.is_empty() && list.is_empty() {
                    self.write_comment_prefix(
                        self.pos_for(group.pos()),
                        next.clone(),
                        last.as_ref(),
                        tok,
                    );
                    self.pos = next.clone();
                    self.last = next.clone();
                    self.next_comment();
                    return self.write_comment_suffix(false);
                }
            }
            for c in &list {
                self.write_comment_prefix(self.pos_for(c.pos()), next.clone(), last.as_ref(), tok);
                self.write_comment(c);
                last = Some(c.clone());
            }
            if !group.list.is_empty() && changed {
                let last_c = group.list.last().unwrap();
                self.pos = self.pos_for(last_c.end());
                self.last = self.pos.clone();
            }
            self.next_comment();
        }

        if let Some(ref last_c) = last {
            let mut needs_linebreak = false;
            if self.mode & NO_EXTRA_BLANK == PMode(0)
                && last_c.text.as_bytes().get(1) == Some(&b'*')
                && self.line_for(last_c.pos()) == next.line
                && tok != Token::COMMA
                && (tok != Token::RPAREN || self.prev_open == Token::LPAREN)
                && (tok != Token::RBRACK || self.prev_open == Token::LBRACK)
            {
                if self.contains_linebreak()
                    && self.mode & NO_EXTRA_LINEBREAK == PMode(0)
                    && self.level == 0
                {
                    needs_linebreak = true;
                } else {
                    self.write_byte(b' ', 1);
                }
            }
            if last_c.text.as_bytes().get(1) == Some(&b'/')
                || tok == Token::EOF
                || (tok == Token::RBRACE && self.mode & NO_EXTRA_LINEBREAK == PMode(0))
            {
                needs_linebreak = true;
            }
            return self.write_comment_suffix(needs_linebreak);
        }
        self.internal_error("intersperseComments called without pending comments");
        (false, false)
    }

    pub(crate) fn write_whitespace(&mut self, n: usize) {
        let mut i = 0usize;
        while i < n {
            match self.wsbuf[i] {
                IGNORE => {}
                INDENT => self.indent += 1,
                UNINDENT => {
                    self.indent -= 1;
                    if self.indent < 0 {
                        self.internal_error("negative indentation");
                        self.indent = 0;
                    }
                }
                NEWLINE | FORMFEED => {
                    if i + 1 < n && self.wsbuf[i + 1] == UNINDENT {
                        self.wsbuf[i] = UNINDENT;
                        self.wsbuf[i + 1] = FORMFEED;
                        continue;
                    }
                    self.write_byte(self.wsbuf[i].0, 1);
                }
                other => self.write_byte(other.0, 1),
            }
            i += 1;
        }
        self.wsbuf.drain(..n);
    }

    pub(crate) fn set_pos(&mut self, pos: Pos) {
        if pos.is_valid() {
            self.pos = self.pos_for(pos);
        }
    }

    pub(crate) fn print(&mut self, args: &[Item<'_>]) {
        for arg in args {
            let (data, is_lit, mut implied_semi): (String, bool, bool);
            match self.last_tok {
                Token::ILLEGAL => {}
                Token::LPAREN | Token::LBRACK => self.prev_open = self.last_tok,
                _ => self.prev_open = Token::ILLEGAL,
            }

            match *arg {
                Item::Mode(m) => {
                    self.mode ^= m;
                    continue;
                }
                Item::Ws(x) => {
                    if x == IGNORE {
                        continue;
                    }
                    if self.wsbuf.len() == self.wsbuf.capacity() && self.wsbuf.capacity() > 0 {
                        let i = self.wsbuf.len();
                        self.write_whitespace(i);
                    }
                    self.wsbuf.push(x);
                    if x == NEWLINE || x == FORMFEED {
                        self.implied_semi = false;
                    }
                    self.last_tok = Token::ILLEGAL;
                    continue;
                }
                Item::Ident(x) => {
                    data = x.name.clone();
                    is_lit = false;
                    implied_semi = true;
                    self.last_tok = Token::IDENT;
                }
                Item::Lit(x) => {
                    data = x.value.clone();
                    is_lit = true;
                    implied_semi = true;
                    self.last_tok = x.kind.unwrap_or(Token::ILLEGAL);
                }
                Item::Tok(x) => {
                    let s = x.as_str().to_string();
                    if may_combine(self.last_tok, s.as_bytes()[0]) {
                        if !self.wsbuf.is_empty() {
                            self.internal_error("whitespace buffer not empty");
                        }
                        self.wsbuf.clear();
                        self.wsbuf.push(BLANK);
                    }
                    data = s;
                    implied_semi = matches!(
                        x,
                        Token::BREAK
                            | Token::CONTINUE
                            | Token::FALLTHROUGH
                            | Token::RETURN
                            | Token::INC
                            | Token::DEC
                            | Token::RPAREN
                            | Token::RBRACK
                            | Token::RBRACE
                    );
                    self.last_tok = x;
                    is_lit = false;
                }
                Item::Str(x) => {
                    data = x.to_string();
                    is_lit = true;
                    implied_semi = true;
                    self.last_tok = Token::STRING;
                }
            }

            let next = self.pos.clone();
            let (wrote_newline, dropped_ff) = self.flush(next.clone(), self.last_tok);
            if !self.implied_semi {
                let mut n = nlimit(next.line - self.pos.line);
                if wrote_newline && n == MAX_NEWLINES {
                    n = MAX_NEWLINES - 1;
                }
                if n > 0 {
                    let mut ch = b'\n';
                    if dropped_ff {
                        ch = b'\x0c';
                    }
                    self.write_byte(ch, n as i32);
                    implied_semi = false;
                }
            }
            if let Some(ptr) = self.line_ptr.take() {
                unsafe {
                    *ptr = self.out.line;
                }
            }
            self.write_string(next, &data, is_lit);
            self.implied_semi = implied_semi;
        }
    }

    pub(crate) fn flush(&mut self, next: Position, tok: Token) -> (bool, bool) {
        if self.comment_before(&next) {
            self.intersperse_comments(next, tok)
        } else {
            let n = self.wsbuf.len();
            self.write_whitespace(n);
            (false, false)
        }
    }

    pub(crate) fn set_comment(&mut self, g: Option<&CommentGroup>) {
        let Some(g) = g else { return };
        if !self.use_node_comments {
            return;
        }
        if self.comments.is_empty() {
            self.comments.push(g.clone());
        } else if self.comment.cindex < self.comments.len() {
            self.flush(self.pos_for(g.list[0].pos()), Token::ILLEGAL);
            self.comments.clear();
            self.comments.push(g.clone());
            self.internal_error("setComment found pending comments");
        } else {
            self.comments.clear();
            self.comments.push(g.clone());
        }
        self.comment.cindex = 0;
        if self.comment.comment_offset == INFINITY {
            self.next_comment();
        }
    }
}

pub(crate) fn nlimit(n: i64) -> i64 {
    n.min(MAX_NEWLINES)
}

fn may_combine(prev: Token, next: u8) -> bool {
    match prev {
        Token::INT => next == b'.',
        Token::ADD => next == b'+',
        Token::SUB => next == b'-',
        Token::QUO => next == b'*',
        Token::LSS => next == b'-' || next == b'<',
        Token::AND => next == b'&' || next == b'^',
        _ => false,
    }
}

fn is_blank(s: &str) -> bool {
    s.bytes().all(|b| b <= b' ')
}

fn common_prefix(a: &str, b: &str) -> String {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    let mut i = 0;
    while i < ab.len() && i < bb.len() && ab[i] == bb[i] && (ab[i] <= b' ' || ab[i] == b'*') {
        i += 1;
    }
    a[..i].to_string()
}

fn trim_right(s: &str) -> &str {
    s.trim_end_matches(|c: char| c.is_whitespace())
}

fn strip_common_prefix(lines: &mut [String]) {
    if lines.len() <= 1 {
        return;
    }
    let mut prefix = String::new();
    let mut prefix_set = false;
    if lines.len() > 2 {
        // Match Go: on every non-blank inner line, set prefix then ALWAYS
        // run commonPrefix (which keeps only leading whitespace/'*'). Skipping
        // the first commonPrefix left the full line as prefix and emptied
        // comment bodies (math/j0.go, cgo preambles, …).
        for i in 1..lines.len() - 1 {
            if is_blank(&lines[i]) {
                lines[i].clear();
            } else {
                if !prefix_set {
                    prefix = lines[i].clone();
                    prefix_set = true;
                }
                prefix = common_prefix(&prefix, &lines[i]);
            }
        }
    }
    if !prefix_set {
        let line = lines[lines.len() - 1].clone();
        prefix = common_prefix(&line, &line);
    }

    let mut line_of_stars = false;
    if let Some((p, _)) = prefix.split_once('*') {
        prefix = p.trim_end_matches(' ').to_string();
        line_of_stars = true;
    } else {
        let first = lines[0].clone();
        if first.len() >= 2 && is_blank(&first[2..]) {
            let mut i = prefix.len();
            let mut n = 0;
            while n < 3 && i > 0 && prefix.as_bytes()[i - 1] == b' ' {
                i -= 1;
                n += 1;
            }
            if i == prefix.len() && i > 0 && prefix.as_bytes()[i - 1] == b'\t' {
                i -= 1;
            }
            prefix = prefix[..i].to_string();
        } else {
            let fb = first.as_bytes();
            let mut suffix = vec![0u8; first.len()];
            let mut n = 2usize;
            while n < fb.len() && fb[n] <= b' ' {
                suffix[n] = fb[n];
                n += 1;
            }
            let suffix_s = if n > 2 && suffix[2] == b'\t' {
                String::from_utf8_lossy(&suffix[2..n]).into_owned()
            } else {
                suffix[0] = b' ';
                suffix[1] = b' ';
                String::from_utf8_lossy(&suffix[0..n]).into_owned()
            };
            if let Some(stripped) = prefix.strip_suffix(&suffix_s) {
                prefix = stripped.to_string();
            }
        }
    }

    let last = lines[lines.len() - 1].clone();
    let closing = "*/";
    let before = last.split(closing).next().unwrap_or("");
    if is_blank(before) {
        let closing = if line_of_stars { " */" } else { "*/" };
        lines[lines.len() - 1] = format!("{prefix}{closing}");
    } else {
        prefix = common_prefix(&prefix, &last);
    }

    for i in 0..lines.len() {
        if i > 0 && !lines[i].is_empty() {
            if lines[i].starts_with(&prefix) {
                lines[i] = lines[i][prefix.len()..].to_string();
            }
        }
    }
}

fn get_doc<'b>(n: &PrintNode<'b>) -> Option<&'b CommentGroup> {
    match n {
        PrintNode::File(f) => f.doc.as_ref(),
        PrintNode::Decl(Decl::GenDecl(d)) => d.doc.as_ref(),
        PrintNode::Decl(Decl::FuncDecl(d)) => d.doc.as_ref(),
        PrintNode::Spec(Spec::ImportSpec(s)) => s.doc.as_ref(),
        PrintNode::Spec(Spec::ValueSpec(s)) => s.doc.as_ref(),
        PrintNode::Spec(Spec::TypeSpec(s)) => s.doc.as_ref(),
        _ => None,
    }
}

fn get_last_comment<'b>(n: &PrintNode<'b>) -> Option<&'b CommentGroup> {
    match n {
        PrintNode::Spec(Spec::ImportSpec(s)) => s.comment.as_ref(),
        PrintNode::Spec(Spec::ValueSpec(s)) => s.comment.as_ref(),
        PrintNode::Spec(Spec::TypeSpec(s)) => s.comment.as_ref(),
        PrintNode::Decl(Decl::GenDecl(d)) => {
            d.specs.last().and_then(|s| match s {
                Spec::ImportSpec(x) => x.comment.as_ref(),
                Spec::ValueSpec(x) => x.comment.as_ref(),
                Spec::TypeSpec(x) => x.comment.as_ref(),
            })
        }
        PrintNode::File(f) => f.comments.last(),
        _ => None,
    }
}

impl Printer<'_> {
    pub(crate) fn print_node(&mut self, node: PrintNode<'_>) -> Result<(), String> {
        // Lifetime trick: we re-bind comments from the node. Because Printer's
        // lifetime is tied to fset, and comments live as long as the AST which
        // must outlive the print call, we use a scoped approach with raw
        // indices into File.comments when printing a File.

        // For File we set comments from file.comments via unsafe transmute of
        // lifetimes — NOT used. Instead, print_file_node takes &File and sets
        // comments properly in fprint.
        match node {
            PrintNode::File(f) => {
                self.comments = f.comments.clone();
                self.use_node_comments = self.comments.is_empty();
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                self.file(f);
            }
            PrintNode::Expr(e) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                self.expr(e);
            }
            PrintNode::Stmt(s) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                if matches!(s, Stmt::LabeledStmt(_)) {
                    self.indent = 1;
                }
                self.stmt(s, false);
            }
            PrintNode::Decl(d) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                self.decl(d);
            }
            PrintNode::Spec(s) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                self.spec(s, 1, false);
            }
            PrintNode::Stmts(list) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                for s in list {
                    if matches!(s, Stmt::LabeledStmt(_)) {
                        self.indent = 1;
                    }
                }
                self.stmt_list(list, 0, false);
            }
            PrintNode::Decls(list) => {
                self.use_node_comments = true;
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                self.decl_list(list);
            }
            PrintNode::Commented(c) => {
                self.comments = c.comments.to_vec();
                self.use_node_comments = self.comments.is_empty();
                self.next_comment();
                self.print(&[Item::Mode(PMode(0))]);
                return self.print_node(*c.node);
            }
        }
        if let Some(e) = self.source_pos_err.clone() {
            return Err(e);
        }
        Ok(())
    }
}

/// Trimmer: strip Escape, trailing blanks/tabs; convert formfeed/vtab.
struct Trimmer<W: Write> {
    output: W,
    state: i32,
    space: Vec<u8>,
}

const IN_SPACE: i32 = 0;
const IN_ESCAPE: i32 = 1;
const IN_TEXT: i32 = 2;

impl<W: Write> Trimmer<W> {
    fn new(output: W) -> Self {
        Self {
            output,
            state: IN_SPACE,
            space: Vec::new(),
        }
    }
    fn reset_space(&mut self) {
        self.state = IN_SPACE;
        self.space.clear();
    }
}

impl<W: Write> Write for Trimmer<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut m = 0usize;
        let mut n = 0usize;
        while n < data.len() {
            let mut b = data[n];
            if b == b'\x0b' {
                b = b'\t';
            }
            match self.state {
                IN_SPACE => match b {
                    b'\t' | b' ' => self.space.push(b),
                    b'\n' | b'\x0c' => {
                        self.reset_space();
                        self.output.write_all(b"\n")?;
                    }
                    tabwriter::ESCAPE => {
                        self.output.write_all(&self.space)?;
                        self.state = IN_ESCAPE;
                        m = n + 1;
                    }
                    _ => {
                        self.output.write_all(&self.space)?;
                        self.state = IN_TEXT;
                        m = n;
                    }
                },
                IN_ESCAPE => {
                    if b == tabwriter::ESCAPE {
                        self.output.write_all(&data[m..n])?;
                        self.reset_space();
                    }
                }
                IN_TEXT => match b {
                    b'\t' | b' ' => {
                        self.output.write_all(&data[m..n])?;
                        self.reset_space();
                        self.space.push(b);
                    }
                    b'\n' | b'\x0c' => {
                        self.output.write_all(&data[m..n])?;
                        self.reset_space();
                        self.output.write_all(b"\n")?;
                    }
                    tabwriter::ESCAPE => {
                        self.output.write_all(&data[m..n])?;
                        self.state = IN_ESCAPE;
                        m = n + 1;
                    }
                    _ => {}
                },
                _ => unreachable!(),
            }
            n += 1;
        }
        match self.state {
            IN_ESCAPE | IN_TEXT => {
                self.output.write_all(&data[m..n])?;
                self.reset_space();
            }
            _ => {}
        }
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.output.flush()
    }
}

struct SizeCounter {
    size: i32,
    has_newline: bool,
}

impl Write for SizeCounter {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        for &b in p {
            if b == b'\n' || b == b'\x0c' {
                self.has_newline = true;
            }
        }
        self.size += p.len() as i32;
        Ok(p.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Config {
    /// Pretty-print `node` to `output`.
    pub fn fprint<W: Write>(
        &self,
        output: &mut W,
        fset: &Arc<FileSet>,
        node: PrintNode<'_>,
    ) -> io::Result<()> {
        self.fprint_with_sizes(output, fset, node, HashMap::new())
            .map(|_| ())
    }

    /// Like [`fprint`](Self::fprint) but threads the `node_sizes` memo map in
    /// and out. Go's `printer.nodeSize` passes `p.nodeSizes` *by reference* to
    /// the recursive `cfg.fprint`, so sizes computed while measuring a node are
    /// shared back into the caller's map (this is what keeps `nodeSize` linear
    /// on deeply nested literals — issue 1628). Rust can't alias `self`'s map
    /// through a sub-printer, so we move it in and return the enriched map to be
    /// moved back — same effect, no clone.
    pub(crate) fn fprint_with_sizes<W: Write>(
        &self,
        output: &mut W,
        fset: &Arc<FileSet>,
        node: PrintNode<'_>,
        node_sizes: HashMap<SizeKey, i32>,
    ) -> io::Result<HashMap<SizeKey, i32>> {
        let mut p = Printer::new(self.clone(), fset, node_sizes);
        if let Err(e) = p.print_node(node) {
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }
        p.implied_semi = false;
        p.flush(
            Position {
                offset: INFINITY as i64,
                line: INFINITY as i64,
                ..Default::default()
            },
            Token::EOF,
        );
        p.fix_go_build_lines();

        let mut buf = Vec::new();
        {
            let mut trimmer = Trimmer::new(&mut buf);
            if self.mode & RAW_FORMAT == 0 {
                let mut minwidth = self.tabwidth;
                let mut padchar = b'\t';
                if self.mode & USE_SPACES != 0 {
                    padchar = b' ';
                }
                let mut twmode = tabwriter::DISCARD_EMPTY_COLUMNS;
                if self.mode & TAB_INDENT != 0 {
                    minwidth = 0;
                    twmode |= tabwriter::TAB_INDENT;
                }
                let mut tw = TabWriter::new(
                    &mut trimmer,
                    minwidth as usize,
                    self.tabwidth as usize,
                    1,
                    padchar,
                    twmode,
                );
                tw.write(&p.output)?;
                tw.flush()?;
            } else {
                trimmer.write_all(&p.output)?;
            }
        }
        output.write_all(&buf)?;
        Ok(std::mem::take(&mut p.node_sizes))
    }
}

/// Pretty-print with default config (tabwidth 8). Prefer [`format`](crate::format)
/// for gofmt-identical output.
pub fn fprint<W: Write>(
    output: &mut W,
    fset: &Arc<FileSet>,
    node: PrintNode<'_>,
) -> io::Result<()> {
    Config {
        tabwidth: 8,
        ..Default::default()
    }
    .fprint(output, fset, node)
}

// Silence unused imports that nodes.rs will use via the same module.
#[allow(dead_code)]
fn _keep_imports(
    _: Field,
    _: FuncDecl,
    _: GenDecl,
    _: ImportSpec,
    _: TypeSpec,
    _: ValueSpec,
    _: Stmt,
    _: Expr,
    _: Spec,
    _: Decl,
) {
}
