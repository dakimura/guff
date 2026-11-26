// Port of Go's go/build/constraint to Rust.
//
// Original: Copyright 2020/2023 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// Combines `expr.go` (parsing + evaluation) and `vers.go` (minimum Go
// version extraction). The single Go interface `Expr` (with 4 concrete
// types) becomes a closed enum [`Expr`].
//
// Notable differences from Go:
//
// * Errors are propagated via `Result<_, SyntaxError>` instead of
//   `panic`/`recover`.
// * `Eval` takes `&mut dyn FnMut(&str) -> bool` to match Go's habit of
//   capturing closures that mutate a tag set.
// * The `// +build` "expression too complex" error becomes a typed
//   [`PlusBuildError::TooComplex`] variant.

use std::fmt;

// ====================================================================
// Expr
// ====================================================================

/// Boolean build-tag expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A single build tag, e.g. `linux` or `cgo`.
    Tag(String),
    /// `!X`.
    Not(Box<Expr>),
    /// `X && Y`.
    And(Box<Expr>, Box<Expr>),
    /// `X || Y`.
    Or(Box<Expr>, Box<Expr>),
}

impl Expr {
    pub fn tag(t: impl Into<String>) -> Expr {
        Expr::Tag(t.into())
    }
    pub fn not(x: Expr) -> Expr {
        Expr::Not(Box::new(x))
    }
    pub fn and(x: Expr, y: Expr) -> Expr {
        Expr::And(Box::new(x), Box::new(y))
    }
    pub fn or(x: Expr, y: Expr) -> Expr {
        Expr::Or(Box::new(x), Box::new(y))
    }

    /// `true` iff the expression evaluates to true under `ok`, which
    /// reports whether a given build tag is satisfied.
    pub fn eval(&self, ok: &mut dyn FnMut(&str) -> bool) -> bool {
        match self {
            Expr::Tag(t) => ok(t),
            Expr::Not(x) => !x.eval(ok),
            // AND/OR evaluate both sides so the closure observes every
            // tag — matches Go's documented behavior.
            Expr::And(x, y) => {
                let xok = x.eval(ok);
                let yok = y.eval(ok);
                xok && yok
            }
            Expr::Or(x, y) => {
                let xok = x.eval(ok);
                let yok = y.eval(ok);
                xok || yok
            }
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expr::Tag(t) => f.write_str(t),
            Expr::Not(x) => {
                let inner = match x.as_ref() {
                    Expr::And(_, _) | Expr::Or(_, _) => format!("({})", x),
                    _ => format!("{}", x),
                };
                write!(f, "!{}", inner)
            }
            Expr::And(x, y) => write!(f, "{} && {}", and_arg(x), and_arg(y)),
            Expr::Or(x, y) => write!(f, "{} || {}", or_arg(x), or_arg(y)),
        }
    }
}

fn and_arg(x: &Expr) -> String {
    match x {
        Expr::Or(_, _) => format!("({})", x),
        _ => x.to_string(),
    }
}

fn or_arg(x: &Expr) -> String {
    match x {
        Expr::And(_, _) => format!("({})", x),
        _ => x.to_string(),
    }
}

// ====================================================================
// SyntaxError
// ====================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub offset: usize,
    pub err: String,
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.err)
    }
}

impl std::error::Error for SyntaxError {}

impl SyntaxError {
    fn new(offset: usize, err: impl Into<String>) -> Self {
        SyntaxError {
            offset,
            err: err.into(),
        }
    }
}

// ====================================================================
// Parse / IsGoBuild / IsPlusBuild
// ====================================================================

/// Top-level error returned by [`parse`] when the input is not a
/// recognized build-constraint line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input does not start with `//go:build` or `// +build`.
    NotConstraint,
    /// Syntactic problem within a `//go:build` expression.
    Syntax(SyntaxError),
    /// `// +build` expression too complex.
    PlusBuildTooComplex,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::NotConstraint => f.write_str("not a build constraint"),
            ParseError::Syntax(e) => fmt::Display::fmt(e, f),
            ParseError::PlusBuildTooComplex => {
                f.write_str("expression too complex for // +build lines")
            }
        }
    }
}

impl std::error::Error for ParseError {}

impl From<SyntaxError> for ParseError {
    fn from(e: SyntaxError) -> Self {
        ParseError::Syntax(e)
    }
}

/// Parse a single build-constraint line of the form `//go:build …` or
/// `// +build …`.
pub fn parse(line: &str) -> Result<Expr, ParseError> {
    if let Some(text) = split_go_build(line) {
        return parse_expr(text).map_err(ParseError::Syntax);
    }
    if let Some(text) = split_plus_build(line) {
        return parse_plus_build_expr(text);
    }
    Err(ParseError::NotConstraint)
}

/// `true` iff `line` is a `//go:build` constraint comment (prefix
/// check only — does not validate the expression).
pub fn is_go_build(line: &str) -> bool {
    split_go_build(line).is_some()
}

/// `true` iff `line` is a `// +build` constraint comment.
pub fn is_plus_build(line: &str) -> bool {
    split_plus_build(line).is_some()
}

fn strip_trailing_newline(line: &str) -> Option<&str> {
    let mut s = line;
    if s.ends_with('\n') {
        s = &s[..s.len() - 1];
    }
    if s.contains('\n') {
        return None;
    }
    Some(s)
}

fn split_go_build(line: &str) -> Option<&str> {
    let line = strip_trailing_newline(line)?;
    if !line.starts_with("//go:build") {
        return None;
    }
    let line = line.trim();
    let rest = &line["//go:build".len()..];
    let trimmed = rest.trim();
    // The prefix must be followed by whitespace (or be the entire line).
    if rest.len() == trimmed.len() && !rest.is_empty() {
        return None;
    }
    Some(trimmed)
}

fn split_plus_build(line: &str) -> Option<&str> {
    let line = strip_trailing_newline(line)?;
    if !line.starts_with("//") {
        return None;
    }
    let rest = &line[2..];
    let rest = rest.trim();
    if !rest.starts_with("+build") {
        return None;
    }
    let rest = &rest["+build".len()..];
    let trimmed = rest.trim();
    if rest.len() == trimmed.len() && !rest.is_empty() {
        return None;
    }
    Some(trimmed)
}

// ====================================================================
// Recursive-descent parser
// ====================================================================

const MAX_SIZE: usize = 1000;

struct ExprParser<'a> {
    s: &'a str,
    i: usize,
    tok: String,
    is_tag: bool,
    pos: usize,
    size: usize,
}

/// Parse a boolean build-tag expression (without the `//go:build`
/// prefix).
pub fn parse_expr(text: &str) -> Result<Expr, SyntaxError> {
    let mut p = ExprParser {
        s: text,
        i: 0,
        tok: String::new(),
        is_tag: false,
        pos: 0,
        size: 0,
    };
    let x = p.or()?;
    if !p.tok.is_empty() {
        return Err(SyntaxError::new(
            p.pos,
            format!("unexpected token {}", p.tok),
        ));
    }
    Ok(x)
}

impl<'a> ExprParser<'a> {
    fn or(&mut self) -> Result<Expr, SyntaxError> {
        let mut x = self.and()?;
        while self.tok == "||" {
            let y = self.and()?;
            x = Expr::or(x, y);
        }
        Ok(x)
    }

    fn and(&mut self) -> Result<Expr, SyntaxError> {
        let mut x = self.not()?;
        while self.tok == "&&" {
            let y = self.not()?;
            x = Expr::and(x, y);
        }
        Ok(x)
    }

    fn not(&mut self) -> Result<Expr, SyntaxError> {
        self.size += 1;
        if self.size > MAX_SIZE {
            return Err(SyntaxError::new(self.pos, "build expression too large"));
        }
        self.lex()?;
        if self.tok == "!" {
            self.lex()?;
            if self.tok == "!" {
                return Err(SyntaxError::new(self.pos, "double negation not allowed"));
            }
            let a = self.atom()?;
            return Ok(Expr::not(a));
        }
        self.atom()
    }

    fn atom(&mut self) -> Result<Expr, SyntaxError> {
        if self.tok == "(" {
            let outer_pos = self.pos;
            let x = match self.or() {
                Ok(x) => x,
                Err(mut e) => {
                    // Mirror Go's defer: an inner "unexpected end of
                    // expression" turns into "missing close paren" but
                    // keeps its original offset.
                    if e.err == "unexpected end of expression" {
                        e.err = "missing close paren".to_string();
                    }
                    return Err(e);
                }
            };
            if self.tok != ")" {
                return Err(SyntaxError::new(outer_pos, "missing close paren"));
            }
            self.lex()?;
            return Ok(x);
        }

        if !self.is_tag {
            if self.tok.is_empty() {
                return Err(SyntaxError::new(self.pos, "unexpected end of expression"));
            }
            return Err(SyntaxError::new(
                self.pos,
                format!("unexpected token {}", self.tok),
            ));
        }
        let tok = self.tok.clone();
        self.lex()?;
        Ok(Expr::Tag(tok))
    }

    fn lex(&mut self) -> Result<(), SyntaxError> {
        self.is_tag = false;
        let bytes = self.s.as_bytes();
        while self.i < bytes.len() && (bytes[self.i] == b' ' || bytes[self.i] == b'\t') {
            self.i += 1;
        }
        if self.i >= bytes.len() {
            self.tok.clear();
            self.pos = self.i;
            return Ok(());
        }
        let b = bytes[self.i];
        match b {
            b'(' | b')' | b'!' => {
                self.pos = self.i;
                self.i += 1;
                self.tok = (b as char).to_string();
                return Ok(());
            }
            b'&' | b'|' => {
                if self.i + 1 >= bytes.len() || bytes[self.i + 1] != b {
                    return Err(SyntaxError::new(
                        self.i,
                        format!("invalid syntax at {}", b as char),
                    ));
                }
                self.pos = self.i;
                self.i += 2;
                self.tok = format!("{0}{0}", b as char);
                return Ok(());
            }
            _ => {}
        }

        // Tag: letters, digits, '_', '.'. Matches Go's
        // unicode.IsLetter|unicode.IsDigit; we approximate the latter
        // as ASCII digits since real build tags are always ASCII.
        let tail = &self.s[self.i..];
        let mut tag_len = 0usize;
        for (off, c) in tail.char_indices() {
            if !is_tag_char(c) {
                tag_len = off;
                break;
            }
            tag_len = off + c.len_utf8();
        }
        if tag_len == 0 {
            // Decode the first rune to embed it in the error message.
            let c = tail.chars().next().unwrap_or('\0');
            return Err(SyntaxError::new(self.i, format!("invalid syntax at {}", c)));
        }
        self.pos = self.i;
        self.tok = self.s[self.pos..self.pos + tag_len].to_string();
        self.i += tag_len;
        self.is_tag = true;
        Ok(())
    }
}

fn is_tag_char(c: char) -> bool {
    c.is_alphabetic() || c.is_ascii_digit() || c == '_' || c == '.'
}

// ====================================================================
// // +build parser
// ====================================================================

const MAX_OLD_SIZE: usize = 100;

/// Parse a legacy `// +build` expression (the text after the
/// `// +build` prefix, with optional leading/trailing whitespace).
pub fn parse_plus_build_expr(text: &str) -> Result<Expr, ParseError> {
    let mut size = 0usize;
    let mut x: Option<Expr> = None;
    for clause in text.split_whitespace() {
        let mut y: Option<Expr> = None;
        for lit in clause.split(',') {
            let z: Expr;
            let mut neg = false;
            if lit.starts_with("!!") || lit == "!" {
                z = Expr::tag("ignore");
            } else {
                let mut l = lit;
                if let Some(rest) = l.strip_prefix('!') {
                    neg = true;
                    l = rest;
                }
                let inner = if is_valid_tag(l) {
                    Expr::tag(l)
                } else {
                    Expr::tag("ignore")
                };
                z = if neg { Expr::not(inner) } else { inner };
            }
            y = Some(match y {
                None => z,
                Some(prev) => {
                    size += 1;
                    if size > MAX_OLD_SIZE {
                        return Err(ParseError::PlusBuildTooComplex);
                    }
                    Expr::and(prev, z)
                }
            });
        }
        if let Some(y) = y {
            x = Some(match x {
                None => y,
                Some(prev) => {
                    size += 1;
                    if size > MAX_OLD_SIZE {
                        return Err(ParseError::PlusBuildTooComplex);
                    }
                    Expr::or(prev, y)
                }
            });
        }
    }
    Ok(x.unwrap_or_else(|| Expr::tag("ignore")))
}

fn is_valid_tag(word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    word.chars().all(is_tag_char)
}

// ====================================================================
// PlusBuildLines
// ====================================================================

/// Convert `x` back to a sequence of `// +build` lines. Returns an
/// error if the expression is too complex (contains non-CNF shapes
/// that `// +build` can't represent).
pub fn plus_build_lines(x: &Expr) -> Result<Vec<String>, ParseError> {
    let pushed = push_not(x.clone(), false);

    // Decompose into AND of ORs of ANDs of literals.
    let mut split: Vec<Vec<Vec<Expr>>> = Vec::new();
    for or_expr in append_split_and(Vec::new(), &pushed) {
        let mut ands: Vec<Vec<Expr>> = Vec::new();
        for and_expr in append_split_or(Vec::new(), &or_expr) {
            let mut lits: Vec<Expr> = Vec::new();
            for lit in append_split_and(Vec::new(), &and_expr) {
                match lit {
                    Expr::Tag(_) | Expr::Not(_) => lits.push(lit),
                    _ => return Err(ParseError::PlusBuildTooComplex),
                }
            }
            ands.push(lits);
        }
        split.push(ands);
    }

    // If every OR has length 1, flatten into a single line.
    let max_or = split.iter().map(|or| or.len()).max().unwrap_or(0);
    if max_or == 1 {
        let mut lits: Vec<Expr> = Vec::new();
        for or in split {
            for and in or {
                lits.extend(and);
            }
        }
        split = vec![vec![lits]];
    }

    let mut lines: Vec<String> = Vec::new();
    for or in split {
        let mut line = String::from("// +build");
        for and in or {
            line.push(' ');
            for (i, lit) in and.iter().enumerate() {
                if i > 0 {
                    line.push(',');
                }
                line.push_str(&lit.to_string());
            }
        }
        lines.push(line);
    }

    Ok(lines)
}

fn push_not(x: Expr, not: bool) -> Expr {
    match x {
        Expr::Not(inner) => {
            // `!tag` with not=false stays as-is.
            if matches!(*inner, Expr::Tag(_)) && !not {
                Expr::Not(inner)
            } else {
                push_not(*inner, !not)
            }
        }
        Expr::Tag(t) => {
            if not {
                Expr::not(Expr::Tag(t))
            } else {
                Expr::Tag(t)
            }
        }
        Expr::And(x, y) => {
            let x1 = push_not(*x, not);
            let y1 = push_not(*y, not);
            if not {
                Expr::or(x1, y1)
            } else {
                Expr::and(x1, y1)
            }
        }
        Expr::Or(x, y) => {
            let x1 = push_not(*x, not);
            let y1 = push_not(*y, not);
            if not {
                Expr::and(x1, y1)
            } else {
                Expr::or(x1, y1)
            }
        }
    }
}

fn append_split_and(mut list: Vec<Expr>, x: &Expr) -> Vec<Expr> {
    if let Expr::And(a, b) = x {
        list = append_split_and(list, a);
        list = append_split_and(list, b);
        return list;
    }
    list.push(x.clone());
    list
}

fn append_split_or(mut list: Vec<Expr>, x: &Expr) -> Vec<Expr> {
    if let Expr::Or(a, b) = x {
        list = append_split_or(list, a);
        list = append_split_or(list, b);
        return list;
    }
    list.push(x.clone());
    list
}

// ====================================================================
// vers.go: GoVersion
// ====================================================================

/// Minimum Go version implied by the expression, or `None` if no Go
/// version tag is required.
pub fn go_version(x: &Expr) -> Option<String> {
    let v = min_version(x, 1);
    if v < 0 {
        return None;
    }
    if v == 0 {
        return Some("go1".to_string());
    }
    Some(format!("go1.{}", v))
}

fn min_version(z: &Expr, sign: i32) -> i32 {
    match z {
        Expr::And(x, y) => {
            let xv = min_version(x, sign);
            let yv = min_version(y, sign);
            if sign < 0 {
                or_version(xv, yv)
            } else {
                and_version(xv, yv)
            }
        }
        Expr::Or(x, y) => {
            let xv = min_version(x, sign);
            let yv = min_version(y, sign);
            if sign < 0 {
                and_version(xv, yv)
            } else {
                or_version(xv, yv)
            }
        }
        Expr::Not(x) => min_version(x, -sign),
        Expr::Tag(t) => {
            if sign < 0 {
                // !foo implies nothing.
                return -1;
            }
            if t == "go1" {
                return 0;
            }
            match t.strip_prefix("go1.").and_then(|v| v.parse::<i32>().ok()) {
                Some(n) => n,
                None => -1, // not a go1.N tag
            }
        }
    }
}

fn and_version(x: i32, y: i32) -> i32 {
    x.max(y)
}

fn or_version(x: i32, y: i32) -> i32 {
    x.min(y)
}

// ====================================================================
// Tests
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ------------- Expr String --------------------------------------

    #[test]
    fn expr_string_tag() {
        assert_eq!(Expr::tag("abc").to_string(), "abc");
    }

    #[test]
    fn expr_string_not_tag() {
        assert_eq!(Expr::not(Expr::tag("abc")).to_string(), "!abc");
    }

    #[test]
    fn expr_string_not_and() {
        let e = Expr::not(Expr::and(Expr::tag("abc"), Expr::tag("def")));
        assert_eq!(e.to_string(), "!(abc && def)");
    }

    #[test]
    fn expr_string_and_or() {
        let e = Expr::and(
            Expr::tag("abc"),
            Expr::or(Expr::tag("def"), Expr::tag("ghi")),
        );
        assert_eq!(e.to_string(), "abc && (def || ghi)");
    }

    #[test]
    fn expr_string_or_and() {
        let e = Expr::or(
            Expr::and(Expr::tag("abc"), Expr::tag("def")),
            Expr::tag("ghi"),
        );
        assert_eq!(e.to_string(), "(abc && def) || ghi");
    }

    // ------------- Lex ----------------------------------------------

    fn lex_dump(input: &str) -> String {
        let mut p = ExprParser {
            s: input,
            i: 0,
            tok: String::new(),
            is_tag: false,
            pos: 0,
            size: 0,
        };
        let mut out = String::new();
        loop {
            match p.lex() {
                Ok(()) => {}
                Err(e) => {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str("err: ");
                    out.push_str(&e.err);
                    return out;
                }
            }
            if p.tok.is_empty() {
                return out;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(&p.tok);
        }
    }

    #[test]
    fn lex_cases() {
        let cases = [
            ("", ""),
            ("x", "x"),
            ("x.y", "x.y"),
            ("x_y", "x_y"),
            ("αx", "αx"),
            ("αx²", "αx err: invalid syntax at ²"),
            ("go1.2", "go1.2"),
            ("x y", "x y"),
            ("x!y", "x ! y"),
            ("&&||!()xy yx ", "&& || ! ( ) xy yx"),
            ("x~", "x err: invalid syntax at ~"),
            ("x ~", "x err: invalid syntax at ~"),
            ("x &", "x err: invalid syntax at &"),
            ("x &y", "x err: invalid syntax at &"),
        ];
        for (input, want) in cases {
            let got = lex_dump(input);
            assert_eq!(got, want, "lex({:?})", input);
        }
    }

    // ------------- parseExpr happy cases -----------------------------

    #[test]
    fn parse_expr_cases() {
        let cases: &[(&str, Expr)] = &[
            ("x", Expr::tag("x")),
            ("x&&y", Expr::and(Expr::tag("x"), Expr::tag("y"))),
            ("x||y", Expr::or(Expr::tag("x"), Expr::tag("y"))),
            ("(x)", Expr::tag("x")),
            (
                "x||y&&z",
                Expr::or(Expr::tag("x"), Expr::and(Expr::tag("y"), Expr::tag("z"))),
            ),
            (
                "x&&y||z",
                Expr::or(Expr::and(Expr::tag("x"), Expr::tag("y")), Expr::tag("z")),
            ),
            (
                "x&&(y||z)",
                Expr::and(Expr::tag("x"), Expr::or(Expr::tag("y"), Expr::tag("z"))),
            ),
            (
                "(x||y)&&z",
                Expr::and(Expr::or(Expr::tag("x"), Expr::tag("y")), Expr::tag("z")),
            ),
            (
                "!(x&&y)",
                Expr::not(Expr::and(Expr::tag("x"), Expr::tag("y"))),
            ),
        ];
        for (input, want) in cases {
            let got = parse_expr(input).unwrap();
            assert_eq!(got.to_string(), want.to_string(), "parseExpr({:?})", input);
        }
    }

    // ------------- parseExpr error cases -----------------------------

    #[test]
    fn parse_expr_errors() {
        let cases: &[(&str, SyntaxError)] = &[
            ("x && ", SyntaxError::new(5, "unexpected end of expression")),
            ("x && (", SyntaxError::new(6, "missing close paren")),
            ("x && ||", SyntaxError::new(5, "unexpected token ||")),
            (
                "x && !",
                SyntaxError::new(6, "unexpected end of expression"),
            ),
            (
                "x && !!",
                SyntaxError::new(6, "double negation not allowed"),
            ),
            ("x !", SyntaxError::new(2, "unexpected token !")),
            ("x && (y", SyntaxError::new(5, "missing close paren")),
        ];
        for (input, want) in cases {
            let err = parse_expr(input).expect_err("expected error");
            assert_eq!(&err, want, "parseExpr({:?})", input);
        }
    }

    // ------------- Eval ---------------------------------------------

    #[test]
    fn expr_eval() {
        struct Case<'a> {
            input: &'a str,
            ok: bool,
            tags: &'a str,
        }
        let cases = [
            Case {
                input: "x",
                ok: false,
                tags: "x",
            },
            Case {
                input: "x && y",
                ok: false,
                tags: "x y",
            },
            Case {
                input: "x || y",
                ok: false,
                tags: "x y",
            },
            Case {
                input: "!x && yes",
                ok: true,
                tags: "x yes",
            },
            Case {
                input: "yes || y",
                ok: true,
                tags: "y yes",
            },
        ];
        for c in cases {
            let x = parse_expr(c.input).unwrap();
            let mut tags: HashMap<String, bool> = HashMap::new();
            let ok = x.eval(&mut |t: &str| {
                tags.insert(t.to_string(), true);
                t == "yes"
            });
            let want_tags: HashMap<String, bool> = c
                .tags
                .split_whitespace()
                .map(|s| (s.to_string(), true))
                .collect();
            assert_eq!(ok, c.ok, "ok for {:?}", c.input);
            assert_eq!(tags, want_tags, "tags for {:?}", c.input);
        }
    }

    // ------------- parsePlusBuildExpr -------------------------------

    #[test]
    fn parse_plus_build_expr_cases() {
        let cases: &[(&str, Expr)] = &[
            ("x", Expr::tag("x")),
            ("x,y", Expr::and(Expr::tag("x"), Expr::tag("y"))),
            ("x y", Expr::or(Expr::tag("x"), Expr::tag("y"))),
            (
                "x y,z",
                Expr::or(Expr::tag("x"), Expr::and(Expr::tag("y"), Expr::tag("z"))),
            ),
            (
                "x,y z",
                Expr::or(Expr::and(Expr::tag("x"), Expr::tag("y")), Expr::tag("z")),
            ),
            (
                "x,!y !z",
                Expr::or(
                    Expr::and(Expr::tag("x"), Expr::not(Expr::tag("y"))),
                    Expr::not(Expr::tag("z")),
                ),
            ),
            ("!! x", Expr::or(Expr::tag("ignore"), Expr::tag("x"))),
            ("!!x", Expr::tag("ignore")),
            ("!x", Expr::not(Expr::tag("x"))),
            ("!", Expr::tag("ignore")),
            ("", Expr::tag("ignore")),
        ];
        for (input, want) in cases {
            let got = parse_plus_build_expr(input).unwrap();
            assert_eq!(got.to_string(), want.to_string(), "+build {:?}", input);
        }
    }

    // ------------- Parse (top-level) --------------------------------

    #[test]
    fn parse_constraint_cases() {
        struct Case<'a> {
            input: &'a str,
            want_string: Option<&'a str>,
            err_contains: &'a str,
        }
        let cases = [
            Case {
                input: "//+build !",
                want_string: Some("ignore"),
                err_contains: "",
            },
            Case {
                input: "//+build",
                want_string: Some("ignore"),
                err_contains: "",
            },
            Case {
                input: "//+build x y",
                want_string: Some("x || y"),
                err_contains: "",
            },
            Case {
                input: "// +build x y \n",
                want_string: Some("x || y"),
                err_contains: "",
            },
            Case {
                input: "// +build x y \n ",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: "// +build x y \nmore",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: " //+build x y",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: "//go:build x && y",
                want_string: Some("x && y"),
                err_contains: "",
            },
            Case {
                input: "//go:build x && y\n",
                want_string: Some("x && y"),
                err_contains: "",
            },
            Case {
                input: "//go:build x && y\n ",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: "//go:build x && y\nmore",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: " //go:build x && y",
                want_string: None,
                err_contains: "not a build constraint",
            },
            Case {
                input: "//go:build\n",
                want_string: None,
                err_contains: "unexpected end of expression",
            },
        ];
        for c in cases {
            match parse(c.input) {
                Ok(x) => {
                    let want = c.want_string.expect("expected error but got value");
                    assert_eq!(x.to_string(), want, "Parse({:?})", c.input);
                }
                Err(e) => {
                    assert!(
                        c.want_string.is_none(),
                        "Parse({:?}) unexpected error: {}",
                        c.input,
                        e
                    );
                    assert!(
                        e.to_string().contains(c.err_contains),
                        "Parse({:?}) error {:?} doesn't contain {:?}",
                        c.input,
                        e.to_string(),
                        c.err_contains
                    );
                }
            }
        }
    }

    // ------------- PlusBuildLines -----------------------------------

    #[test]
    fn plus_build_lines_cases() {
        let cases: &[(&str, Option<&[&str]>)] = &[
            ("x", Some(&["x"])),
            ("x && !y", Some(&["x,!y"])),
            ("x || y", Some(&["x y"])),
            ("x && (y || z)", Some(&["x", "y z"])),
            ("!(x && y)", Some(&["!x !y"])),
            ("x || (y && z)", Some(&["x y,z"])),
            ("w && (x || (y && z))", Some(&["w", "x y,z"])),
            ("v || (w && (x || (y && z)))", None), // too complex
        ];
        for (input, want) in cases {
            let x = parse_expr(input).unwrap();
            match plus_build_lines(&x) {
                Ok(got) => {
                    let want = want.expect("unexpected ok");
                    let expected: Vec<String> =
                        want.iter().map(|s| format!("// +build {}", s)).collect();
                    assert_eq!(got, expected, "PlusBuildLines({:?})", input);
                }
                Err(ParseError::PlusBuildTooComplex) => {
                    assert!(want.is_none(), "unexpected too-complex on {:?}", input);
                }
                Err(other) => panic!("unexpected error: {}", other),
            }
        }
    }

    // ------------- Size limits --------------------------------------

    #[test]
    fn size_limit_go_build_or() {
        let expr = format!("//go:build {}", "a || ".repeat(MAX_SIZE + 2));
        let err = parse(&expr).expect_err("expected size limit");
        match err {
            ParseError::Syntax(SyntaxError { err, .. }) => {
                assert_eq!(err, "build expression too large");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn size_limit_plus_build_or() {
        let expr = format!("// +build {}", "a ".repeat(MAX_OLD_SIZE + 2));
        let err = parse(&expr).expect_err("expected too complex");
        assert!(matches!(err, ParseError::PlusBuildTooComplex));
    }

    #[test]
    fn size_limit_plus_build_and() {
        let expr = format!("// +build {}", "a,".repeat(MAX_OLD_SIZE + 2));
        let err = parse(&expr).expect_err("expected too complex");
        assert!(matches!(err, ParseError::PlusBuildTooComplex));
    }

    // ------------- vers.go: GoVersion -------------------------------

    #[test]
    fn go_version_cases() {
        let cases: &[(&str, Option<&str>)] = &[
            ("//go:build linux && go1.60", Some("go1.60")),
            ("//go:build ignore && go1.60", Some("go1.60")),
            ("//go:build ignore || go1.60", None),
            ("//go:build go1.50 || (ignore && go1.60)", Some("go1.50")),
            ("// +build go1.60,linux", Some("go1.60")),
            ("// +build go1.60 linux", None),
            ("//go:build go1.50 && !go1.60", Some("go1.50")),
            ("//go:build !go1.60", None),
            (
                "//go:build linux && go1.50 || darwin && go1.60",
                Some("go1.50"),
            ),
            (
                "//go:build linux && go1.50 || !(!darwin || !go1.60)",
                Some("go1.50"),
            ),
        ];
        for (input, want) in cases {
            let x = parse(input).unwrap();
            let got = go_version(&x);
            assert_eq!(got.as_deref(), *want, "GoVersion({:?})", input);
        }
    }
}
