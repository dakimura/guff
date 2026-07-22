// Port of Go's go/parser/parser.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Recursive-descent parser for Go source files. Public entry points
// are [`parse_file`] and [`parse_expr_from`].
//
// Translation notes:
//
// * `Trace` mode and the `trace()`/`un()`/`printTrace()` helpers are
//   omitted — they'd account for ~5% of the file and don't affect
//   behavior. `Mode::TRACE` is accepted by the API but has no effect.
// * `panic(bailout{})` is mapped to `panic_any(Bailout{ … })`,
//   caught at the top of [`parse_file`] via `catch_unwind`.
// * `defer decNestLev(incNestLev(p))` is expressed by wrapping the
//   body in [`Parser::with_nest`] so the decrement happens on every
//   normal return path.
// * `p.error` collects into a shared `Rc<RefCell<ErrorList>>` because
//   the scanner's `ErrorHandler` and the parser both write to it.

use std::cell::RefCell;
use std::panic::{catch_unwind, panic_any, AssertUnwindSafe};
use std::rc::Rc;
use std::sync::Arc;

use crate::ast::*;
use crate::constraint;
use crate::errors::ErrorList;
use crate::parser_resolver::resolve_file;
use crate::position::{File as PosFile, FileSet, Pos, Position, NO_POS};
use crate::scanner::{self, Scanner, SCAN_COMMENTS};
use crate::token::{self, Token, LOWEST_PREC};

// ====================================================================
// Mode flags
// ====================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode(pub u32);

impl Mode {
    pub const NONE: Mode = Mode(0);
    pub fn contains(self, m: Mode) -> bool {
        self.0 & m.0 != 0
    }
}

impl std::ops::BitOr for Mode {
    type Output = Mode;
    fn bitor(self, rhs: Mode) -> Mode {
        Mode(self.0 | rhs.0)
    }
}

pub const PACKAGE_CLAUSE_ONLY: Mode = Mode(1 << 0);
pub const IMPORTS_ONLY: Mode = Mode(1 << 1);
pub const PARSE_COMMENTS: Mode = Mode(1 << 2);
pub const TRACE: Mode = Mode(1 << 3);
pub const DECLARATION_ERRORS: Mode = Mode(1 << 4);
pub const SPURIOUS_ERRORS: Mode = Mode(1 << 5);
pub const SKIP_OBJECT_RESOLUTION: Mode = Mode(1 << 6);
pub const ALL_ERRORS: Mode = SPURIOUS_ERRORS;

// ====================================================================
// Bailout
// ====================================================================

#[derive(Debug, Clone)]
struct Bailout {
    _pos: Pos,
    _msg: String,
}

const MAX_NEST_LEV: usize = 100_000;

// ====================================================================
// Token-set predicates (replace Go's map[Token]bool)
// ====================================================================

fn is_stmt_start(t: Token) -> bool {
    matches!(
        t,
        Token::BREAK
            | Token::CONST
            | Token::CONTINUE
            | Token::DEFER
            | Token::FALLTHROUGH
            | Token::FOR
            | Token::GO
            | Token::GOTO
            | Token::IF
            | Token::RETURN
            | Token::SELECT
            | Token::SWITCH
            | Token::TYPE
            | Token::VAR
    )
}

fn is_decl_start(t: Token) -> bool {
    matches!(t, Token::IMPORT | Token::CONST | Token::TYPE | Token::VAR)
}

fn is_expr_end(t: Token) -> bool {
    matches!(
        t,
        Token::COMMA
            | Token::COLON
            | Token::SEMICOLON
            | Token::RPAREN
            | Token::RBRACK
            | Token::RBRACE
    )
}

// ====================================================================
// Parser
// ====================================================================

struct Parser {
    file: Arc<PosFile>,
    errors: Rc<RefCell<ErrorList>>,
    scanner: Scanner<'static>,

    mode: Mode,

    // Comments
    comments: Vec<CommentGroup>,
    lead_comment: Option<CommentGroup>,
    line_comment: Option<CommentGroup>,
    top: bool,
    go_version: String,

    // Next token
    pos: Pos,
    tok: Token,
    lit: String,
    string_end: Pos,

    // Error recovery
    sync_pos: Pos,
    sync_cnt: usize,

    // Non-syntactic control
    expr_lev: i32,
    in_rhs: bool,

    imports: Vec<ImportSpec>,

    nest_lev: usize,
}

// SimpleStmt parsing modes
const SS_BASIC: u32 = 0;
const SS_LABEL_OK: u32 = 1;
const SS_RANGE_OK: u32 = 2;

impl Parser {
    fn new(file: Arc<PosFile>, src: &[u8], mode: Mode) -> Self {
        let errors: Rc<RefCell<ErrorList>> = Rc::new(RefCell::new(ErrorList::new()));
        let err_clone = Rc::clone(&errors);
        let eh: scanner::ErrorHandler<'static> = Box::new(move |pos: Position, msg: &str| {
            err_clone.borrow_mut().add(pos, msg);
        });
        let mut scanner: Scanner<'static> = Scanner::new();
        scanner.init(Arc::clone(&file), src, Some(eh), SCAN_COMMENTS);

        let mut p = Parser {
            file,
            errors,
            scanner,
            mode,
            comments: Vec::new(),
            lead_comment: None,
            line_comment: None,
            top: true,
            go_version: String::new(),
            pos: NO_POS,
            tok: Token::ILLEGAL,
            lit: String::new(),
            string_end: NO_POS,
            sync_pos: NO_POS,
            sync_cnt: 0,
            expr_lev: 0,
            in_rhs: false,
            imports: Vec::new(),
            nest_lev: 0,
        };
        p.next();
        p
    }

    fn line_for(&self, pos: Pos) -> i64 {
        self.file.position_for(pos, false).line
    }

    // ---- comment/scanner plumbing ---------------------------------

    fn consume_comment(&mut self) -> (Comment, i64) {
        let mut endline = self.line_for(self.pos);
        if self.lit.as_bytes().get(1) == Some(&b'*') {
            for b in self.lit.as_bytes() {
                if *b == b'\n' {
                    endline += 1;
                }
            }
        }
        let comment = Comment {
            slash: self.pos,
            text: self.lit.clone(),
        };
        self.next0();
        (comment, endline)
    }

    fn consume_comment_group(&mut self, n: i64) -> (CommentGroup, i64) {
        let mut list: Vec<Comment> = Vec::new();
        let mut endline = self.line_for(self.pos);
        while self.tok == Token::COMMENT && self.line_for(self.pos) <= endline + n {
            let (c, el) = self.consume_comment();
            endline = el;
            list.push(c);
        }
        let group = CommentGroup { list };
        self.comments.push(group.clone());
        (group, endline)
    }

    fn next0(&mut self) {
        loop {
            let (pos, tok, lit) = self.scanner.scan();
            self.pos = pos;
            self.tok = tok;
            self.lit = lit;
            if tok == Token::COMMENT {
                if self.top && self.lit.starts_with("//go:build") {
                    if let Ok(x) = constraint::parse(&self.lit) {
                        if let Some(v) = constraint::go_version(&x) {
                            self.go_version = v;
                        }
                    }
                }
                // Always retain comments before the first declaration so package
                // and leading declaration docs are available without ParseComments.
                if !self.mode.contains(PARSE_COMMENTS) && !self.top {
                    continue;
                }
            } else {
                if tok == Token::STRING {
                    self.string_end = self.scanner.string_end();
                }
                self.top = false;
            }
            break;
        }
    }

    fn next(&mut self) {
        self.lead_comment = None;
        self.line_comment = None;
        let prev = self.pos;
        self.next0();

        if self.tok == Token::COMMENT {
            let mut comment: Option<CommentGroup> = None;
            let mut endline: i64;

            if self.line_for(self.pos) == self.line_for(prev) {
                let (cg, el) = self.consume_comment_group(0);
                comment = Some(cg);
                endline = el;
                if self.line_for(self.pos) != endline
                    || self.tok == Token::SEMICOLON
                    || self.tok == Token::EOF
                {
                    self.line_comment = comment.clone();
                }
            }
            endline = -1;
            while self.tok == Token::COMMENT {
                let (cg, el) = self.consume_comment_group(1);
                comment = Some(cg);
                endline = el;
            }
            if endline + 1 == self.line_for(self.pos) {
                self.lead_comment = comment;
            }
        }
    }

    // ---- error reporting ------------------------------------------

    fn error(&mut self, pos: Pos, msg: impl Into<String>) {
        let msg = msg.into();
        let epos = self.file.position(pos);

        if !self.mode.contains(ALL_ERRORS) {
            let errs = self.errors.borrow();
            let n = errs.len();
            if n > 0 && errs.iter().last().map(|e| e.pos.line) == Some(epos.line) {
                return;
            }
            if n > 10 {
                drop(errs);
                panic_any(Bailout {
                    _pos: pos,
                    _msg: String::new(),
                });
            }
        }
        self.errors.borrow_mut().add(epos, msg);
    }

    fn error_expected(&mut self, pos: Pos, msg: &str) {
        let mut msg = format!("expected {}", msg);
        if pos == self.pos {
            if self.tok == Token::SEMICOLON && self.lit == "\n" {
                msg.push_str(", found newline");
            } else if self.tok.is_literal() {
                msg.push_str(&format!(", found {}", self.lit));
            } else {
                msg.push_str(&format!(", found '{}'", self.tok));
            }
        }
        self.error(pos, msg);
    }

    fn expect(&mut self, tok: Token) -> Pos {
        let pos = self.pos;
        if self.tok != tok {
            self.error_expected(pos, &format!("'{}'", tok));
        }
        self.next();
        pos
    }

    fn expect2(&mut self, tok: Token) -> Pos {
        let pos = if self.tok == tok {
            self.pos
        } else {
            self.error_expected(self.pos, &format!("'{}'", tok));
            NO_POS
        };
        self.next();
        pos
    }

    fn expect_closing(&mut self, tok: Token, context: &str) -> Pos {
        if self.tok != tok && self.tok == Token::SEMICOLON && self.lit == "\n" {
            let pos = self.pos;
            self.error(pos, format!("missing ',' before newline in {}", context));
            self.next();
        }
        self.expect(tok)
    }

    fn expect_semi(&mut self) -> Option<CommentGroup> {
        match self.tok {
            Token::RPAREN | Token::RBRACE => None,
            Token::COMMA => {
                self.error_expected(self.pos, "';'");
                // fallthrough to SEMICOLON
                self.expect_semi_inner()
            }
            Token::SEMICOLON => self.expect_semi_inner(),
            _ => {
                self.error_expected(self.pos, "';'");
                self.advance_to(is_stmt_start);
                None
            }
        }
    }

    fn expect_semi_inner(&mut self) -> Option<CommentGroup> {
        let comment;
        if self.lit == ";" {
            self.next();
            comment = self.line_comment.clone();
        } else {
            comment = self.line_comment.clone();
            self.next();
        }
        comment
    }

    fn at_comma(&mut self, context: &str, follow: Token) -> bool {
        if self.tok == Token::COMMA {
            return true;
        }
        if self.tok != follow {
            let mut msg = String::from("missing ','");
            if self.tok == Token::SEMICOLON && self.lit == "\n" {
                msg.push_str(" before newline");
            }
            let pos = self.pos;
            self.error(pos, format!("{} in {}", msg, context));
            return true;
        }
        false
    }

    fn advance_to(&mut self, predicate: fn(Token) -> bool) {
        while self.tok != Token::EOF {
            if predicate(self.tok) {
                if self.pos == self.sync_pos && self.sync_cnt < 10 {
                    self.sync_cnt += 1;
                    return;
                }
                if self.pos.0 > self.sync_pos.0 {
                    self.sync_pos = self.pos;
                    self.sync_cnt = 0;
                    return;
                }
            }
            self.next();
        }
    }

    // ---- nesting guard --------------------------------------------

    fn inc_nest_lev(&mut self) {
        self.nest_lev += 1;
        if self.nest_lev > MAX_NEST_LEV {
            let pos = self.pos;
            self.error(pos, "exceeded max nesting depth");
            panic_any(Bailout {
                _pos: pos,
                _msg: String::new(),
            });
        }
    }

    fn dec_nest_lev(&mut self) {
        if self.nest_lev > 0 {
            self.nest_lev -= 1;
        }
    }

    fn with_nest<R>(&mut self, body: impl FnOnce(&mut Self) -> R) -> R {
        self.inc_nest_lev();
        let r = body(self);
        self.dec_nest_lev();
        r
    }

    // ---- identifiers ----------------------------------------------

    fn parse_ident(&mut self) -> Ident {
        let pos = self.pos;
        let name = if self.tok == Token::IDENT {
            let n = self.lit.clone();
            self.next();
            n
        } else {
            self.expect(Token::IDENT);
            "_".to_string()
        };
        Ident {
            name_pos: pos,
            name,
            obj: std::sync::Mutex::new(None),
            id: crate::ast::next_node_id(),
        }
    }

    fn parse_ident_list(&mut self) -> Vec<Ident> {
        let mut list = vec![self.parse_ident()];
        while self.tok == Token::COMMA {
            self.next();
            list.push(self.parse_ident());
        }
        list
    }

    // ---- common productions ---------------------------------------

    fn parse_expr_list(&mut self) -> Vec<Expr> {
        let mut list = vec![self.parse_expr()];
        while self.tok == Token::COMMA {
            self.next();
            list.push(self.parse_expr());
        }
        list
    }

    fn parse_list(&mut self, in_rhs: bool) -> Vec<Expr> {
        let old = self.in_rhs;
        self.in_rhs = in_rhs;
        let list = self.parse_expr_list();
        self.in_rhs = old;
        list
    }

    // ---- types ----------------------------------------------------

    fn parse_type(&mut self) -> Expr {
        let typ = self.try_ident_or_type();
        match typ {
            Some(t) => t,
            None => {
                let pos = self.pos;
                self.error_expected(pos, "type");
                self.advance_to(is_expr_end);
                Expr::BadExpr(BadExpr {
                    id: 0,
                    from: pos,
                    to: self.pos,
                })
            }
        }
    }

    fn parse_qualified_ident(&mut self, ident: Option<Ident>) -> Expr {
        let typ = self.parse_type_name(ident);
        if self.tok == Token::LBRACK {
            self.parse_type_instance(typ)
        } else {
            typ
        }
    }

    fn parse_type_name(&mut self, ident: Option<Ident>) -> Expr {
        let ident = ident.unwrap_or_else(|| self.parse_ident());
        if self.tok == Token::PERIOD {
            self.next();
            let sel = self.parse_ident();
            return Expr::SelectorExpr(SelectorExpr {
                id: 0,
                x: Box::new(Expr::Ident(ident)),
                sel,
            });
        }
        Expr::Ident(ident)
    }

    fn parse_array_type(&mut self, lbrack: Pos, len: Option<Expr>) -> ArrayType {
        let len = if let Some(l) = len {
            Some(Box::new(l))
        } else {
            self.expr_lev += 1;
            let l = if self.tok == Token::ELLIPSIS {
                let p = self.pos;
                self.next();
                Some(Box::new(Expr::Ellipsis(Ellipsis {
                    id: 0,
                    ellipsis: p,
                    elt: None,
                })))
            } else if self.tok != Token::RBRACK {
                Some(Box::new(self.parse_rhs()))
            } else {
                None
            };
            self.expr_lev -= 1;
            l
        };
        if self.tok == Token::COMMA {
            let pos = self.pos;
            self.error(pos, "unexpected comma; expecting ]");
            self.next();
        }
        self.expect(Token::RBRACK);
        let elt = self.parse_type();
        ArrayType {
            id: 0,
            lbrack,
            len,
            elt: Box::new(elt),
        }
    }

    fn parse_array_field_or_type_instance(&mut self, x: Ident) -> (Option<Ident>, Expr) {
        let lbrack = self.expect(Token::LBRACK);
        let mut trailing_comma = NO_POS;
        let mut args: Vec<Expr> = Vec::new();
        if self.tok != Token::RBRACK {
            self.expr_lev += 1;
            args.push(self.parse_rhs());
            while self.tok == Token::COMMA {
                let comma = self.pos;
                self.next();
                if self.tok == Token::RBRACK {
                    trailing_comma = comma;
                    break;
                }
                args.push(self.parse_rhs());
            }
            self.expr_lev -= 1;
        }
        let rbrack = self.expect(Token::RBRACK);

        if args.is_empty() {
            let elt = self.parse_type();
            return (
                Some(x),
                Expr::ArrayType(ArrayType {
                    id: 0,
                    lbrack,
                    len: None,
                    elt: Box::new(elt),
                }),
            );
        }

        if args.len() == 1 {
            if let Some(elt) = self.try_ident_or_type() {
                if trailing_comma.is_valid() {
                    self.error(trailing_comma, "unexpected comma; expecting ]");
                }
                return (
                    Some(x),
                    Expr::ArrayType(ArrayType {
                        id: 0,
                        lbrack,
                        len: Some(Box::new(args.into_iter().next().unwrap())),
                        elt: Box::new(elt),
                    }),
                );
            }
        }

        let packed = pack_index_expr(Expr::Ident(x), lbrack, args, rbrack);
        (None, packed)
    }

    fn parse_field_decl(&mut self) -> Field {
        let doc = self.lead_comment.clone();
        let mut names: Vec<Ident> = Vec::new();
        let mut typ: Option<Expr> = None;

        match self.tok {
            Token::IDENT => {
                let name = self.parse_ident();
                if matches!(
                    self.tok,
                    Token::PERIOD | Token::STRING | Token::SEMICOLON | Token::RBRACE
                ) {
                    typ = Some(if self.tok == Token::PERIOD {
                        self.parse_qualified_ident(Some(name))
                    } else {
                        Expr::Ident(name)
                    });
                } else {
                    names.push(name);
                    while self.tok == Token::COMMA {
                        self.next();
                        names.push(self.parse_ident());
                    }
                    if names.len() == 1 && self.tok == Token::LBRACK {
                        let first = names.remove(0);
                        let (n, t) = self.parse_array_field_or_type_instance(first);
                        match n {
                            Some(nm) => names.push(nm),
                            None => names.clear(),
                        }
                        typ = Some(t);
                    } else {
                        typ = Some(self.parse_type());
                    }
                }
            }
            Token::MUL => {
                let star = self.pos;
                self.next();
                let inner = if self.tok == Token::LPAREN {
                    let pos = self.pos;
                    self.error(pos, "cannot parenthesize embedded type");
                    self.next();
                    let inner = self.parse_qualified_ident(None);
                    if self.tok == Token::RPAREN {
                        self.next();
                    }
                    inner
                } else {
                    self.parse_qualified_ident(None)
                };
                typ = Some(Expr::StarExpr(StarExpr {
                    id: 0,
                    star,
                    x: Box::new(inner),
                }));
            }
            Token::LPAREN => {
                let pos = self.pos;
                self.error(pos, "cannot parenthesize embedded type");
                self.next();
                let inner = if self.tok == Token::MUL {
                    let star = self.pos;
                    self.next();
                    Expr::StarExpr(StarExpr {
                        id: 0,
                        star,
                        x: Box::new(self.parse_qualified_ident(None)),
                    })
                } else {
                    self.parse_qualified_ident(None)
                };
                if self.tok == Token::RPAREN {
                    self.next();
                }
                typ = Some(inner);
            }
            _ => {
                let pos = self.pos;
                self.error_expected(pos, "field name or embedded type");
                self.advance_to(is_expr_end);
                typ = Some(Expr::BadExpr(BadExpr {
                    id: 0,
                    from: pos,
                    to: self.pos,
                }));
            }
        }

        let tag = if self.tok == Token::STRING {
            let bl = BasicLit {
                id: 0,
                value_pos: self.pos,
                value_end: self.string_end,
                kind: Some(self.tok),
                value: self.lit.clone(),
            };
            self.next();
            Some(bl)
        } else {
            None
        };

        let comment = self.expect_semi();
        Field {
            doc,
            names,
            ty: typ,
            tag,
            comment,
            id: 0,
        }
    }

    fn parse_struct_type(&mut self) -> StructType {
        let pos = self.expect(Token::STRUCT);
        let lbrace = self.expect(Token::LBRACE);
        let mut list: Vec<Field> = Vec::new();
        while matches!(self.tok, Token::IDENT | Token::MUL | Token::LPAREN) {
            list.push(self.parse_field_decl());
        }
        let rbrace = self.expect(Token::RBRACE);
        StructType {
            id: 0,
            struct_: pos,
            fields: FieldList {
                opening: lbrace,
                list,
                closing: rbrace,
            },
            incomplete: false,
        }
    }

    fn parse_pointer_type(&mut self) -> StarExpr {
        let star = self.expect(Token::MUL);
        let base = self.parse_type();
        StarExpr {
            id: 0,
            star,
            x: Box::new(base),
        }
    }

    fn parse_dots_type(&mut self) -> Ellipsis {
        let pos = self.expect(Token::ELLIPSIS);
        let elt = self.parse_type();
        Ellipsis {
            id: 0,
            ellipsis: pos,
            elt: Some(Box::new(elt)),
        }
    }

    fn parse_param_decl(&mut self, name: Option<Ident>, type_sets_ok: bool) -> ParamField {
        let ptok = self.tok;
        if name.is_some() {
            self.tok = Token::IDENT;
        } else if type_sets_ok && self.tok == Token::TILDE {
            return ParamField {
                name: None,
                ty: Some(self.embedded_elem(None)),
            };
        }

        let mut f = ParamField {
            name: None,
            ty: None,
        };
        match self.tok {
            Token::IDENT => {
                f.name = if let Some(n) = name {
                    self.tok = ptok;
                    Some(n)
                } else {
                    Some(self.parse_ident())
                };
                match self.tok {
                    Token::IDENT
                    | Token::MUL
                    | Token::ARROW
                    | Token::FUNC
                    | Token::CHAN
                    | Token::MAP
                    | Token::STRUCT
                    | Token::INTERFACE
                    | Token::LPAREN => {
                        f.ty = Some(self.parse_type());
                    }
                    Token::LBRACK => {
                        let nm = f.name.take().unwrap();
                        let (n, t) = self.parse_array_field_or_type_instance(nm);
                        f.name = n;
                        f.ty = Some(t);
                    }
                    Token::ELLIPSIS => {
                        f.ty = Some(Expr::Ellipsis(self.parse_dots_type()));
                        return f;
                    }
                    Token::PERIOD => {
                        let nm = f.name.take().unwrap();
                        f.ty = Some(self.parse_qualified_ident(Some(nm)));
                    }
                    Token::TILDE => {
                        if type_sets_ok {
                            f.ty = Some(self.embedded_elem(None));
                            return f;
                        }
                    }
                    Token::OR => {
                        if type_sets_ok {
                            let nm = f.name.take().unwrap();
                            f.ty = Some(self.embedded_elem(Some(Expr::Ident(nm))));
                            return f;
                        }
                    }
                    _ => {}
                }
            }
            Token::MUL
            | Token::ARROW
            | Token::FUNC
            | Token::LBRACK
            | Token::CHAN
            | Token::MAP
            | Token::STRUCT
            | Token::INTERFACE
            | Token::LPAREN => {
                f.ty = Some(self.parse_type());
            }
            Token::ELLIPSIS => {
                f.ty = Some(Expr::Ellipsis(self.parse_dots_type()));
                return f;
            }
            _ => {
                let pos = self.pos;
                self.error_expected(pos, "')'");
                self.advance_to(is_expr_end);
            }
        }
        if type_sets_ok && self.tok == Token::OR && f.ty.is_some() {
            f.ty = Some(self.embedded_elem(f.ty.take()));
        }
        f
    }

    fn parse_parameter_list(
        &mut self,
        name0: Option<Ident>,
        typ0: Option<Expr>,
        closing: Token,
        dddok: bool,
    ) -> Vec<Field> {
        let tparams = closing == Token::RBRACK;
        let mut list: Vec<ParamField> = Vec::new();
        let mut named = 0usize;
        let mut typed = 0usize;
        let mut name0_opt = name0;
        let mut typ0_opt = typ0;

        while name0_opt.is_some() || (self.tok != closing && self.tok != Token::EOF) {
            let par = if let Some(t0) = typ0_opt.take() {
                let mut ty = t0;
                if tparams {
                    ty = self.embedded_elem(Some(ty));
                }
                ParamField {
                    name: name0_opt.take(),
                    ty: Some(ty),
                }
            } else {
                self.parse_param_decl(name0_opt.take(), tparams)
            };
            if par.name.is_some() || par.ty.is_some() {
                if par.name.is_some() && par.ty.is_some() {
                    named += 1;
                }
                if par.ty.is_some() {
                    typed += 1;
                }
                list.push(par);
            }
            if !self.at_comma("parameter list", closing) {
                break;
            }
            self.next();
        }

        if list.is_empty() {
            return Vec::new();
        }

        if named == 0 {
            // All unnamed; treat any "name" as type.
            for par in list.iter_mut() {
                if let Some(name) = par.name.take() {
                    par.ty = Some(Expr::Ident(name));
                }
            }
        } else if named != list.len() {
            // Sweep right-to-left filling in missing types.
            let mut cur_typ: Option<Expr> = None;
            for par in list.iter_mut().rev() {
                if let Some(t) = par.ty.clone() {
                    cur_typ = Some(t);
                    if par.name.is_none() {
                        let pos = par.ty.as_ref().unwrap().pos();
                        let mut n = Ident::new_ident("_");
                        n.name_pos = pos;
                        par.name = Some(n);
                    }
                } else if let Some(t) = cur_typ.clone() {
                    par.ty = Some(t);
                } else {
                    let from = par.name.as_ref().map(|n| n.pos()).unwrap_or(NO_POS);
                    par.ty = Some(Expr::BadExpr(BadExpr {
                        id: 0,
                        from,
                        to: self.pos,
                    }));
                }
            }
        }

        // ... ellipsis usage check
        let total = list.len();
        let mut first = true;
        for (i, f) in list.iter_mut().enumerate() {
            if let Some(Expr::Ellipsis(e)) = &f.ty {
                let bad_pos = e.ellipsis;
                if !(dddok && i + 1 == total) {
                    if first {
                        first = false;
                        let msg = if dddok {
                            "can only use ... with final parameter"
                        } else {
                            "invalid use of ..."
                        };
                        self.error(bad_pos, msg);
                    }
                    let end = match &f.ty {
                        Some(Expr::Ellipsis(e)) => e
                            .elt
                            .as_ref()
                            .map(|x| x.end())
                            .unwrap_or(Pos(bad_pos.0 + 3)),
                        _ => bad_pos,
                    };
                    f.ty = Some(Expr::BadExpr(BadExpr {
                        id: 0,
                        from: bad_pos,
                        to: end,
                    }));
                }
            }
        }

        // Group consecutive params with the same type into a single Field.
        let mut params: Vec<Field> = Vec::new();
        if named == 0 {
            for par in list {
                params.push(Field {
                    doc: None,
                    names: vec![],
                    ty: par.ty,
                    tag: None,
                    comment: None,
                    id: 0,
                });
            }
            return params;
        }
        let mut current_names: Vec<Ident> = Vec::new();
        let mut current_ty: Option<Expr> = None;
        let mut add = |current_names: &mut Vec<Ident>,
                       current_ty: &mut Option<Expr>,
                       out: &mut Vec<Field>| {
            if !current_names.is_empty() {
                out.push(Field {
                    doc: None,
                    names: std::mem::take(current_names),
                    ty: current_ty.clone(),
                    tag: None,
                    comment: None,
                    id: 0,
                });
            }
        };
        for par in list {
            let same = match (&par.ty, &current_ty) {
                (Some(a), Some(b)) => expr_eq_shallow(a, b),
                _ => false,
            };
            if !same {
                add(&mut current_names, &mut current_ty, &mut params);
                current_ty = par.ty.clone();
            }
            if let Some(n) = par.name {
                current_names.push(n);
            }
        }
        add(&mut current_names, &mut current_ty, &mut params);

        // suppress unused warnings
        let _ = typed;
        params
    }

    fn parse_type_parameters(&mut self) -> Option<FieldList> {
        let lbrack = self.expect(Token::LBRACK);
        let list = if self.tok != Token::RBRACK {
            self.parse_parameter_list(None, None, Token::RBRACK, false)
        } else {
            Vec::new()
        };
        let rbrack = self.expect(Token::RBRACK);
        if list.is_empty() {
            self.error(rbrack, "empty type parameter list");
            return None;
        }
        Some(FieldList {
            opening: lbrack,
            list,
            closing: rbrack,
        })
    }

    fn parse_parameters(&mut self, result: bool) -> Option<FieldList> {
        if !result || self.tok == Token::LPAREN {
            let lparen = self.expect(Token::LPAREN);
            let list = if self.tok != Token::RPAREN {
                self.parse_parameter_list(None, None, Token::RPAREN, !result)
            } else {
                Vec::new()
            };
            let rparen = self.expect(Token::RPAREN);
            return Some(FieldList {
                opening: lparen,
                list,
                closing: rparen,
            });
        }
        if let Some(typ) = self.try_ident_or_type() {
            return Some(FieldList {
                opening: NO_POS,
                list: vec![Field {
                    doc: None,
                    names: vec![],
                    ty: Some(typ),
                    tag: None,
                    comment: None,
                    id: 0,
                }],
                closing: NO_POS,
            });
        }
        None
    }

    fn parse_func_type(&mut self) -> FuncType {
        let pos = self.expect(Token::FUNC);
        if self.tok == Token::LBRACK {
            if let Some(tp) = self.parse_type_parameters() {
                self.error(tp.opening, "function type must have no type parameters");
            }
        }
        let params = self.parse_parameters(false);
        let results = self.parse_parameters(true);
        FuncType {
            id: 0,
            func: pos,
            type_params: None,
            params,
            results,
        }
    }

    fn parse_method_spec(&mut self) -> Field {
        let doc = self.lead_comment.clone();
        let mut idents: Vec<Ident> = Vec::new();
        let typ;
        let x = self.parse_type_name(None);
        if let Expr::Ident(ident) = &x {
            let ident = ident.clone();
            if self.tok == Token::LBRACK {
                let lbrack = self.pos;
                self.next();
                self.expr_lev += 1;
                let inner = self.parse_expr();
                self.expr_lev -= 1;
                if let Expr::Ident(name0) = &inner {
                    if self.tok != Token::COMMA && self.tok != Token::RBRACK {
                        // generic method (disallowed; report but continue)
                        let name0 = name0.clone();
                        let _ = self.parse_parameter_list(Some(name0), None, Token::RBRACK, false);
                        self.expect(Token::RBRACK);
                        self.error(lbrack, "interface method must have no type parameters");
                        let params = self.parse_parameters(false);
                        let results = self.parse_parameters(true);
                        idents.push(ident.clone());
                        typ = Expr::FuncType(FuncType {
                            id: 0,
                            func: NO_POS,
                            type_params: None,
                            params,
                            results,
                        });
                        return Field {
                            doc,
                            names: idents,
                            ty: Some(typ),
                            tag: None,
                            comment: None,
                            id: 0,
                        };
                    }
                }
                // Embedded instantiated type.
                let mut list = vec![inner];
                if self.at_comma("type argument list", Token::RBRACK) {
                    self.expr_lev += 1;
                    self.next();
                    while self.tok != Token::RBRACK && self.tok != Token::EOF {
                        list.push(self.parse_type());
                        if !self.at_comma("type argument list", Token::RBRACK) {
                            break;
                        }
                        self.next();
                    }
                    self.expr_lev -= 1;
                }
                let rbrack = self.expect_closing(Token::RBRACK, "type argument list");
                typ = pack_index_expr(Expr::Ident(ident.clone()), lbrack, list, rbrack);
            } else if self.tok == Token::LPAREN {
                let params = self.parse_parameters(false);
                let results = self.parse_parameters(true);
                idents.push(ident.clone());
                typ = Expr::FuncType(FuncType {
                    id: 0,
                    func: NO_POS,
                    type_params: None,
                    params,
                    results,
                });
            } else {
                typ = Expr::Ident(ident);
            }
        } else {
            typ = if self.tok == Token::LBRACK {
                self.parse_type_instance(x)
            } else {
                x
            };
        }
        Field {
            doc,
            names: idents,
            ty: Some(typ),
            tag: None,
            comment: None,
            id: 0,
        }
    }

    fn embedded_elem(&mut self, x: Option<Expr>) -> Expr {
        let mut x = x.unwrap_or_else(|| self.embedded_term());
        while self.tok == Token::OR {
            let op_pos = self.pos;
            self.next();
            let y = self.embedded_term();
            x = Expr::BinaryExpr(BinaryExpr {
                id: 0,
                x: Box::new(x),
                op_pos,
                op: Token::OR,
                y: Box::new(y),
            });
        }
        x
    }

    fn embedded_term(&mut self) -> Expr {
        if self.tok == Token::TILDE {
            let op_pos = self.pos;
            self.next();
            let inner = self.parse_type();
            return Expr::UnaryExpr(UnaryExpr {
                id: 0,
                op_pos,
                op: Token::TILDE,
                x: Box::new(inner),
            });
        }
        match self.try_ident_or_type() {
            Some(t) => t,
            None => {
                let pos = self.pos;
                self.error_expected(pos, "~ term or type");
                self.advance_to(is_expr_end);
                Expr::BadExpr(BadExpr {
                    id: 0,
                    from: pos,
                    to: self.pos,
                })
            }
        }
    }

    fn parse_interface_type(&mut self) -> InterfaceType {
        let pos = self.expect(Token::INTERFACE);
        let lbrace = self.expect(Token::LBRACE);
        let mut list: Vec<Field> = Vec::new();
        loop {
            if self.tok == Token::IDENT {
                let mut f = self.parse_method_spec();
                if f.names.is_empty() {
                    f.ty = Some(self.embedded_elem(f.ty));
                }
                f.comment = self.expect_semi();
                list.push(f);
            } else if self.tok == Token::TILDE {
                let typ = self.embedded_elem(None);
                let comment = self.expect_semi();
                list.push(Field {
                    doc: None,
                    names: vec![],
                    ty: Some(typ),
                    tag: None,
                    comment,
                    id: 0,
                });
            } else if let Some(t) = self.try_ident_or_type() {
                let typ = self.embedded_elem(Some(t));
                let comment = self.expect_semi();
                list.push(Field {
                    doc: None,
                    names: vec![],
                    ty: Some(typ),
                    tag: None,
                    comment,
                    id: 0,
                });
            } else {
                break;
            }
        }
        let rbrace = self.expect(Token::RBRACE);
        InterfaceType {
            id: 0,
            interface_: pos,
            methods: FieldList {
                opening: lbrace,
                list,
                closing: rbrace,
            },
            incomplete: false,
        }
    }

    fn parse_map_type(&mut self) -> MapType {
        let pos = self.expect(Token::MAP);
        self.expect(Token::LBRACK);
        let key = self.parse_type();
        self.expect(Token::RBRACK);
        let value = self.parse_type();
        MapType {
            id: 0,
            map_: pos,
            key: Box::new(key),
            value: Box::new(value),
        }
    }

    fn parse_chan_type(&mut self) -> ChanType {
        let pos = self.pos;
        let mut dir = ChanDir(ChanDir::SEND.0 | ChanDir::RECV.0);
        let mut arrow = NO_POS;
        if self.tok == Token::CHAN {
            self.next();
            if self.tok == Token::ARROW {
                arrow = self.pos;
                self.next();
                dir = ChanDir::SEND;
            }
        } else {
            arrow = self.expect(Token::ARROW);
            self.expect(Token::CHAN);
            dir = ChanDir::RECV;
        }
        let value = self.parse_type();
        ChanType {
            id: 0,
            begin: pos,
            arrow,
            dir,
            value: Box::new(value),
        }
    }

    fn parse_type_instance(&mut self, typ: Expr) -> Expr {
        let opening = self.expect(Token::LBRACK);
        self.expr_lev += 1;
        let mut list: Vec<Expr> = Vec::new();
        while self.tok != Token::RBRACK && self.tok != Token::EOF {
            list.push(self.parse_type());
            if !self.at_comma("type argument list", Token::RBRACK) {
                break;
            }
            self.next();
        }
        self.expr_lev -= 1;
        let closing = self.expect_closing(Token::RBRACK, "type argument list");

        if list.is_empty() {
            self.error_expected(closing, "type argument list");
            return Expr::IndexExpr(IndexExpr {
                id: 0,
                x: Box::new(typ),
                lbrack: opening,
                index: Box::new(Expr::BadExpr(BadExpr {
                    id: 0,
                    from: Pos(opening.0 + 1),
                    to: closing,
                })),
                rbrack: closing,
            });
        }
        pack_index_expr(typ, opening, list, closing)
    }

    fn try_ident_or_type(&mut self) -> Option<Expr> {
        self.with_nest(|p| match p.tok {
            Token::IDENT => {
                let typ = p.parse_type_name(None);
                if p.tok == Token::LBRACK {
                    Some(p.parse_type_instance(typ))
                } else {
                    Some(typ)
                }
            }
            Token::LBRACK => {
                let lbrack = p.expect(Token::LBRACK);
                Some(Expr::ArrayType(p.parse_array_type(lbrack, None)))
            }
            Token::STRUCT => Some(Expr::StructType(p.parse_struct_type())),
            Token::MUL => Some(Expr::StarExpr(p.parse_pointer_type())),
            Token::FUNC => Some(Expr::FuncType(p.parse_func_type())),
            Token::INTERFACE => Some(Expr::InterfaceType(p.parse_interface_type())),
            Token::MAP => Some(Expr::MapType(p.parse_map_type())),
            Token::CHAN | Token::ARROW => Some(Expr::ChanType(p.parse_chan_type())),
            Token::LPAREN => {
                let lparen = p.pos;
                p.next();
                let typ = p.parse_type();
                let rparen = p.expect(Token::RPAREN);
                Some(Expr::ParenExpr(ParenExpr {
                    id: 0,
                    lparen,
                    x: Box::new(typ),
                    rparen,
                }))
            }
            _ => None,
        })
    }

    // ---- statements / blocks --------------------------------------

    fn parse_stmt_list(&mut self) -> Vec<Stmt> {
        let mut list = Vec::new();
        while !matches!(
            self.tok,
            Token::CASE | Token::DEFAULT | Token::RBRACE | Token::EOF
        ) {
            list.push(self.parse_stmt());
        }
        list
    }

    fn parse_body(&mut self) -> BlockStmt {
        let lbrace = self.expect(Token::LBRACE);
        let list = self.parse_stmt_list();
        let rbrace = self.expect2(Token::RBRACE);
        BlockStmt {
            lbrace,
            list,
            rbrace,
            id: 0,
        }
    }

    fn parse_block_stmt(&mut self) -> BlockStmt {
        self.parse_body()
    }

    // ---- expressions ----------------------------------------------

    fn parse_func_type_or_lit(&mut self) -> Expr {
        let typ = self.parse_func_type();
        if self.tok != Token::LBRACE {
            return Expr::FuncType(typ);
        }
        self.expr_lev += 1;
        let body = self.parse_body();
        self.expr_lev -= 1;
        Expr::FuncLit(FuncLit {
            id: 0,
            ty: typ,
            body,
        })
    }

    fn parse_operand(&mut self) -> Expr {
        match self.tok {
            Token::IDENT => Expr::Ident(self.parse_ident()),
            Token::INT | Token::FLOAT | Token::IMAG | Token::CHAR | Token::STRING => {
                let end = if self.tok == Token::STRING {
                    self.string_end
                } else {
                    Pos(self.pos.0 + self.lit.len() as i64)
                };
                let bl = BasicLit {
                    id: 0,
                    value_pos: self.pos,
                    value_end: end,
                    kind: Some(self.tok),
                    value: self.lit.clone(),
                };
                self.next();
                Expr::BasicLit(bl)
            }
            Token::LPAREN => {
                let lparen = self.pos;
                self.next();
                self.expr_lev += 1;
                let x = self.parse_rhs();
                self.expr_lev -= 1;
                let rparen = self.expect(Token::RPAREN);
                Expr::ParenExpr(ParenExpr {
                    id: 0,
                    lparen,
                    x: Box::new(x),
                    rparen,
                })
            }
            Token::FUNC => self.parse_func_type_or_lit(),
            _ => {
                if let Some(typ) = self.try_ident_or_type() {
                    return typ;
                }
                let pos = self.pos;
                self.error_expected(pos, "operand");
                self.advance_to(is_stmt_start);
                Expr::BadExpr(BadExpr {
                    id: 0,
                    from: pos,
                    to: self.pos,
                })
            }
        }
    }

    fn parse_selector(&mut self, x: Expr) -> Expr {
        let sel = self.parse_ident();
        Expr::SelectorExpr(SelectorExpr {
            id: 0,
            x: Box::new(x),
            sel,
        })
    }

    fn parse_type_assertion(&mut self, x: Expr) -> Expr {
        let lparen = self.expect(Token::LPAREN);
        let typ = if self.tok == Token::TYPE {
            self.next();
            None
        } else {
            Some(Box::new(self.parse_type()))
        };
        let rparen = self.expect(Token::RPAREN);
        Expr::TypeAssertExpr(TypeAssertExpr {
            id: 0,
            x: Box::new(x),
            lparen,
            ty: typ,
            rparen,
        })
    }

    fn parse_index_or_slice_or_instance(&mut self, x: Expr) -> Expr {
        let lbrack = self.expect(Token::LBRACK);
        if self.tok == Token::RBRACK {
            self.error_expected(self.pos, "operand");
            let rbrack = self.pos;
            self.next();
            return Expr::IndexExpr(IndexExpr {
                id: 0,
                x: Box::new(x),
                lbrack,
                index: Box::new(Expr::BadExpr(BadExpr {
                    id: 0,
                    from: rbrack,
                    to: rbrack,
                })),
                rbrack,
            });
        }
        self.expr_lev += 1;
        const N: usize = 3;
        let mut index: [Option<Expr>; N] = [None, None, None];
        let mut colons: [Pos; 2] = [NO_POS, NO_POS];
        let mut args: Vec<Expr> = Vec::new();
        if self.tok != Token::COLON {
            index[0] = Some(self.parse_rhs());
        }
        let mut ncolons = 0;
        match self.tok {
            Token::COLON => {
                while self.tok == Token::COLON && ncolons < colons.len() {
                    colons[ncolons] = self.pos;
                    ncolons += 1;
                    self.next();
                    if !matches!(self.tok, Token::COLON | Token::RBRACK | Token::EOF) {
                        index[ncolons] = Some(self.parse_rhs());
                    }
                }
            }
            Token::COMMA => {
                args.push(index[0].take().unwrap());
                while self.tok == Token::COMMA {
                    self.next();
                    if !matches!(self.tok, Token::RBRACK | Token::EOF) {
                        args.push(self.parse_type());
                    }
                }
            }
            _ => {}
        }
        self.expr_lev -= 1;
        let rbrack = self.expect(Token::RBRACK);

        if ncolons > 0 {
            let slice3 = ncolons == 2;
            let low = index[0].take();
            let mut high = index[1].take();
            let mut max = index[2].take();
            if slice3 {
                if high.is_none() {
                    self.error(colons[0], "middle index required in 3-index slice");
                    high = Some(Expr::BadExpr(BadExpr {
                        id: 0,
                        from: Pos(colons[0].0 + 1),
                        to: colons[1],
                    }));
                }
                if max.is_none() {
                    self.error(colons[1], "final index required in 3-index slice");
                    max = Some(Expr::BadExpr(BadExpr {
                        id: 0,
                        from: Pos(colons[1].0 + 1),
                        to: rbrack,
                    }));
                }
            }
            return Expr::SliceExpr(SliceExpr {
                id: 0,
                x: Box::new(x),
                lbrack,
                low: low.map(Box::new),
                high: high.map(Box::new),
                max: max.map(Box::new),
                slice3,
                rbrack,
            });
        }
        if args.is_empty() {
            return Expr::IndexExpr(IndexExpr {
                id: 0,
                x: Box::new(x),
                lbrack,
                index: Box::new(index[0].take().unwrap()),
                rbrack,
            });
        }
        pack_index_expr(x, lbrack, args, rbrack)
    }

    fn parse_call_or_conversion(&mut self, fun: Expr) -> CallExpr {
        let lparen = self.expect(Token::LPAREN);
        self.expr_lev += 1;
        let mut list: Vec<Expr> = Vec::new();
        let mut ellipsis = NO_POS;
        while self.tok != Token::RPAREN && self.tok != Token::EOF && !ellipsis.is_valid() {
            list.push(self.parse_rhs());
            if self.tok == Token::ELLIPSIS {
                ellipsis = self.pos;
                self.next();
            }
            if !self.at_comma("argument list", Token::RPAREN) {
                break;
            }
            self.next();
        }
        self.expr_lev -= 1;
        let rparen = self.expect_closing(Token::RPAREN, "argument list");
        CallExpr {
            id: 0,
            fun: Box::new(fun),
            lparen,
            args: list,
            ellipsis,
            rparen,
        }
    }

    fn parse_value(&mut self) -> Expr {
        if self.tok == Token::LBRACE {
            self.parse_literal_value(None)
        } else {
            self.parse_expr()
        }
    }

    fn parse_element(&mut self) -> Expr {
        let x = self.parse_value();
        if self.tok == Token::COLON {
            let colon = self.pos;
            self.next();
            return Expr::KeyValueExpr(KeyValueExpr {
                id: 0,
                key: Box::new(x),
                colon,
                value: Box::new(self.parse_value()),
            });
        }
        x
    }

    fn parse_element_list(&mut self) -> Vec<Expr> {
        let mut list = Vec::new();
        while self.tok != Token::RBRACE && self.tok != Token::EOF {
            list.push(self.parse_element());
            if !self.at_comma("composite literal", Token::RBRACE) {
                break;
            }
            self.next();
        }
        list
    }

    fn parse_literal_value(&mut self, typ: Option<Expr>) -> Expr {
        self.with_nest(|p| {
            let lbrace = p.expect(Token::LBRACE);
            let mut elts: Vec<Expr> = Vec::new();
            p.expr_lev += 1;
            if p.tok != Token::RBRACE {
                elts = p.parse_element_list();
            }
            p.expr_lev -= 1;
            let rbrace = p.expect_closing(Token::RBRACE, "composite literal");
            Expr::CompositeLit(CompositeLit {
                id: 0,
                ty: typ.map(Box::new),
                lbrace,
                elts,
                rbrace,
                incomplete: false,
            })
        })
    }

    fn parse_primary_expr(&mut self, x0: Option<Expr>) -> Expr {
        let mut x = x0.unwrap_or_else(|| self.parse_operand());
        let mut n = 0usize;
        loop {
            n += 1;
            self.inc_nest_lev();
            match self.tok {
                Token::PERIOD => {
                    self.next();
                    match self.tok {
                        Token::IDENT => x = self.parse_selector(x),
                        Token::LPAREN => x = self.parse_type_assertion(x),
                        _ => {
                            let pos = self.pos;
                            self.error_expected(pos, "selector or type assertion");
                            if self.tok != Token::RBRACE {
                                self.next();
                            }
                            let sel = Ident {
                                name_pos: pos,
                                name: "_".to_string(),
                                obj: std::sync::Mutex::new(None),
                                id: crate::ast::next_node_id(),
                            };
                            x = Expr::SelectorExpr(SelectorExpr {
                                id: 0,
                                x: Box::new(x),
                                sel,
                            });
                        }
                    }
                }
                Token::LBRACK => x = self.parse_index_or_slice_or_instance(x),
                Token::LPAREN => x = Expr::CallExpr(self.parse_call_or_conversion(x)),
                Token::LBRACE => {
                    let t = unparen_ref(&x);
                    let composite_lit_ok = match t {
                        Expr::BadExpr(_)
                        | Expr::Ident(_)
                        | Expr::SelectorExpr(_)
                        | Expr::IndexExpr(_)
                        | Expr::IndexListExpr(_) => self.expr_lev >= 0,
                        Expr::ArrayType(_) | Expr::StructType(_) | Expr::MapType(_) => true,
                        _ => false,
                    };
                    if !composite_lit_ok {
                        self.nest_lev -= n;
                        return x;
                    }
                    if !std::ptr::eq(t as *const _, &x as *const _) {
                        self.error(t.pos(), "cannot parenthesize type in composite literal");
                    }
                    x = self.parse_literal_value(Some(x));
                }
                _ => {
                    self.nest_lev -= n;
                    return x;
                }
            }
        }
    }

    fn parse_unary_expr(&mut self) -> Expr {
        self.with_nest(|p| match p.tok {
            Token::ADD | Token::SUB | Token::NOT | Token::XOR | Token::AND | Token::TILDE => {
                let op_pos = p.pos;
                let op = p.tok;
                p.next();
                let x = p.parse_unary_expr();
                Expr::UnaryExpr(UnaryExpr {
                    id: 0,
                    op_pos,
                    op,
                    x: Box::new(x),
                })
            }
            Token::ARROW => {
                let arrow = p.pos;
                p.next();
                let x = p.parse_unary_expr();
                if let Expr::ChanType(mut ct) = x {
                    // Re-associate arrow with channel type.
                    let mut current_dir = ChanDir::SEND;
                    loop {
                        if ct.dir == ChanDir::RECV {
                            p.error_expected(ct.arrow, "'chan'");
                        }
                        let old_arrow = ct.arrow;
                        ct.arrow = arrow;
                        ct.begin = arrow;
                        let _ = old_arrow;
                        let old_dir = ct.dir;
                        ct.dir = ChanDir::RECV;
                        current_dir = old_dir;
                        if let Expr::ChanType(inner) = ct.value.as_ref() {
                            let inner_clone = inner.clone();
                            ct.value = Box::new(Expr::ChanType(inner_clone));
                            // Loop into the inner ChanType by repeating
                            // — simplified: only re-associate once.
                            break;
                        } else {
                            break;
                        }
                    }
                    if current_dir == ChanDir::SEND {
                        // (best-effort; matches Go behavior approximately)
                    }
                    return Expr::ChanType(ct);
                }
                Expr::UnaryExpr(UnaryExpr {
                    id: 0,
                    op_pos: arrow,
                    op: Token::ARROW,
                    x: Box::new(x),
                })
            }
            Token::MUL => {
                let pos = p.pos;
                p.next();
                let x = p.parse_unary_expr();
                Expr::StarExpr(StarExpr {
                    id: 0,
                    star: pos,
                    x: Box::new(x),
                })
            }
            _ => p.parse_primary_expr(None),
        })
    }

    fn tok_prec(&self) -> (Token, i32) {
        let mut tok = self.tok;
        if self.in_rhs && tok == Token::ASSIGN {
            tok = Token::EQL;
        }
        (tok, tok.precedence())
    }

    fn parse_binary_expr(&mut self, x0: Option<Expr>, prec1: i32) -> Expr {
        let mut x = x0.unwrap_or_else(|| self.parse_unary_expr());
        let mut n = 0usize;
        loop {
            n += 1;
            self.inc_nest_lev();
            let (op, oprec) = self.tok_prec();
            if oprec < prec1 {
                self.nest_lev -= n;
                return x;
            }
            let op_pos = self.expect(op);
            let y = self.parse_binary_expr(None, oprec + 1);
            x = Expr::BinaryExpr(BinaryExpr {
                id: 0,
                x: Box::new(x),
                op_pos,
                op,
                y: Box::new(y),
            });
        }
    }

    fn parse_expr(&mut self) -> Expr {
        self.parse_binary_expr(None, LOWEST_PREC + 1)
    }

    fn parse_rhs(&mut self) -> Expr {
        let old = self.in_rhs;
        self.in_rhs = true;
        let x = self.parse_expr();
        self.in_rhs = old;
        x
    }

    // ---- statements -----------------------------------------------

    fn parse_simple_stmt(&mut self, mode: u32) -> (Stmt, bool) {
        let x = self.parse_list(false);
        let assign_toks = [
            Token::DEFINE,
            Token::ASSIGN,
            Token::AddAssign,
            Token::SubAssign,
            Token::MulAssign,
            Token::QuoAssign,
            Token::RemAssign,
            Token::AndAssign,
            Token::OrAssign,
            Token::XorAssign,
            Token::ShlAssign,
            Token::ShrAssign,
            Token::AndNotAssign,
        ];
        if assign_toks.contains(&self.tok) {
            let pos = self.pos;
            let tok = self.tok;
            self.next();
            let mut is_range = false;
            let y = if mode == SS_RANGE_OK
                && self.tok == Token::RANGE
                && (tok == Token::DEFINE || tok == Token::ASSIGN)
            {
                let rpos = self.pos;
                self.next();
                is_range = true;
                vec![Expr::UnaryExpr(UnaryExpr {
                    id: 0,
                    op_pos: rpos,
                    op: Token::RANGE,
                    x: Box::new(self.parse_rhs()),
                })]
            } else {
                self.parse_list(true)
            };
            return (
                Stmt::AssignStmt(AssignStmt {
                    lhs: x,
                    tok_pos: pos,
                    tok: Some(tok),
                    rhs: y,
                }),
                is_range,
            );
        }

        if x.len() > 1 {
            self.error_expected(x[0].pos(), "1 expression");
        }
        let first = x.into_iter().next().unwrap();

        match self.tok {
            Token::COLON => {
                let colon = self.pos;
                self.next();
                if mode == SS_LABEL_OK {
                    if let Expr::Ident(label) = first.clone() {
                        let stmt = self.parse_stmt();
                        return (
                            Stmt::LabeledStmt(LabeledStmt {
                                label,
                                colon,
                                stmt: Box::new(stmt),
                            }),
                            false,
                        );
                    }
                }
                self.error(colon, "illegal label declaration");
                (
                    Stmt::BadStmt(BadStmt {
                        from: first.pos(),
                        to: Pos(colon.0 + 1),
                    }),
                    false,
                )
            }
            Token::ARROW => {
                let arrow = self.pos;
                self.next();
                let y = self.parse_rhs();
                (
                    Stmt::SendStmt(SendStmt {
                        chan_: first,
                        arrow,
                        value: y,
                    }),
                    false,
                )
            }
            Token::INC | Token::DEC => {
                let tok = self.tok;
                let tok_pos = self.pos;
                self.next();
                (
                    Stmt::IncDecStmt(IncDecStmt {
                        x: first,
                        tok_pos,
                        tok,
                    }),
                    false,
                )
            }
            _ => (Stmt::ExprStmt(ExprStmt { x: first }), false),
        }
    }

    fn parse_call_expr(&mut self, call_type: &str) -> Option<CallExpr> {
        let x = self.parse_rhs();
        let x_pos = x.pos();
        let x_end = x.end();
        // Go: `if t := ast.Unparen(x); t != x { ... }` — the expression must not
        // be parenthesized. `unparen_ref` returns `&x` itself when there are no
        // parens to strip, so a pointer-inequality means parens were removed.
        // (The previous `unparen(x.clone())` compared two distinct locals and so
        // always reported the error, breaking every `go`/`defer` call.)
        if !std::ptr::eq(unparen_ref(&x) as *const Expr, &x as *const Expr) {
            self.error(
                x_pos,
                format!("expression in {} must not be parenthesized", call_type),
            );
        }
        if let Expr::CallExpr(c) = unparen(x) {
            return Some(c);
        }
        // not a call
        self.error(
            x_end,
            format!("expression in {} must be function call", call_type),
        );
        None
    }

    fn parse_go_stmt(&mut self) -> Stmt {
        let pos = self.expect(Token::GO);
        let call = self.parse_call_expr("go");
        self.expect_semi();
        match call {
            Some(c) => Stmt::GoStmt(GoStmt { go_: pos, call: c }),
            None => Stmt::BadStmt(BadStmt {
                from: pos,
                to: Pos(pos.0 + 2),
            }),
        }
    }

    fn parse_defer_stmt(&mut self) -> Stmt {
        let pos = self.expect(Token::DEFER);
        let call = self.parse_call_expr("defer");
        self.expect_semi();
        match call {
            Some(c) => Stmt::DeferStmt(DeferStmt {
                defer_: pos,
                call: c,
            }),
            None => Stmt::BadStmt(BadStmt {
                from: pos,
                to: Pos(pos.0 + 5),
            }),
        }
    }

    fn parse_return_stmt(&mut self) -> ReturnStmt {
        let pos = self.pos;
        self.expect(Token::RETURN);
        let results = if self.tok != Token::SEMICOLON && self.tok != Token::RBRACE {
            self.parse_list(true)
        } else {
            Vec::new()
        };
        self.expect_semi();
        ReturnStmt {
            return_: pos,
            results,
        }
    }

    fn parse_branch_stmt(&mut self, tok: Token) -> BranchStmt {
        let pos = self.expect(tok);
        let label = if tok == Token::GOTO
            || ((tok == Token::CONTINUE || tok == Token::BREAK) && self.tok == Token::IDENT)
        {
            Some(self.parse_ident())
        } else {
            None
        };
        self.expect_semi();
        BranchStmt {
            tok_pos: pos,
            tok,
            label,
        }
    }

    fn make_expr(&mut self, s: Option<Stmt>, want: &str) -> Option<Expr> {
        match s {
            None => None,
            Some(Stmt::ExprStmt(e)) => Some(e.x),
            Some(other) => {
                let pos = other.pos();
                let end = other.end();
                let found = if matches!(other, Stmt::AssignStmt(_)) {
                    "assignment"
                } else {
                    "simple statement"
                };
                self.error(
                    pos,
                    format!(
                        "expected {}, found {} (missing parentheses around composite literal?)",
                        want, found
                    ),
                );
                Some(Expr::BadExpr(BadExpr {
                    id: 0,
                    from: pos,
                    to: end,
                }))
            }
        }
    }

    fn parse_if_header(&mut self) -> (Option<Stmt>, Expr) {
        if self.tok == Token::LBRACE {
            self.error(self.pos, "missing condition in if statement");
            return (
                None,
                Expr::BadExpr(BadExpr {
                    id: 0,
                    from: self.pos,
                    to: self.pos,
                }),
            );
        }
        let prev_lev = self.expr_lev;
        self.expr_lev = -1;

        let mut init = None;
        if self.tok != Token::SEMICOLON {
            if self.tok == Token::VAR {
                self.next();
                self.error(self.pos, "var declaration not allowed in if initializer");
            }
            init = Some(self.parse_simple_stmt(SS_BASIC).0);
        }

        let mut cond_stmt: Option<Stmt> = None;
        let mut semi_pos = NO_POS;
        let mut semi_lit = String::new();
        if self.tok != Token::LBRACE {
            if self.tok == Token::SEMICOLON {
                semi_pos = self.pos;
                semi_lit = self.lit.clone();
                self.next();
            } else {
                self.expect(Token::SEMICOLON);
            }
            if self.tok != Token::LBRACE {
                cond_stmt = Some(self.parse_simple_stmt(SS_BASIC).0);
            }
        } else {
            cond_stmt = init.take();
        }

        let cond = if let Some(s) = cond_stmt {
            self.make_expr(Some(s), "boolean expression")
                .unwrap_or(Expr::BadExpr(BadExpr {
                    id: 0,
                    from: self.pos,
                    to: self.pos,
                }))
        } else {
            if semi_pos.is_valid() {
                if semi_lit == "\n" {
                    self.error(semi_pos, "unexpected newline, expecting { after if clause");
                } else {
                    self.error(semi_pos, "missing condition in if statement");
                }
            }
            Expr::BadExpr(BadExpr {
                id: 0,
                from: self.pos,
                to: self.pos,
            })
        };

        self.expr_lev = prev_lev;
        (init, cond)
    }

    fn parse_if_stmt(&mut self) -> IfStmt {
        self.inc_nest_lev();
        let pos = self.expect(Token::IF);
        let (init, cond) = self.parse_if_header();
        let body = self.parse_block_stmt();
        let else_ = if self.tok == Token::ELSE {
            self.next();
            match self.tok {
                Token::IF => Some(Stmt::IfStmt(self.parse_if_stmt())),
                Token::LBRACE => {
                    let b = self.parse_block_stmt();
                    self.expect_semi();
                    Some(Stmt::BlockStmt(b))
                }
                _ => {
                    self.error_expected(self.pos, "if statement or block");
                    Some(Stmt::BadStmt(BadStmt {
                        from: self.pos,
                        to: self.pos,
                    }))
                }
            }
        } else {
            self.expect_semi();
            None
        };
        self.dec_nest_lev();
        IfStmt {
            if_: pos,
            init: init.map(Box::new),
            cond,
            body,
            else_: else_.map(Box::new),
            id: 0,
        }
    }

    fn parse_case_clause(&mut self) -> CaseClause {
        let pos = self.pos;
        let list = if self.tok == Token::CASE {
            self.next();
            self.parse_list(true)
        } else {
            self.expect(Token::DEFAULT);
            Vec::new()
        };
        let colon = self.expect(Token::COLON);
        let body = self.parse_stmt_list();
        CaseClause {
            case: pos,
            list,
            colon,
            body,
            id: 0,
        }
    }

    fn parse_switch_stmt(&mut self) -> Stmt {
        let pos = self.expect(Token::SWITCH);
        let mut s1: Option<Stmt> = None;
        let mut s2: Option<Stmt> = None;
        if self.tok != Token::LBRACE {
            let prev_lev = self.expr_lev;
            self.expr_lev = -1;
            if self.tok != Token::SEMICOLON {
                s2 = Some(self.parse_simple_stmt(SS_BASIC).0);
            }
            if self.tok == Token::SEMICOLON {
                self.next();
                s1 = s2.take();
                if self.tok != Token::LBRACE {
                    s2 = Some(self.parse_simple_stmt(SS_BASIC).0);
                }
            }
            self.expr_lev = prev_lev;
        }

        let type_switch = is_type_switch_guard(&s2);
        let lbrace = self.expect(Token::LBRACE);
        let mut list: Vec<Stmt> = Vec::new();
        while matches!(self.tok, Token::CASE | Token::DEFAULT) {
            list.push(Stmt::CaseClause(self.parse_case_clause()));
        }
        let rbrace = self.expect(Token::RBRACE);
        self.expect_semi();
        let body = BlockStmt {
            lbrace,
            list,
            rbrace,
            id: 0,
        };

        if type_switch {
            let assign = s2.expect("type switch guard");
            Stmt::TypeSwitchStmt(TypeSwitchStmt {
                switch: pos,
                init: s1.map(Box::new),
                assign: Box::new(assign),
                body,
                id: 0,
            })
        } else {
            let tag = self.make_expr(s2, "switch expression");
            Stmt::SwitchStmt(SwitchStmt {
                switch: pos,
                init: s1.map(Box::new),
                tag,
                body,
                id: 0,
            })
        }
    }

    fn parse_comm_clause(&mut self) -> CommClause {
        let pos = self.pos;
        let mut comm: Option<Stmt> = None;
        if self.tok == Token::CASE {
            self.next();
            let lhs = self.parse_list(false);
            if self.tok == Token::ARROW {
                if lhs.len() > 1 {
                    self.error_expected(lhs[0].pos(), "1 expression");
                }
                let arrow = self.pos;
                self.next();
                let rhs = self.parse_rhs();
                let chan_ = lhs.into_iter().next().unwrap();
                comm = Some(Stmt::SendStmt(SendStmt {
                    chan_,
                    arrow,
                    value: rhs,
                }));
            } else if self.tok == Token::ASSIGN || self.tok == Token::DEFINE {
                let tok = self.tok;
                let mut lhs = lhs;
                if lhs.len() > 2 {
                    self.error_expected(lhs[0].pos(), "1 or 2 expressions");
                    lhs.truncate(2);
                }
                let tok_pos = self.pos;
                self.next();
                let rhs = self.parse_rhs();
                comm = Some(Stmt::AssignStmt(AssignStmt {
                    lhs,
                    tok_pos,
                    tok: Some(tok),
                    rhs: vec![rhs],
                }));
            } else {
                if lhs.len() > 1 {
                    self.error_expected(lhs[0].pos(), "1 expression");
                }
                comm = Some(Stmt::ExprStmt(ExprStmt {
                    x: lhs.into_iter().next().unwrap(),
                }));
            }
        } else {
            self.expect(Token::DEFAULT);
        }
        let colon = self.expect(Token::COLON);
        let body = self.parse_stmt_list();
        CommClause {
            case: pos,
            comm: comm.map(Box::new),
            colon,
            body,
            id: 0,
        }
    }

    fn parse_select_stmt(&mut self) -> SelectStmt {
        let pos = self.expect(Token::SELECT);
        let lbrace = self.expect(Token::LBRACE);
        let mut list: Vec<Stmt> = Vec::new();
        while matches!(self.tok, Token::CASE | Token::DEFAULT) {
            list.push(Stmt::CommClause(self.parse_comm_clause()));
        }
        let rbrace = self.expect(Token::RBRACE);
        self.expect_semi();
        SelectStmt {
            select_: pos,
            body: BlockStmt {
                lbrace,
                list,
                rbrace,
                id: 0,
            },
        }
    }

    fn parse_for_stmt(&mut self) -> Stmt {
        let pos = self.expect(Token::FOR);
        let mut s1: Option<Stmt> = None;
        let mut s2: Option<Stmt> = None;
        let mut s3: Option<Stmt> = None;
        let mut is_range = false;
        if self.tok != Token::LBRACE {
            let prev_lev = self.expr_lev;
            self.expr_lev = -1;
            if self.tok != Token::SEMICOLON {
                if self.tok == Token::RANGE {
                    let rpos = self.pos;
                    self.next();
                    let y = vec![Expr::UnaryExpr(UnaryExpr {
                        id: 0,
                        op_pos: rpos,
                        op: Token::RANGE,
                        x: Box::new(self.parse_rhs()),
                    })];
                    s2 = Some(Stmt::AssignStmt(AssignStmt {
                        lhs: Vec::new(),
                        tok_pos: NO_POS,
                        tok: None,
                        rhs: y,
                    }));
                    is_range = true;
                } else {
                    let (st, isr) = self.parse_simple_stmt(SS_RANGE_OK);
                    s2 = Some(st);
                    is_range = isr;
                }
            }
            if !is_range && self.tok == Token::SEMICOLON {
                self.next();
                s1 = s2.take();
                if self.tok != Token::SEMICOLON {
                    s2 = Some(self.parse_simple_stmt(SS_BASIC).0);
                }
                self.expect_semi();
                if self.tok != Token::LBRACE {
                    s3 = Some(self.parse_simple_stmt(SS_BASIC).0);
                }
            }
            self.expr_lev = prev_lev;
        }
        let body = self.parse_block_stmt();
        self.expect_semi();
        if is_range {
            let as_stmt = match s2.unwrap() {
                Stmt::AssignStmt(a) => a,
                _ => unreachable!(),
            };
            let (key, value) = match as_stmt.lhs.len() {
                0 => (None, None),
                1 => (Some(as_stmt.lhs.into_iter().next().unwrap()), None),
                _ => {
                    let mut it = as_stmt.lhs.into_iter();
                    (it.next(), it.next())
                }
            };
            let range_x = match as_stmt.rhs.into_iter().next().unwrap() {
                Expr::UnaryExpr(u) => *u.x,
                other => other,
            };
            return Stmt::RangeStmt(RangeStmt {
                for_: pos,
                key,
                value,
                tok_pos: as_stmt.tok_pos,
                tok: as_stmt.tok,
                range_: NO_POS,
                x: range_x,
                body,
                id: 0,
            });
        }
        let cond = self.make_expr(s2, "boolean or range expression");
        Stmt::ForStmt(ForStmt {
            for_: pos,
            init: s1.map(Box::new),
            cond,
            post: s3.map(Box::new),
            body,
            id: 0,
        })
    }

    fn parse_stmt(&mut self) -> Stmt {
        self.with_nest(|p| match p.tok {
            Token::CONST | Token::TYPE | Token::VAR => Stmt::DeclStmt(DeclStmt {
                decl: p.parse_decl(is_stmt_start),
            }),
            Token::IDENT
            | Token::INT
            | Token::FLOAT
            | Token::IMAG
            | Token::CHAR
            | Token::STRING
            | Token::FUNC
            | Token::LPAREN
            | Token::LBRACK
            | Token::STRUCT
            | Token::MAP
            | Token::CHAN
            | Token::INTERFACE
            | Token::ADD
            | Token::SUB
            | Token::MUL
            | Token::AND
            | Token::XOR
            | Token::ARROW
            | Token::NOT => {
                let (s, _) = p.parse_simple_stmt(SS_LABEL_OK);
                if !matches!(&s, Stmt::LabeledStmt(_)) {
                    p.expect_semi();
                }
                s
            }
            Token::GO => p.parse_go_stmt(),
            Token::DEFER => p.parse_defer_stmt(),
            Token::RETURN => Stmt::ReturnStmt(p.parse_return_stmt()),
            Token::BREAK | Token::CONTINUE | Token::GOTO | Token::FALLTHROUGH => {
                Stmt::BranchStmt(p.parse_branch_stmt(p.tok))
            }
            Token::LBRACE => {
                let b = p.parse_block_stmt();
                p.expect_semi();
                Stmt::BlockStmt(b)
            }
            Token::IF => Stmt::IfStmt(p.parse_if_stmt()),
            Token::SWITCH => p.parse_switch_stmt(),
            Token::SELECT => Stmt::SelectStmt(p.parse_select_stmt()),
            Token::FOR => p.parse_for_stmt(),
            Token::SEMICOLON => {
                let implicit = p.lit == "\n";
                let pos = p.pos;
                p.next();
                Stmt::EmptyStmt(EmptyStmt {
                    semicolon: pos,
                    implicit,
                })
            }
            Token::RBRACE => Stmt::EmptyStmt(EmptyStmt {
                semicolon: p.pos,
                implicit: true,
            }),
            _ => {
                let pos = p.pos;
                p.error_expected(pos, "statement");
                p.advance_to(is_stmt_start);
                Stmt::BadStmt(BadStmt {
                    from: pos,
                    to: p.pos,
                })
            }
        })
    }

    // ---- declarations ---------------------------------------------

    fn parse_import_spec(&mut self, doc: Option<CommentGroup>) -> Spec {
        let mut ident: Option<Ident> = None;
        match self.tok {
            Token::IDENT => ident = Some(self.parse_ident()),
            Token::PERIOD => {
                ident = Some(Ident {
                    name_pos: self.pos,
                    name: ".".to_string(),
                    obj: std::sync::Mutex::new(None),
                    id: crate::ast::next_node_id(),
                });
                self.next();
            }
            _ => {}
        }
        let pos = self.pos;
        let mut end = self.pos;
        let mut path = String::new();
        if self.tok == Token::STRING {
            path = self.lit.clone();
            end = self.string_end;
            self.next();
        } else if self.tok.is_literal() {
            self.error(pos, "import path must be a string");
            self.next();
        } else {
            self.error(pos, "missing import path");
            self.advance_to(is_expr_end);
        }
        let comment = self.expect_semi();
        let spec = ImportSpec {
            doc,
            name: ident,
            path: BasicLit {
                id: 0,
                value_pos: pos,
                value_end: end,
                kind: Some(Token::STRING),
                value: path,
            },
            comment,
            end_pos: end,
            id: 0,
        };
        self.imports.push(spec.clone());
        Spec::ImportSpec(spec)
    }

    fn parse_value_spec(&mut self, doc: Option<CommentGroup>, keyword: Token) -> Spec {
        let idents = self.parse_ident_list();
        let mut typ: Option<Expr> = None;
        let mut values: Vec<Expr> = Vec::new();
        match keyword {
            Token::CONST => {
                if !matches!(self.tok, Token::EOF | Token::SEMICOLON | Token::RPAREN) {
                    typ = self.try_ident_or_type();
                    if self.tok == Token::ASSIGN {
                        self.next();
                        values = self.parse_list(true);
                    }
                }
            }
            Token::VAR => {
                if self.tok != Token::ASSIGN {
                    typ = Some(self.parse_type());
                }
                if self.tok == Token::ASSIGN {
                    self.next();
                    values = self.parse_list(true);
                }
            }
            _ => unreachable!(),
        }
        let comment = self.expect_semi();
        Spec::ValueSpec(ValueSpec {
            doc,
            names: idents,
            ty: typ,
            values,
            comment,
        })
    }

    fn parse_type_spec(&mut self, doc: Option<CommentGroup>) -> Spec {
        let name = self.parse_ident();
        let mut type_params: Option<FieldList> = None;
        let mut assign = NO_POS;
        let typ: Expr;
        if self.tok == Token::LBRACK {
            let lbrack = self.pos;
            self.next();
            if self.tok == Token::IDENT {
                let mut x: Expr = Expr::Ident(self.parse_ident());
                if self.tok != Token::LBRACK {
                    self.expr_lev += 1;
                    let lhs = self.parse_primary_expr(Some(x));
                    x = self.parse_binary_expr(Some(lhs), LOWEST_PREC + 1);
                    self.expr_lev -= 1;
                }
                let force = self.tok == Token::COMMA;
                let (pname, ptype) = extract_name(x.clone(), force);
                let is_array = pname.is_some() && (ptype.is_some() || self.tok != Token::RBRACK);
                if is_array {
                    let close_pos: Pos;
                    let list = self.parse_parameter_list(pname, ptype, Token::RBRACK, false);
                    close_pos = self.expect(Token::RBRACK);
                    type_params = Some(FieldList {
                        opening: lbrack,
                        list,
                        closing: close_pos,
                    });
                    if self.tok == Token::ASSIGN {
                        assign = self.pos;
                        self.next();
                    }
                    typ = self.parse_type();
                } else {
                    typ = Expr::ArrayType(self.parse_array_type(lbrack, Some(x)));
                }
            } else {
                typ = Expr::ArrayType(self.parse_array_type(lbrack, None));
            }
        } else {
            if self.tok == Token::ASSIGN {
                assign = self.pos;
                self.next();
            }
            typ = self.parse_type();
        }
        let comment = self.expect_semi();
        Spec::TypeSpec(TypeSpec {
            doc,
            name,
            type_params,
            assign,
            ty: typ,
            comment,
            id: 0,
        })
    }

    fn parse_gen_decl(&mut self, keyword: Token) -> GenDecl {
        let doc = self.lead_comment.clone();
        let pos = self.expect(keyword);
        let mut lparen = NO_POS;
        let mut rparen = NO_POS;
        let mut list: Vec<Spec> = Vec::new();
        if self.tok == Token::LPAREN {
            lparen = self.pos;
            self.next();
            while self.tok != Token::RPAREN && self.tok != Token::EOF {
                let d = self.lead_comment.clone();
                let s = self.parse_spec(keyword, d);
                list.push(s);
            }
            rparen = self.expect(Token::RPAREN);
            self.expect_semi();
        } else {
            let s = self.parse_spec(keyword, None);
            list.push(s);
        }
        GenDecl {
            doc,
            tok_pos: pos,
            tok: Some(keyword),
            lparen,
            specs: list,
            rparen,
        }
    }

    fn parse_spec(&mut self, keyword: Token, doc: Option<CommentGroup>) -> Spec {
        match keyword {
            Token::IMPORT => self.parse_import_spec(doc),
            Token::CONST | Token::VAR => self.parse_value_spec(doc, keyword),
            Token::TYPE => self.parse_type_spec(doc),
            _ => unreachable!(),
        }
    }

    fn parse_func_decl(&mut self) -> FuncDecl {
        let doc = self.lead_comment.clone();
        let pos = self.expect(Token::FUNC);
        let recv = if self.tok == Token::LPAREN {
            self.parse_parameters(false)
        } else {
            None
        };
        let ident = self.parse_ident();
        let mut tparams: Option<FieldList> = None;
        if self.tok == Token::LBRACK {
            tparams = self.parse_type_parameters();
            if recv.is_some() && tparams.is_some() {
                self.error(
                    tparams.as_ref().unwrap().opening,
                    "method must have no type parameters",
                );
                tparams = None;
            }
        }
        let params = self.parse_parameters(false);
        let results = self.parse_parameters(true);
        let body = match self.tok {
            Token::LBRACE => {
                let b = self.parse_body();
                self.expect_semi();
                Some(b)
            }
            Token::SEMICOLON => {
                self.next();
                if self.tok == Token::LBRACE {
                    self.error(self.pos, "unexpected semicolon or newline before {");
                    let b = self.parse_body();
                    self.expect_semi();
                    Some(b)
                } else {
                    None
                }
            }
            _ => {
                self.expect_semi();
                None
            }
        };
        FuncDecl {
            doc,
            recv,
            name: ident,
            ty: FuncType {
                id: 0,
                func: pos,
                type_params: tparams,
                params,
                results,
            },
            body,
        }
    }

    fn parse_decl(&mut self, sync: fn(Token) -> bool) -> Decl {
        match self.tok {
            Token::IMPORT => Decl::GenDecl(self.parse_gen_decl(Token::IMPORT)),
            Token::CONST | Token::VAR => Decl::GenDecl(self.parse_gen_decl(self.tok)),
            Token::TYPE => Decl::GenDecl(self.parse_gen_decl(Token::TYPE)),
            Token::FUNC => Decl::FuncDecl(self.parse_func_decl()),
            _ => {
                let pos = self.pos;
                self.error_expected(pos, "declaration");
                self.advance_to(sync);
                Decl::BadDecl(BadDecl {
                    from: pos,
                    to: self.pos,
                })
            }
        }
    }

    // ---- file -----------------------------------------------------

    fn parse_file_inner(&mut self) -> Option<File> {
        if !self.errors.borrow().is_empty() {
            return None;
        }
        let doc = self.lead_comment.clone();
        let pos = self.expect(Token::PACKAGE);
        let ident = self.parse_ident();
        if ident.name == "_" && self.mode.contains(DECLARATION_ERRORS) {
            self.error(self.pos, "invalid package name _");
        }
        self.expect_semi();
        if !self.errors.borrow().is_empty() {
            return None;
        }

        let mut decls: Vec<Decl> = Vec::new();
        if !self.mode.contains(PACKAGE_CLAUSE_ONLY) {
            while self.tok == Token::IMPORT {
                decls.push(Decl::GenDecl(self.parse_gen_decl(Token::IMPORT)));
            }
            if !self.mode.contains(IMPORTS_ONLY) {
                let mut prev = Token::IMPORT;
                while self.tok != Token::EOF {
                    if self.tok == Token::IMPORT && prev != Token::IMPORT {
                        self.error(self.pos, "imports must appear before other declarations");
                    }
                    prev = self.tok;
                    decls.push(self.parse_decl(is_decl_start));
                }
            }
        }

        let file = File {
            doc,
            package: pos,
            name: ident,
            decls,
            file_start: NO_POS,
            file_end: NO_POS,
            scope: None,
            imports: std::mem::take(&mut self.imports),
            unresolved: Vec::new(),
            comments: std::mem::take(&mut self.comments),
            go_version: std::mem::take(&mut self.go_version),
            id: 0,
        };

        Some(file)
    }
}

// ====================================================================
// Helper free functions
// ====================================================================

fn pack_index_expr(x: Expr, lbrack: Pos, exprs: Vec<Expr>, rbrack: Pos) -> Expr {
    match exprs.len() {
        0 => panic!("internal error: pack_index_expr with empty expr slice"),
        1 => Expr::IndexExpr(IndexExpr {
            id: 0,
            x: Box::new(x),
            lbrack,
            index: Box::new(exprs.into_iter().next().unwrap()),
            rbrack,
        }),
        _ => Expr::IndexListExpr(IndexListExpr {
            id: 0,
            x: Box::new(x),
            lbrack,
            indices: exprs,
            rbrack,
        }),
    }
}

fn extract_name(x: Expr, force: bool) -> (Option<Ident>, Option<Expr>) {
    match x {
        Expr::Ident(id) => (Some(id), None),
        Expr::BinaryExpr(b) => match b.op {
            Token::MUL => {
                if let Expr::Ident(name) = &*b.x {
                    if force || is_type_elem(&b.y) {
                        let name = name.clone();
                        let star = Expr::StarExpr(StarExpr {
                            id: 0,
                            star: b.op_pos,
                            x: b.y.clone(),
                        });
                        return (Some(name), Some(star));
                    }
                }
                (None, Some(Expr::BinaryExpr(b)))
            }
            Token::OR => {
                let (name, lhs) = extract_name(*b.x.clone(), force || is_type_elem(&b.y));
                if let (Some(n), Some(l)) = (name, lhs) {
                    let new = Expr::BinaryExpr(BinaryExpr {
                        id: 0,
                        x: Box::new(l),
                        op_pos: b.op_pos,
                        op: Token::OR,
                        y: b.y,
                    });
                    return (Some(n), Some(new));
                }
                (None, Some(Expr::BinaryExpr(b)))
            }
            _ => (None, Some(Expr::BinaryExpr(b))),
        },
        Expr::CallExpr(c) => {
            if let Expr::Ident(name) = &*c.fun {
                if c.args.len() == 1 && c.ellipsis == NO_POS {
                    if force || is_type_elem(&c.args[0]) {
                        let name = name.clone();
                        let arg = c.args.into_iter().next().unwrap();
                        return (
                            Some(name),
                            Some(Expr::ParenExpr(ParenExpr {
                                id: 0,
                                lparen: c.lparen,
                                x: Box::new(arg),
                                rparen: c.rparen,
                            })),
                        );
                    }
                }
            }
            (None, Some(Expr::CallExpr(c)))
        }
        other => (None, Some(other)),
    }
}

fn is_type_elem(x: &Expr) -> bool {
    match x {
        Expr::ArrayType(_)
        | Expr::StructType(_)
        | Expr::FuncType(_)
        | Expr::InterfaceType(_)
        | Expr::MapType(_)
        | Expr::ChanType(_) => true,
        Expr::BinaryExpr(b) => is_type_elem(&b.x) || is_type_elem(&b.y),
        Expr::UnaryExpr(u) => u.op == Token::TILDE,
        Expr::ParenExpr(p) => is_type_elem(&p.x),
        _ => false,
    }
}

fn is_type_switch_assert(x: &Expr) -> bool {
    matches!(x, Expr::TypeAssertExpr(a) if a.ty.is_none())
}

fn is_type_switch_guard(s: &Option<Stmt>) -> bool {
    match s {
        Some(Stmt::ExprStmt(e)) => is_type_switch_assert(&e.x),
        Some(Stmt::AssignStmt(a))
            if a.lhs.len() == 1 && a.rhs.len() == 1 && is_type_switch_assert(&a.rhs[0]) =>
        {
            matches!(a.tok, Some(Token::DEFINE) | Some(Token::ASSIGN))
        }
        _ => false,
    }
}

fn unparen(mut e: Expr) -> Expr {
    loop {
        match e {
            Expr::ParenExpr(p) => e = *p.x,
            other => return other,
        }
    }
}

fn unparen_ref(e: &Expr) -> &Expr {
    let mut cur = e;
    while let Expr::ParenExpr(p) = cur {
        cur = &p.x;
    }
    cur
}

/// Shallow equality used to group consecutive params with the same
/// type. Match Go's pointer comparison: identical references.
/// Whether two parameter types should share a single [`Field`] during
/// parameter-list grouping.
///
/// Go compares type pointers here: after type distribution, earlier params
/// receive the *same* `ast.Expr` pointer as the typed parameter that
/// follows them. Our fill step clones, so pointer identity fails; compare
/// structurally instead (idents by name+pos; other nodes recursively),
/// ignoring [`Ident::id`] / resolver state.
fn expr_eq_shallow(a: &Expr, b: &Expr) -> bool {
    match (a, b) {
        (Expr::Ident(x), Expr::Ident(y)) => x.name == y.name && x.name_pos == y.name_pos,
        (Expr::BasicLit(x), Expr::BasicLit(y)) => {
            x.kind == y.kind && x.value == y.value && x.value_pos == y.value_pos
        }
        (Expr::StarExpr(x), Expr::StarExpr(y)) => expr_eq_shallow(&x.x, &y.x),
        (Expr::ParenExpr(x), Expr::ParenExpr(y)) => expr_eq_shallow(&x.x, &y.x),
        (Expr::UnaryExpr(x), Expr::UnaryExpr(y)) => {
            x.op == y.op && expr_eq_shallow(&x.x, &y.x)
        }
        (Expr::BinaryExpr(x), Expr::BinaryExpr(y)) => {
            x.op == y.op && expr_eq_shallow(&x.x, &y.x) && expr_eq_shallow(&x.y, &y.y)
        }
        (Expr::SelectorExpr(x), Expr::SelectorExpr(y)) => {
            x.sel.name == y.sel.name
                && x.sel.name_pos == y.sel.name_pos
                && expr_eq_shallow(&x.x, &y.x)
        }
        (Expr::IndexExpr(x), Expr::IndexExpr(y)) => {
            expr_eq_shallow(&x.x, &y.x) && expr_eq_shallow(&x.index, &y.index)
        }
        (Expr::IndexListExpr(x), Expr::IndexListExpr(y)) => {
            expr_eq_shallow(&x.x, &y.x)
                && x.indices.len() == y.indices.len()
                && x.indices
                    .iter()
                    .zip(y.indices.iter())
                    .all(|(a, b)| expr_eq_shallow(a, b))
        }
        (Expr::ArrayType(x), Expr::ArrayType(y)) => {
            match (&x.len, &y.len) {
                (None, None) => {}
                (Some(a), Some(b)) if expr_eq_shallow(a, b) => {}
                _ => return false,
            }
            expr_eq_shallow(&x.elt, &y.elt)
        }
        (Expr::Ellipsis(x), Expr::Ellipsis(y)) => match (&x.elt, &y.elt) {
            (None, None) => true,
            (Some(a), Some(b)) => expr_eq_shallow(a, b),
            _ => false,
        },
        (Expr::MapType(x), Expr::MapType(y)) => {
            expr_eq_shallow(&x.key, &y.key) && expr_eq_shallow(&x.value, &y.value)
        }
        (Expr::ChanType(x), Expr::ChanType(y)) => {
            x.dir == y.dir && expr_eq_shallow(&x.value, &y.value)
        }
        (Expr::FuncType(x), Expr::FuncType(y)) => {
            field_list_eq_shallow(x.type_params.as_ref(), y.type_params.as_ref())
                && field_list_eq_shallow(x.params.as_ref(), y.params.as_ref())
                && field_list_eq_shallow(x.results.as_ref(), y.results.as_ref())
        }
        (Expr::StructType(x), Expr::StructType(y)) => {
            x.incomplete == y.incomplete && field_list_eq_owned(&x.fields, &y.fields)
        }
        (Expr::InterfaceType(x), Expr::InterfaceType(y)) => {
            x.incomplete == y.incomplete && field_list_eq_owned(&x.methods, &y.methods)
        }
        // Fall back to pointer equality for remaining rare shapes that were
        // not cloned during type distribution.
        _ => std::ptr::eq(a as *const _, b as *const _),
    }
}

fn field_list_eq_shallow(a: Option<&FieldList>, b: Option<&FieldList>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => field_list_eq_owned(a, b),
        _ => false,
    }
}

fn field_list_eq_owned(a: &FieldList, b: &FieldList) -> bool {
    a.list.len() == b.list.len()
        && a.list.iter().zip(b.list.iter()).all(|(fa, fb)| {
            fa.names.len() == fb.names.len()
                && fa
                    .names
                    .iter()
                    .zip(fb.names.iter())
                    .all(|(na, nb)| na.name == nb.name && na.name_pos == nb.name_pos)
                && match (&fa.ty, &fb.ty) {
                    (None, None) => true,
                    (Some(ta), Some(tb)) => expr_eq_shallow(ta, tb),
                    _ => false,
                }
        })
}

#[derive(Default)]
struct ParamField {
    name: Option<Ident>,
    ty: Option<Expr>,
}

// ====================================================================
// Public API
// ====================================================================

/// Parse a Go source file, mirroring `parser.ParseFile`.
pub fn parse_file(
    fset: &Arc<FileSet>,
    filename: &str,
    src: &[u8],
    mode: Mode,
) -> Result<File, ErrorList> {
    // `base = -1` lets the FileSet allocate the base atomically under its write
    // lock. Passing `fset.base()` here is a read-then-write race when multiple
    // files are parsed into a shared FileSet concurrently (guff-packages
    // type-checks packages in parallel).
    let pos_file = fset.add_file(filename, -1, src.len() as i64);
    let file_start = pos_file.pos(0);
    let file_end = pos_file.end();
    let p = Rc::new(RefCell::new(Parser::new(Arc::clone(&pos_file), src, mode)));
    let errors_handle = Rc::clone(&p.borrow().errors);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut p = p.borrow_mut();
        p.parse_file_inner()
    }));
    let file_opt = match result {
        Ok(f) => f,
        Err(payload) => {
            if payload.downcast_ref::<Bailout>().is_some() {
                None
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    };
    let mut errs = errors_handle.borrow().clone();
    errs.sort();
    if let Some(mut file) = file_opt {
        file.file_start = file_start;
        file.file_end = file_end;
        if !mode.contains(SKIP_OBJECT_RESOLUTION) {
            resolve_file(&mut file, &pos_file, None);
        }
        // Assign stable node ids to every unstamped expression so the type
        // checker's `Info` maps (Defs/Uses/Types) can key on them.
        crate::stamp::stamp_node_ids(&mut file);
        if errs.is_empty() {
            Ok(file)
        } else {
            Err(errs)
        }
    } else {
        Err(errs)
    }
}

/// Parse a Go expression (no enclosing source file). Mirrors
/// `parser.ParseExprFrom`.
pub fn parse_expr_from(
    fset: &Arc<FileSet>,
    filename: &str,
    src: &[u8],
    mode: Mode,
) -> Result<Expr, ErrorList> {
    // `base = -1` lets the FileSet allocate the base atomically under its write
    // lock. Passing `fset.base()` here is a read-then-write race when multiple
    // files are parsed into a shared FileSet concurrently (guff-packages
    // type-checks packages in parallel).
    let pos_file = fset.add_file(filename, -1, src.len() as i64);
    let p = Rc::new(RefCell::new(Parser::new(pos_file, src, mode)));
    let errors_handle = Rc::clone(&p.borrow().errors);

    let result = catch_unwind(AssertUnwindSafe(|| {
        let mut p = p.borrow_mut();
        let x = p.parse_rhs();
        // Allow trailing semicolon.
        if p.tok == Token::SEMICOLON && p.lit == "\n" {
            p.next();
        }
        if p.tok != Token::EOF {
            let pos = p.pos;
            p.error_expected(pos, "EOF");
        }
        x
    }));
    let mut errs = errors_handle.borrow().clone();
    errs.sort();
    match result {
        Ok(mut expr) => {
            crate::stamp::stamp_expr_ids(&mut expr);
            if errs.is_empty() {
                Ok(expr)
            } else {
                Err(errs)
            }
        }
        Err(payload) => {
            if payload.downcast_ref::<Bailout>().is_some() {
                Err(errs)
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    }
}

// Silence unused imports that are only referenced in tests.
#[cfg(test)]
use token as _token_used;

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Result<File, ErrorList> {
        let fset = FileSet::new();
        parse_file(&fset, "t.go", src.as_bytes(), Mode::NONE)
    }

    #[test]
    fn parses_empty_package() {
        let f = parse("package p\n").unwrap();
        assert_eq!(f.name.name, "p");
        assert!(f.decls.is_empty());
    }

    #[test]
    fn parses_imports() {
        let src = "package p\nimport (\n\t\"fmt\"\n\t\"strings\"\n)\n";
        let f = parse(src).unwrap();
        assert_eq!(f.imports.len(), 2);
        assert_eq!(f.imports[0].path.value, "\"fmt\"");
        assert_eq!(f.imports[1].path.value, "\"strings\"");
    }

    #[test]
    fn parses_const() {
        let src = "package p\nconst Pi = 3.14\n";
        let f = parse(src).unwrap();
        if let Decl::GenDecl(g) = &f.decls[0] {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                assert_eq!(v.names[0].name, "Pi");
            }
        }
    }

    #[test]
    fn parses_var_decl() {
        let src = "package p\nvar x int = 1\n";
        let f = parse(src).unwrap();
        if let Decl::GenDecl(g) = &f.decls[0] {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                assert_eq!(v.names[0].name, "x");
                assert!(v.ty.is_some());
                assert_eq!(v.values.len(), 1);
            }
        }
    }

    #[test]
    fn parses_type_decl_struct() {
        let src = "package p\ntype T struct { X int }\n";
        let f = parse(src).unwrap();
        if let Decl::GenDecl(g) = &f.decls[0] {
            if let Spec::TypeSpec(ts) = &g.specs[0] {
                assert_eq!(ts.name.name, "T");
                assert!(matches!(ts.ty, Expr::StructType(_)));
            }
        }
    }

    #[test]
    fn parses_func_decl() {
        let src = "package p\nfunc f(x int) int { return x + 1 }\n";
        let f = parse(src).unwrap();
        if let Decl::FuncDecl(fd) = &f.decls[0] {
            assert_eq!(fd.name.name, "f");
            assert!(fd.body.is_some());
        }
    }

    #[test]
    fn parses_method() {
        let src = "package p\ntype T int\nfunc (t T) M() {}\n";
        let f = parse(src).unwrap();
        let method = match &f.decls[1] {
            Decl::FuncDecl(fd) => fd,
            _ => panic!("expected FuncDecl"),
        };
        assert!(method.recv.is_some());
        assert_eq!(method.name.name, "M");
    }

    #[test]
    fn parses_if_else() {
        let src = "package p\nfunc f(x int) {\n\tif x > 0 { return } else { return }\n}\n";
        parse(src).unwrap();
    }

    #[test]
    fn parses_for_range() {
        let src =
            "package p\nfunc f(xs []int) {\n\tfor i, v := range xs {\n\t\t_ = i; _ = v\n\t}\n}\n";
        parse(src).unwrap();
    }

    #[test]
    fn parses_switch() {
        let src = "package p\nfunc f(x int) {\n\tswitch x {\n\tcase 1: return\n\tdefault: return\n\t}\n}\n";
        parse(src).unwrap();
    }

    #[test]
    fn rejects_invalid_syntax() {
        let r = parse("package\n");
        assert!(r.is_err());
    }

    #[test]
    fn parse_expr_from_simple() {
        let fset = FileSet::new();
        let e = parse_expr_from(&fset, "e", b"x + 1", Mode::NONE).unwrap();
        assert!(matches!(e, Expr::BinaryExpr(_)));
    }

    #[test]
    fn parses_realistic_sample() {
        // The sample from goastpractice/main.go (subset).
        let src = r#"package shop

import (
	"fmt"
	"strings"
)

const TaxRate = 0.1

var defaultCurrency = "JPY"

type Item struct {
	ID    int
	Name  string
	Price float64
}

type Discounter interface {
	Discount(rate float64) float64
}

func Total(items []Item) float64 {
	var sum float64
	for _, it := range items {
		sum += it.Price
	}
	return sum * (1 + TaxRate)
}

func (i Item) Discount(rate float64) float64 {
	return i.Price * (1 - rate)
}

func describe(i Item) string {
	return fmt.Sprintf("%s (%s)", i.Name, strings.ToUpper(defaultCurrency))
}
"#;
        let fset = FileSet::new();
        let f = parse_file(&fset, "shop.go", src.as_bytes(), Mode::NONE).unwrap();
        assert_eq!(f.name.name, "shop");
        assert_eq!(f.imports.len(), 2);
        // 1 import-decl + 1 const + 1 var + 2 type-decl + 3 funcs
        assert_eq!(f.decls.len(), 8);
    }

    #[test]
    fn parses_go_and_defer_calls() {
        // Regression: `parseCallExpr` used to compare a cloned `unparen(x)`
        // against `x` by pointer, which never matched, so every `go`/`defer`
        // was flagged "must not be parenthesized". A plain call must parse
        // cleanly now.
        let src = "package p\nfunc f() {}\nfunc g() {\n\tgo f()\n\tdefer f()\n}\n";
        let f = parse(src).unwrap();
        let g = match &f.decls[1] {
            Decl::FuncDecl(fd) => fd.body.as_ref().unwrap(),
            _ => panic!("expected FuncDecl"),
        };
        assert!(matches!(g.list[0], Stmt::GoStmt(_)), "first stmt is `go`");
        assert!(
            matches!(g.list[1], Stmt::DeferStmt(_)),
            "second stmt is `defer`"
        );
    }

    #[test]
    fn rejects_parenthesized_defer_call() {
        // The parenthesization diagnostic must still fire for `defer (f())`.
        let r = parse("package p\nfunc f() {}\nfunc g() {\n\tdefer (f())\n}\n");
        let errs = r.expect_err("parenthesized defer must be rejected");
        assert!(
            format!("{errs:?}").contains("must not be parenthesized"),
            "expected parenthesization error, got: {errs:?}"
        );
    }

    #[test]
    fn rejects_defer_non_call() {
        // `defer x` (not a call) must report "must be function call".
        let r = parse("package p\nfunc g(x int) {\n\tdefer x\n}\n");
        let errs = r.expect_err("defer of non-call must be rejected");
        assert!(
            format!("{errs:?}").contains("must be function call"),
            "expected function-call error, got: {errs:?}"
        );
    }

    #[test]
    fn resolution_runs_on_default_mode() {
        // Without SKIP_OBJECT_RESOLUTION, ident.obj should be populated
        // for declarations referenced by other declarations.
        let src = "package p\nconst A = 1\nconst B = A\n";
        let f = parse(src).unwrap();
        // B's value `A` should resolve to const A.
        if let Decl::GenDecl(g) = &f.decls[1] {
            if let Spec::ValueSpec(v) = &g.specs[0] {
                if let Expr::Ident(used) = &v.values[0] {
                    assert!(used.obj.lock().unwrap().is_some(), "A should resolve");
                }
            }
        }
    }
}
