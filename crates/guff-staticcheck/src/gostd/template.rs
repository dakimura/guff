//! Port of Go's `text/template/parse`, as reached by `template.New("").Parse`.
//!
//! [`SA1001`](../../sa1001/index.html) is not really a check: upstream hands the
//! constant to the standard library and prints `err.Error()` verbatim, keeping
//! only the messages that contain `unexpected` or `bad character`. Reproducing
//! that means reproducing the parser — both halves of it. Stopping at a
//! *different* error than Go does is just as wrong as wording the same error
//! differently: a template Go rejects for an unquotable string but guff walks
//! past will hand back an `unexpected` message from further along and be
//! reported where upstream says nothing.
//!
//! Only the error path is ported. Nodes carry the little that error text needs
//! (a term's rendering, whether a tree is empty) and nothing else — no
//! execution, no positions beyond what the messages use.
//!
//! The ground truth is `compat/oracles/gotemplate`, which runs the real
//! `text/template` (and `html/template`, whose `Parse` returns the same errors)
//! over a corpus; `tests/gostd_template.rs` replays it here.

use std::collections::HashMap;

use super::fmt as gofmt;
use super::strconv;
use super::unicode;

/// The builtin function names `template.New("")` parses against — Go's
/// `builtins()`. `hasFunction` decides `function %q not defined`, and the
/// absence of `break`/`continue` from this map is what enables those keywords.
const BUILTINS: &[&str] = &[
    "and", "call", "html", "index", "slice", "js", "len", "not", "or", "print", "printf",
    "println", "urlquery", "eq", "ge", "gt", "le", "lt", "ne",
];

const LEFT_DELIM: &str = "{{";
const RIGHT_DELIM: &str = "}}";
const LEFT_COMMENT: &str = "/*";
const RIGHT_COMMENT: &str = "*/";
const SPACE_CHARS: &str = " \t\r\n";
const TRIM_MARKER: u8 = b'-';
const TRIM_MARKER_LEN: usize = 2; // marker plus the space beside it

// ---------------------------------------------------------------------------
// Items
// ---------------------------------------------------------------------------

/// `itemType`. The order matters: everything above [`ItemType::Keyword`] is a
/// keyword, which `item.String` renders as `<word>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum ItemType {
    Error,
    Bool,
    Char,
    CharConstant,
    Comment,
    Complex,
    Assign,
    Declare,
    Eof,
    Field,
    Identifier,
    LeftDelim,
    LeftParen,
    Number,
    Pipe,
    RawString,
    RightDelim,
    RightParen,
    Space,
    String,
    Text,
    Variable,
    Keyword,
    Block,
    Break,
    Continue,
    Dot,
    Define,
    Else,
    End,
    If,
    Nil,
    Range,
    Template,
    With,
}

fn keyword(word: &str) -> Option<ItemType> {
    Some(match word {
        "." => ItemType::Dot,
        "block" => ItemType::Block,
        "break" => ItemType::Break,
        "continue" => ItemType::Continue,
        "define" => ItemType::Define,
        "else" => ItemType::Else,
        "end" => ItemType::End,
        "if" => ItemType::If,
        "range" => ItemType::Range,
        "nil" => ItemType::Nil,
        "template" => ItemType::Template,
        "with" => ItemType::With,
        _ => return None,
    })
}

#[derive(Clone, Debug)]
struct Item {
    typ: ItemType,
    val: String,
    line: usize,
}

impl Item {
    fn eof(line: usize) -> Item {
        Item {
            typ: ItemType::Eof,
            val: "EOF".to_string(),
            line,
        }
    }

    /// Mirrors `item.String`.
    ///
    /// The length test is in **bytes** while `%.10q` truncates in **runes**, so
    /// a value of eleven bytes but four runes is quoted whole and still gains
    /// the ellipsis. That asymmetry is Go's, and it shows up in messages.
    fn render(&self) -> String {
        match self.typ {
            ItemType::Eof => "EOF".to_string(),
            ItemType::Error => self.val.clone(),
            t if t > ItemType::Keyword => format!("<{}>", self.val),
            _ if self.val.len() > 10 => {
                let head: String = self.val.chars().take(10).collect();
                format!("{}...", strconv::quote(&head))
            }
            _ => strconv::quote(&self.val),
        }
    }
}

// ---------------------------------------------------------------------------
// Lexer
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Text,
    LeftDelim,
    Comment,
    RightDelim,
    InsideAction,
    Space,
    Identifier,
    Field,
    Variable,
    Char,
    Number,
    Quote,
    RawQuote,
}

struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    start: usize,
    at_eof: bool,
    paren_depth: i32,
    line: usize,
    start_line: usize,
    item: Item,
    inside_action: bool,
}

fn is_space(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\r' || c == '\n'
}

fn is_alpha_numeric(c: char) -> bool {
    c == '_' || unicode::is_letter(c) || unicode::is_digit(c)
}

fn has_left_trim_marker(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0] == TRIM_MARKER && is_space(char::from(b[1]))
}

fn has_right_trim_marker(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && is_space(char::from(b[0])) && b[1] == TRIM_MARKER
}

fn right_trim_length(s: &str) -> usize {
    s.len() - s.trim_end_matches(|c| SPACE_CHARS.contains(c)).len()
}

fn left_trim_length(s: &str) -> usize {
    s.len() - s.trim_start_matches(|c| SPACE_CHARS.contains(c)).len()
}

fn count_newlines(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}

/// `%#U`: `U+0041 'A'`, with the quoted rune only when it is printable.
fn format_rune(c: char) -> String {
    let n = u32::from(c);
    if strconv::is_print(c) {
        format!("U+{n:04X} '{c}'")
    } else {
        format!("U+{n:04X}")
    }
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Lexer<'a> {
        Lexer {
            input,
            pos: 0,
            start: 0,
            at_eof: false,
            paren_depth: 0,
            line: 1,
            start_line: 1,
            item: Item::eof(1),
            inside_action: false,
        }
    }

    fn next(&mut self) -> Option<char> {
        if self.pos >= self.input.len() {
            self.at_eof = true;
            return None;
        }
        let c = self.input[self.pos..].chars().next()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
        }
        Some(c)
    }

    fn backup(&mut self) {
        if !self.at_eof && self.pos > 0 {
            if let Some(c) = self.input[..self.pos].chars().next_back() {
                self.pos -= c.len_utf8();
                if c == '\n' {
                    self.line -= 1;
                }
            }
        }
    }

    fn peek(&mut self) -> Option<char> {
        let c = self.next();
        self.backup();
        c
    }

    fn accept(&mut self, valid: &str) -> bool {
        match self.next() {
            Some(c) if valid.contains(c) => true,
            _ => {
                self.backup();
                false
            }
        }
    }

    fn accept_run(&mut self, valid: &str) {
        while matches!(self.next(), Some(c) if valid.contains(c)) {}
        self.backup();
    }

    fn this_item(&mut self, t: ItemType) -> Item {
        let i = Item {
            typ: t,
            val: self.input[self.start..self.pos].to_string(),
            line: self.start_line,
        };
        self.start = self.pos;
        self.start_line = self.line;
        i
    }

    fn emit(&mut self, t: ItemType) -> Option<State> {
        let i = self.this_item(t);
        self.emit_item(i)
    }

    fn emit_item(&mut self, i: Item) -> Option<State> {
        self.item = i;
        None
    }

    fn ignore(&mut self) {
        self.line += count_newlines(&self.input[self.start..self.pos]);
        self.start = self.pos;
        self.start_line = self.line;
    }

    /// Mirrors `(*lexer).errorf`, including the input truncation: after an
    /// error the lexer is spent, and every later `nextItem` returns EOF.
    fn errorf(&mut self, msg: String) -> Option<State> {
        self.item = Item {
            typ: ItemType::Error,
            val: msg,
            line: self.start_line,
        };
        self.start = 0;
        self.pos = 0;
        self.input = "";
        None
    }

    fn at_right_delim(&self) -> (bool, bool) {
        let rest = &self.input[self.pos..];
        if has_right_trim_marker(rest) && rest[TRIM_MARKER_LEN..].starts_with(RIGHT_DELIM) {
            return (true, true);
        }
        if rest.starts_with(RIGHT_DELIM) {
            return (true, false);
        }
        (false, false)
    }

    fn at_terminator(&mut self) -> bool {
        match self.peek() {
            Some(c) if is_space(c) => true,
            Some('.') | Some(',') | Some('|') | Some(':') | Some(')') | Some('(') => true,
            None => true,
            _ => self.input[self.pos..].starts_with(RIGHT_DELIM),
        }
    }

    fn next_item(&mut self) -> Item {
        self.item = Item {
            typ: ItemType::Eof,
            val: "EOF".to_string(),
            line: self.start_line,
        };
        let mut state = if self.inside_action {
            State::InsideAction
        } else {
            State::Text
        };
        loop {
            match self.step(state) {
                Some(next) => state = next,
                None => return self.item.clone(),
            }
        }
    }

    fn step(&mut self, state: State) -> Option<State> {
        match state {
            State::Text => self.lex_text(),
            State::LeftDelim => self.lex_left_delim(),
            State::Comment => self.lex_comment(),
            State::RightDelim => self.lex_right_delim(),
            State::InsideAction => self.lex_inside_action(),
            State::Space => self.lex_space(),
            State::Identifier => self.lex_identifier(),
            State::Field => self.lex_field_or_variable(ItemType::Field),
            State::Variable => self.lex_variable(),
            State::Char => self.lex_char(),
            State::Number => self.lex_number(),
            State::Quote => self.lex_quote(),
            State::RawQuote => self.lex_raw_quote(),
        }
    }

    fn lex_text(&mut self) -> Option<State> {
        if let Some(x) = self.input[self.pos..].find(LEFT_DELIM) {
            if x > 0 {
                self.pos += x;
                let mut trim_length = 0;
                let delim_end = self.pos + LEFT_DELIM.len();
                if has_left_trim_marker(&self.input[delim_end..]) {
                    trim_length = right_trim_length(&self.input[self.start..self.pos]);
                }
                self.pos -= trim_length;
                self.line += count_newlines(&self.input[self.start..self.pos]);
                let i = self.this_item(ItemType::Text);
                self.pos += trim_length;
                self.ignore();
                if !i.val.is_empty() {
                    return self.emit_item(i);
                }
            }
            return Some(State::LeftDelim);
        }
        self.pos = self.input.len();
        if self.pos > self.start {
            self.line += count_newlines(&self.input[self.start..self.pos]);
            return self.emit(ItemType::Text);
        }
        self.emit(ItemType::Eof)
    }

    fn lex_left_delim(&mut self) -> Option<State> {
        self.pos += LEFT_DELIM.len();
        let trim_space = has_left_trim_marker(&self.input[self.pos..]);
        let after_marker = if trim_space { TRIM_MARKER_LEN } else { 0 };
        if self.input[self.pos + after_marker..].starts_with(LEFT_COMMENT) {
            self.pos += after_marker;
            self.ignore();
            return Some(State::Comment);
        }
        let i = self.this_item(ItemType::LeftDelim);
        self.inside_action = true;
        self.pos += after_marker;
        self.ignore();
        self.paren_depth = 0;
        self.emit_item(i)
    }

    fn lex_comment(&mut self) -> Option<State> {
        self.pos += LEFT_COMMENT.len();
        let Some(x) = self.input[self.pos..].find(RIGHT_COMMENT) else {
            return self.errorf("unclosed comment".to_string());
        };
        self.pos += x + RIGHT_COMMENT.len();
        let (delim, trim_space) = self.at_right_delim();
        if !delim {
            return self.errorf("comment ends before closing delimiter".to_string());
        }
        self.line += count_newlines(&self.input[self.start..self.pos]);
        let _ = self.this_item(ItemType::Comment);
        if trim_space {
            self.pos += TRIM_MARKER_LEN;
        }
        self.pos += RIGHT_DELIM.len();
        if trim_space {
            self.pos += left_trim_length(&self.input[self.pos..]);
        }
        self.ignore();
        // emitComment is off: template.Parse does not set ParseComments.
        Some(State::Text)
    }

    fn lex_right_delim(&mut self) -> Option<State> {
        let (_, trim_space) = self.at_right_delim();
        if trim_space {
            self.pos += TRIM_MARKER_LEN;
            self.ignore();
        }
        self.pos += RIGHT_DELIM.len();
        let i = self.this_item(ItemType::RightDelim);
        if trim_space {
            self.pos += left_trim_length(&self.input[self.pos..]);
            self.ignore();
        }
        self.inside_action = false;
        self.emit_item(i)
    }

    fn lex_inside_action(&mut self) -> Option<State> {
        let (delim, _) = self.at_right_delim();
        if delim {
            if self.paren_depth == 0 {
                return Some(State::RightDelim);
            }
            return self.errorf("unclosed left paren".to_string());
        }
        let Some(r) = self.next() else {
            return self.errorf("unclosed action".to_string());
        };
        if is_space(r) {
            self.backup(); // put the space back in case this is " -}}"
            return Some(State::Space);
        }
        match r {
            '=' => return self.emit(ItemType::Assign),
            ':' => {
                if self.next() != Some('=') {
                    return self.errorf("expected :=".to_string());
                }
                return self.emit(ItemType::Declare);
            }
            '|' => return self.emit(ItemType::Pipe),
            '"' => return Some(State::Quote),
            '`' => return Some(State::RawQuote),
            '$' => return Some(State::Variable),
            '\'' => return Some(State::Char),
            '.' => {
                // Look ahead for ".field" without disturbing backup().
                if self.pos < self.input.len() {
                    let b = self.input.as_bytes()[self.pos];
                    if !b.is_ascii_digit() {
                        return Some(State::Field);
                    }
                }
                // Otherwise fall through: '.' can start a number.
                self.backup();
                return Some(State::Number);
            }
            '(' => {
                self.paren_depth += 1;
                return self.emit(ItemType::LeftParen);
            }
            ')' => {
                self.paren_depth -= 1;
                if self.paren_depth < 0 {
                    return self.errorf("unexpected right paren".to_string());
                }
                return self.emit(ItemType::RightParen);
            }
            _ => {}
        }
        if r == '+' || r == '-' || r.is_ascii_digit() {
            self.backup();
            return Some(State::Number);
        }
        if is_alpha_numeric(r) {
            self.backup();
            return Some(State::Identifier);
        }
        if u32::from(r) <= 0x7f && strconv::is_print(r) {
            return self.emit(ItemType::Char);
        }
        self.errorf(format!(
            "unrecognized character in action: {}",
            format_rune(r)
        ))
    }

    fn lex_space(&mut self) -> Option<State> {
        let mut num_spaces = 0;
        loop {
            match self.peek() {
                Some(c) if is_space(c) => {}
                _ => break,
            }
            self.next();
            num_spaces += 1;
        }
        // A trim-marked closing delimiter has a minus after a space, and we
        // know there is a space.
        if has_right_trim_marker(&self.input[self.pos - 1..])
            && self.input[self.pos - 1 + TRIM_MARKER_LEN..].starts_with(RIGHT_DELIM)
        {
            self.backup(); // before the space
            if num_spaces == 1 {
                return Some(State::RightDelim);
            }
        }
        self.emit(ItemType::Space)
    }

    fn lex_identifier(&mut self) -> Option<State> {
        loop {
            let r = self.next();
            if matches!(r, Some(c) if is_alpha_numeric(c)) {
                continue;
            }
            self.backup();
            let word = self.input[self.start..self.pos].to_string();
            if !self.at_terminator() {
                let Some(c) = r else {
                    // Go formats eof (-1) here; the input cannot reach this,
                    // since at_terminator is true at end of input.
                    return self.errorf("bad character".to_string());
                };
                return self.errorf(format!("bad character {}", format_rune(c)));
            }
            return match keyword(&word) {
                // break and continue are keywords only because no function of
                // those names is defined; with New("") none is.
                Some(k) => self.emit(k),
                None if word.starts_with('.') => self.emit(ItemType::Field),
                None if word == "true" || word == "false" => self.emit(ItemType::Bool),
                None => self.emit(ItemType::Identifier),
            };
        }
    }

    fn lex_variable(&mut self) -> Option<State> {
        if self.at_terminator() {
            // Nothing interesting follows: a bare "$".
            return self.emit(ItemType::Variable);
        }
        self.lex_field_or_variable(ItemType::Variable)
    }

    fn lex_field_or_variable(&mut self, typ: ItemType) -> Option<State> {
        if self.at_terminator() {
            // Nothing interesting follows: a bare "." or "$".
            return if typ == ItemType::Variable {
                self.emit(ItemType::Variable)
            } else {
                self.emit(ItemType::Dot)
            };
        }
        let mut r;
        loop {
            r = self.next();
            if !matches!(r, Some(c) if is_alpha_numeric(c)) {
                self.backup();
                break;
            }
        }
        if !self.at_terminator() {
            let Some(c) = r else {
                // Unreachable: at_terminator is true at end of input, which is
                // the only way `r` is None. Go formats rune(-1) with %#U here.
                return self.errorf("bad character".to_string());
            };
            return self.errorf(format!("bad character {}", format_rune(c)));
        }
        self.emit(typ)
    }

    fn lex_char(&mut self) -> Option<State> {
        loop {
            match self.next() {
                Some('\\') => match self.next() {
                    Some('\n') | None => {
                        return self.errorf("unterminated character constant".to_string())
                    }
                    _ => continue,
                },
                Some('\n') | None => {
                    return self.errorf("unterminated character constant".to_string())
                }
                Some('\'') => break,
                _ => {}
            }
        }
        self.emit(ItemType::CharConstant)
    }

    fn lex_number(&mut self) -> Option<State> {
        if !self.scan_number() {
            let val = self.input[self.start..self.pos].to_string();
            return self.errorf(format!("bad number syntax: {}", strconv::quote(&val)));
        }
        if matches!(self.peek(), Some('+') | Some('-')) {
            // Complex: 1+2i. No spaces, and it must end in 'i'.
            if !self.scan_number() || self.input.as_bytes()[self.pos - 1] != b'i' {
                let val = self.input[self.start..self.pos].to_string();
                return self.errorf(format!("bad number syntax: {}", strconv::quote(&val)));
            }
            return self.emit(ItemType::Complex);
        }
        self.emit(ItemType::Number)
    }

    fn scan_number(&mut self) -> bool {
        self.accept("+-");
        let mut digits = "0123456789_";
        if self.accept("0") {
            // A leading 0 does not mean octal in floats.
            if self.accept("xX") {
                digits = "0123456789abcdefABCDEF_";
            } else if self.accept("oO") {
                digits = "01234567_";
            } else if self.accept("bB") {
                digits = "01_";
            }
        }
        self.accept_run(digits);
        if self.accept(".") {
            self.accept_run(digits);
        }
        if digits.len() == 10 + 1 && self.accept("eE") {
            self.accept("+-");
            self.accept_run("0123456789_");
        }
        if digits.len() == 16 + 6 + 1 && self.accept("pP") {
            self.accept("+-");
            self.accept_run("0123456789_");
        }
        self.accept("i");
        // The next thing must not be alphanumeric.
        if matches!(self.peek(), Some(c) if is_alpha_numeric(c)) {
            self.next();
            return false;
        }
        true
    }

    fn lex_quote(&mut self) -> Option<State> {
        loop {
            match self.next() {
                Some('\\') => match self.next() {
                    Some('\n') | None => {
                        return self.errorf("unterminated quoted string".to_string())
                    }
                    _ => continue,
                },
                Some('\n') | None => return self.errorf("unterminated quoted string".to_string()),
                Some('"') => break,
                _ => {}
            }
        }
        self.emit(ItemType::String)
    }

    fn lex_raw_quote(&mut self) -> Option<State> {
        loop {
            match self.next() {
                None => return self.errorf("unterminated raw quoted string".to_string()),
                Some('`') => break,
                _ => {}
            }
        }
        self.emit(ItemType::RawString)
    }
}

// ---------------------------------------------------------------------------
// Nodes
// ---------------------------------------------------------------------------

/// Only what error text needs: a node's kind, the rendering of the terms that
/// can appear in `unexpected . after term %q`, and enough of a tree for
/// `IsEmptyTree` to answer.
#[derive(Clone, Debug)]
enum Node {
    List(Vec<Node>),
    Text(String),
    Comment,
    Action,
    If,
    Range,
    With,
    Template,
    Break,
    Continue,
    End,
    Else,
    Bool(bool),
    Str(String),
    Number(String),
    Nil,
    Dot,
    Field,
    Variable,
    Identifier,
    Chain,
    Pipe,
}

#[derive(Clone, Debug, Default)]
struct Pipe {
    decl: usize,
    cmds: Vec<Vec<Node>>,
}

impl Node {
    fn render(&self) -> String {
        match self {
            Node::End => "{{end}}".to_string(),
            Node::Else => "{{else}}".to_string(),
            Node::Bool(b) => b.to_string(),
            Node::Str(quoted) => quoted.clone(),
            Node::Number(text) => text.clone(),
            Node::Nil => "nil".to_string(),
            Node::Dot => ".".to_string(),
            // No other node reaches a message.
            _ => String::new(),
        }
    }

    fn is_end_or_else(&self) -> bool {
        matches!(self, Node::End | Node::Else)
    }
}

/// Mirrors `parse.IsEmptyTree`.
fn is_empty_tree(n: Option<&Node>) -> bool {
    match n {
        None => true,
        Some(Node::Comment) => true,
        Some(Node::List(nodes)) => nodes.iter().all(|n| is_empty_tree(Some(n))),
        Some(Node::Text(text)) => text.trim().is_empty(),
        Some(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

/// State the nested trees of `{{define}}` and `{{block}}` share: one lexer for
/// the whole input, and the set of trees defined so far.
struct Shared<'a> {
    lex: Lexer<'a>,
    tree_set: HashMap<String, Option<Node>>,
}

/// One `parse.Tree`. A `{{define}}` or `{{block}}` body gets its own, with its
/// own token buffer and variable scope, reading from the shared lexer.
struct Tree {
    name: String,
    parse_name: String,
    root: Option<Node>,
    token: [Item; 3],
    peek_count: usize,
    vars: Vec<String>,
    action_line: usize,
    range_depth: i32,
    stack_depth: usize,
    recursion: usize,
}

const MAX_STACK_DEPTH: usize = 10000;

/// How deep the parser will recurse before giving up.
///
/// Go has no such limit outside parenthesized pipelines ([`MAX_STACK_DEPTH`],
/// which is upstream's): a goroutine stack grows, so `{{if}}` nested a hundred
/// thousand deep parses fine. A Rust thread's stack does not grow, and this
/// parser is recursive-descent, so the same input aborts the process — the
/// worst failure mode there is, and one the corpus cannot catch because no
/// hand-written template is shaped like that.
///
/// A level costs about 1 KiB of stack in release and 4 KiB unoptimized
/// (measured: at 2 MiB, a debug build overflows just short of 500 levels), so
/// 250 fits in a quarter of a megabyte of guff's 8 MiB worker stack — and in a
/// bare 2 MiB thread even unoptimized, which is what the test pins. Real
/// templates nest single digits deep.
///
/// Past the limit guff declines to answer: the error below is the one string
/// this module produces that Go never would, and it carries neither
/// "unexpected" nor "bad character", so SA1001 stays silent rather than
/// reporting something upstream did not.
const MAX_RECURSION: usize = 250;

const TOO_DEEP: &str = "guff: template nesting exceeds guff's recursion limit";

type PResult<T> = Result<T, String>;

impl Tree {
    fn new(name: &str, parse_name: &str) -> Tree {
        Tree {
            name: name.to_string(),
            parse_name: parse_name.to_string(),
            root: None,
            token: [Item::eof(1), Item::eof(1), Item::eof(1)],
            peek_count: 0,
            vars: vec!["$".to_string()],
            action_line: 0,
            range_depth: 0,
            stack_depth: 0,
            recursion: 0,
        }
    }

    // -- token stream ------------------------------------------------------

    fn next(&mut self, sh: &mut Shared) -> Item {
        if self.peek_count > 0 {
            self.peek_count -= 1;
        } else {
            self.token[0] = sh.lex.next_item();
        }
        self.token[self.peek_count].clone()
    }

    fn backup(&mut self) {
        self.peek_count += 1;
    }

    fn backup2(&mut self, t1: Item) {
        self.token[1] = t1;
        self.peek_count = 2;
    }

    fn backup3(&mut self, t2: Item, t1: Item) {
        self.token[1] = t1;
        self.token[2] = t2;
        self.peek_count = 3;
    }

    fn peek(&mut self, sh: &mut Shared) -> Item {
        if self.peek_count > 0 {
            return self.token[self.peek_count - 1].clone();
        }
        self.peek_count = 1;
        self.token[0] = sh.lex.next_item();
        self.token[0].clone()
    }

    fn next_non_space(&mut self, sh: &mut Shared) -> Item {
        loop {
            let token = self.next(sh);
            if token.typ != ItemType::Space {
                return token;
            }
        }
    }

    fn peek_non_space(&mut self, sh: &mut Shared) -> Item {
        let token = self.next_non_space(sh);
        self.backup();
        token
    }

    /// Enter one level of the parser's recursion; see [`MAX_RECURSION`].
    fn enter(&mut self) -> PResult<()> {
        self.recursion += 1;
        if self.recursion > MAX_RECURSION {
            return Err(TOO_DEEP.to_string());
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.recursion -= 1;
    }

    // -- errors ------------------------------------------------------------

    fn errorf(&self, msg: impl AsRef<str>) -> String {
        format!(
            "template: {}:{}: {}",
            self.parse_name,
            self.token[0].line,
            msg.as_ref()
        )
    }

    fn unexpected(&self, token: &Item, context: &str) -> String {
        if token.typ == ItemType::Error {
            let mut extra = String::new();
            if self.action_line != 0 && self.action_line != token.line {
                extra = format!(
                    " in action started at {}:{}",
                    self.parse_name, self.action_line
                );
                if token.val.ends_with(" action") {
                    // Avoid "action in action".
                    extra = extra[" in action".len()..].to_string();
                }
            }
            return self.errorf(format!("{}{}", token.render(), extra));
        }
        self.errorf(format!("unexpected {} in {}", token.render(), context))
    }

    fn expect(&mut self, sh: &mut Shared, expected: ItemType, context: &str) -> PResult<Item> {
        let token = self.next_non_space(sh);
        if token.typ != expected {
            return Err(self.unexpected(&token, context));
        }
        Ok(token)
    }

    fn expect_one_of(
        &mut self,
        sh: &mut Shared,
        a: ItemType,
        b: ItemType,
        context: &str,
    ) -> PResult<Item> {
        let token = self.next_non_space(sh);
        if token.typ != a && token.typ != b {
            return Err(self.unexpected(&token, context));
        }
        Ok(token)
    }

    // -- tree set ----------------------------------------------------------

    fn add(&mut self, sh: &mut Shared) -> PResult<()> {
        let existing_is_empty = match sh.tree_set.get(&self.name) {
            None => true,
            Some(root) => is_empty_tree(root.as_ref()),
        };
        if existing_is_empty {
            sh.tree_set.insert(self.name.clone(), self.root.clone());
            return Ok(());
        }
        if !is_empty_tree(self.root.as_ref()) {
            return Err(self.errorf(format!(
                "template: multiple definition of template {}",
                strconv::quote(&self.name)
            )));
        }
        Ok(())
    }

    // -- productions -------------------------------------------------------

    /// Mirrors `(*Tree).parse`: the top level, which also takes `{{define}}`.
    fn parse_root(&mut self, sh: &mut Shared) -> PResult<()> {
        let mut root = Vec::new();
        while self.peek(sh).typ != ItemType::Eof {
            if self.peek(sh).typ == ItemType::LeftDelim {
                let delim = self.next(sh);
                if self.next_non_space(sh).typ == ItemType::Define {
                    let mut new_t = Tree::new("definition", &self.parse_name);
                    new_t.parse_definition(sh)?;
                    continue;
                }
                self.backup2(delim);
            }
            let n = self.text_or_action(sh)?;
            if n.is_end_or_else() {
                self.root = Some(Node::List(root));
                return Err(self.errorf(format!("unexpected {}", n.render())));
            }
            root.push(n);
        }
        self.root = Some(Node::List(root));
        Ok(())
    }

    fn parse_definition(&mut self, sh: &mut Shared) -> PResult<()> {
        const CONTEXT: &str = "define clause";
        let name = self.expect_one_of(sh, ItemType::String, ItemType::RawString, CONTEXT)?;
        self.name = strconv::unquote(&name.val).map_err(|e| self.errorf(e.text()))?;
        self.expect(sh, ItemType::RightDelim, CONTEXT)?;
        let (list, end) = self.item_list(sh)?;
        self.root = Some(list);
        if !matches!(end, Node::End) {
            return Err(self.errorf(format!("unexpected {} in {}", end.render(), CONTEXT)));
        }
        self.add(sh)
    }

    /// Mirrors `(*Tree).itemList`, returning the list and the node that ended
    /// it (`{{end}}` or `{{else}}`).
    fn item_list(&mut self, sh: &mut Shared) -> PResult<(Node, Node)> {
        let mut list = Vec::new();
        while self.peek_non_space(sh).typ != ItemType::Eof {
            let n = self.text_or_action(sh)?;
            if n.is_end_or_else() {
                return Ok((Node::List(list), n));
            }
            list.push(n);
        }
        Err(self.errorf("unexpected EOF"))
    }

    fn text_or_action(&mut self, sh: &mut Shared) -> PResult<Node> {
        let token = self.next_non_space(sh);
        match token.typ {
            ItemType::Text => Ok(Node::Text(token.val)),
            ItemType::LeftDelim => {
                self.action_line = token.line;
                let n = self.action(sh);
                self.action_line = 0;
                n
            }
            ItemType::Comment => Ok(Node::Comment),
            _ => Err(self.unexpected(&token, "input")),
        }
    }

    fn action(&mut self, sh: &mut Shared) -> PResult<Node> {
        let token = self.next_non_space(sh);
        match token.typ {
            ItemType::Block => return self.block_control(sh),
            ItemType::Break => return self.break_control(sh, "{{break}}"),
            ItemType::Continue => return self.break_control(sh, "{{continue}}"),
            ItemType::Else => return self.else_control(sh),
            ItemType::End => return self.end_control(sh),
            ItemType::If => return self.parse_control(sh, "if").map(|_| Node::If),
            ItemType::Range => return self.parse_control(sh, "range").map(|_| Node::Range),
            ItemType::Template => return self.template_control(sh),
            ItemType::With => return self.parse_control(sh, "with").map(|_| Node::With),
            _ => {}
        }
        self.backup();
        // Variables are not popped here; they persist until "end".
        self.pipeline(sh, "command", ItemType::RightDelim)?;
        Ok(Node::Action)
    }

    /// `{{break}}` and `{{continue}}`, which differ only in their wording.
    fn break_control(&mut self, sh: &mut Shared, context: &str) -> PResult<Node> {
        let token = self.next_non_space(sh);
        if token.typ != ItemType::RightDelim {
            return Err(self.unexpected(&token, context));
        }
        if self.range_depth == 0 {
            return Err(self.errorf(format!("{context} outside {{{{range}}}}")));
        }
        Ok(if context == "{{break}}" {
            Node::Break
        } else {
            Node::Continue
        })
    }

    fn pipeline(&mut self, sh: &mut Shared, context: &str, end: ItemType) -> PResult<Pipe> {
        let mut pipe = Pipe::default();

        // Declarations or assignments?
        'decls: loop {
            let v = self.peek_non_space(sh);
            if v.typ != ItemType::Variable {
                break;
            }
            self.next(sh);
            // Three-token lookahead in the worst case: in "$x foo" we must read
            // "foo" (rather than ":=") to know $x is an argument, not a
            // declaration.
            let token_after_variable = self.peek(sh);
            let next = self.peek_non_space(sh);
            if next.typ == ItemType::Assign || next.typ == ItemType::Declare {
                self.next_non_space(sh);
                pipe.decl += 1;
                self.vars.push(v.val.clone());
            } else if next.typ == ItemType::Char && next.val == "," {
                self.next_non_space(sh);
                pipe.decl += 1;
                self.vars.push(v.val.clone());
                if context == "range" && pipe.decl < 2 {
                    match self.peek_non_space(sh).typ {
                        ItemType::Variable | ItemType::RightDelim | ItemType::RightParen => {
                            // A second initialized variable in a range pipeline.
                            continue 'decls;
                        }
                        _ => return Err(self.errorf("range can only initialize variables")),
                    }
                }
                return Err(self.errorf(format!("too many declarations in {context}")));
            } else if token_after_variable.typ == ItemType::Space {
                self.backup3(v, token_after_variable);
            } else {
                self.backup2(v);
            }
            break;
        }

        loop {
            let token = self.next_non_space(sh);
            if token.typ == end {
                self.check_pipeline(&pipe, context)?;
                return Ok(pipe);
            }
            match token.typ {
                ItemType::Bool
                | ItemType::CharConstant
                | ItemType::Complex
                | ItemType::Dot
                | ItemType::Field
                | ItemType::Identifier
                | ItemType::Number
                | ItemType::Nil
                | ItemType::RawString
                | ItemType::String
                | ItemType::Variable
                | ItemType::LeftParen => {
                    self.backup();
                    let cmd = self.command(sh)?;
                    pipe.cmds.push(cmd);
                }
                _ => return Err(self.unexpected(&token, context)),
            }
        }
    }

    fn check_pipeline(&self, pipe: &Pipe, context: &str) -> PResult<()> {
        if pipe.cmds.is_empty() {
            return Err(self.errorf(format!("missing value for {context}")));
        }
        // Only the first command of a pipeline may start with a non-executable
        // operand.
        for (i, cmd) in pipe.cmds.iter().skip(1).enumerate() {
            if matches!(
                cmd.first(),
                Some(Node::Bool(_) | Node::Dot | Node::Nil | Node::Number(_) | Node::Str(_))
            ) {
                return Err(self.errorf(format!(
                    "non executable command in pipeline stage {}",
                    i + 2
                )));
            }
        }
        Ok(())
    }

    /// Mirrors `(*Tree).parseControl`, shared by if / range / with.
    fn parse_control(&mut self, sh: &mut Shared, context: &str) -> PResult<()> {
        self.enter()?;
        let vars_at_entry = self.vars.len();
        self.pipeline(sh, context, ItemType::RightDelim)?;
        if context == "range" {
            self.range_depth += 1;
        }
        let (_, next) = self.item_list(sh)?;
        if context == "range" {
            self.range_depth -= 1;
        }
        if matches!(next, Node::Else) {
            // "{{else if}}" and "{{else with}}" are parsed as a nested if/with
            // whose {{end}} closes both.
            if context == "if" && self.peek(sh).typ == ItemType::If {
                self.next(sh);
                self.parse_control(sh, "if")?;
            } else if context == "with" && self.peek(sh).typ == ItemType::With {
                self.next(sh);
                self.parse_control(sh, "with")?;
            } else {
                let (_, next) = self.item_list(sh)?;
                if !matches!(next, Node::End) {
                    return Err(self.errorf(format!("expected end; found {}", next.render())));
                }
            }
        }
        self.vars.truncate(vars_at_entry);
        self.leave();
        Ok(())
    }

    fn end_control(&mut self, sh: &mut Shared) -> PResult<Node> {
        self.expect(sh, ItemType::RightDelim, "end")?;
        Ok(Node::End)
    }

    fn else_control(&mut self, sh: &mut Shared) -> PResult<Node> {
        // "{{else if ...}}" and "{{else with ...}}" become "{{else}}{{if ...}}"
        // and "{{else}}{{with ...}}", so the else node is returned here.
        let peek = self.peek_non_space(sh);
        if peek.typ == ItemType::If || peek.typ == ItemType::With {
            return Ok(Node::Else);
        }
        self.expect(sh, ItemType::RightDelim, "else")?;
        Ok(Node::Else)
    }

    fn block_control(&mut self, sh: &mut Shared) -> PResult<Node> {
        const CONTEXT: &str = "block clause";
        self.enter()?;
        let token = self.next_non_space(sh);
        let name = self.parse_template_name(&token, CONTEXT)?;
        self.pipeline(sh, CONTEXT, ItemType::RightDelim)?;

        let mut block = Tree::new(&name, &self.parse_name);
        let (list, end) = block.item_list(sh)?;
        block.root = Some(list);
        if !matches!(end, Node::End) {
            return Err(self.errorf(format!("unexpected {} in {}", end.render(), CONTEXT)));
        }
        block.add(sh)?;
        self.leave();
        Ok(Node::Template)
    }

    fn template_control(&mut self, sh: &mut Shared) -> PResult<Node> {
        const CONTEXT: &str = "template clause";
        let token = self.next_non_space(sh);
        self.parse_template_name(&token, CONTEXT)?;
        if self.next_non_space(sh).typ != ItemType::RightDelim {
            self.backup();
            // Variables are not popped; they persist until "end".
            self.pipeline(sh, CONTEXT, ItemType::RightDelim)?;
        }
        Ok(Node::Template)
    }

    fn parse_template_name(&self, token: &Item, context: &str) -> PResult<String> {
        match token.typ {
            ItemType::String | ItemType::RawString => {
                strconv::unquote(&token.val).map_err(|e| self.errorf(e.text()))
            }
            _ => Err(self.unexpected(token, context)),
        }
    }

    fn command(&mut self, sh: &mut Shared) -> PResult<Vec<Node>> {
        let mut args: Vec<Node> = Vec::new();
        loop {
            self.peek_non_space(sh); // skip leading spaces
            if let Some(operand) = self.operand(sh)? {
                args.push(operand);
            }
            let token = self.next(sh);
            match token.typ {
                ItemType::Space => continue,
                ItemType::RightDelim | ItemType::RightParen => self.backup(),
                ItemType::Pipe => {}
                _ => return Err(self.unexpected(&token, "operand")),
            }
            break;
        }
        if args.is_empty() {
            return Err(self.errorf("empty command"));
        }
        Ok(args)
    }

    /// `term .Field*`. `None` means the next item is not an operand.
    fn operand(&mut self, sh: &mut Shared) -> PResult<Option<Node>> {
        let Some(node) = self.term(sh)? else {
            return Ok(None);
        };
        if self.peek(sh).typ != ItemType::Field {
            return Ok(Some(node));
        }
        while self.peek(sh).typ == ItemType::Field {
            self.next(sh);
        }
        // Keeping the fields on the original node for a field or variable is a
        // compatibility wart of the original API; what matters here is that a
        // literal followed by a field is an error.
        Ok(Some(match node {
            Node::Field => Node::Field,
            Node::Variable => Node::Variable,
            Node::Bool(_) | Node::Str(_) | Node::Number(_) | Node::Nil | Node::Dot => {
                return Err(self.errorf(format!(
                    "unexpected . after term {}",
                    strconv::quote(&node.render())
                )))
            }
            _ => Node::Chain,
        }))
    }

    /// A literal, a function, `.`, `.Field`, `$`, or a parenthesized pipeline.
    /// `None` means the next item is not a term.
    fn term(&mut self, sh: &mut Shared) -> PResult<Option<Node>> {
        let token = self.next_non_space(sh);
        match token.typ {
            ItemType::Identifier => {
                if !BUILTINS.contains(&token.val.as_str()) {
                    return Err(self.errorf(format!(
                        "function {} not defined",
                        strconv::quote(&token.val)
                    )));
                }
                Ok(Some(Node::Identifier))
            }
            ItemType::Dot => Ok(Some(Node::Dot)),
            ItemType::Nil => Ok(Some(Node::Nil)),
            ItemType::Variable => {
                let ident = token.val.split('.').next().unwrap_or("").to_string();
                if !self.vars.contains(&ident) {
                    return Err(
                        self.errorf(format!("undefined variable {}", strconv::quote(&ident)))
                    );
                }
                Ok(Some(Node::Variable))
            }
            ItemType::Field => Ok(Some(Node::Field)),
            ItemType::Bool => Ok(Some(Node::Bool(token.val == "true"))),
            ItemType::CharConstant | ItemType::Complex | ItemType::Number => {
                new_number(&token.val, token.typ).map_err(|e| self.errorf(e))?;
                Ok(Some(Node::Number(token.val)))
            }
            ItemType::LeftParen => {
                if self.stack_depth >= MAX_STACK_DEPTH {
                    return Err(self.errorf("max expression depth exceeded"));
                }
                self.enter()?;
                self.stack_depth += 1;
                self.pipeline(sh, "parenthesized pipeline", ItemType::RightParen)?;
                self.stack_depth -= 1;
                self.leave();
                Ok(Some(Node::Pipe))
            }
            ItemType::String | ItemType::RawString => {
                strconv::unquote(&token.val).map_err(|e| self.errorf(e.text()))?;
                Ok(Some(Node::Str(token.val)))
            }
            _ => {
                self.backup();
                Ok(None)
            }
        }
    }
}

/// Mirrors `(*Tree).newNumber`, which is where a numeric literal is validated.
/// Only the error matters; every value it computes is dropped.
fn new_number(text: &str, typ: ItemType) -> Result<(), String> {
    match typ {
        ItemType::CharConstant => {
            let quote = text.as_bytes()[0];
            let (_, _, tail) =
                strconv::unquote_char(&text[1..], quote).map_err(|e| e.text().to_string())?;
            if tail != "'" {
                return Err(format!("malformed character constant: {text}"));
            }
            return Ok(());
        }
        ItemType::Complex => {
            // fmt.Sscan parses the pair, so let it do the work.
            return gofmt::sscan_complex(text);
        }
        _ => {}
    }

    // An imaginary constant can only be complex unless it is zero.
    if text.ends_with('i') && strconv::parse_float(&text[..text.len() - 1]).is_ok() {
        return Ok(());
    }

    // The integer test comes first, so that 0x123 and friends are caught here.
    let is_uint = strconv::parse_uint(text, 0, 64).is_ok();
    let is_int = strconv::parse_int(text, 0, 64).is_ok();
    let mut is_float = is_int || is_uint;

    if !is_float && strconv::parse_float(text).is_ok() {
        // Parsed as a float but shaped like an integer: too large for an int.
        if !text.contains(['.', 'e', 'E', 'p', 'P']) {
            return Err(format!("integer overflow: {}", strconv::quote(text)));
        }
        is_float = true;
    }

    if !is_float {
        return Err(format!("illegal number syntax: {}", strconv::quote(text)));
    }
    Ok(())
}

/// Mirrors `template.New("").Parse(text)`: `Ok` if the template parses, the
/// error text Go would return otherwise.
///
/// `html/template.New("").Parse` returns the same errors — its `Parse` hands
/// the text straight to this one — which the oracle checks on every corpus row.
pub fn parse(text: &str) -> Result<(), String> {
    let mut sh = Shared {
        lex: Lexer::new(text),
        tree_set: HashMap::new(),
    };
    let mut t = Tree::new("", "");
    t.parse_root(&mut sh)?;
    t.add(&mut sh)
}
