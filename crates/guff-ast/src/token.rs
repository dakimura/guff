// Port of Go's go/token/token.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.

use std::collections::HashMap;
use std::fmt;
use std::sync::OnceLock;

/// `Token` is the set of lexical tokens of the Go programming language.
///
/// The discriminants mirror the `iota`-assigned constants in Go's
/// `go/token` package, including the internal `*_beg`/`*_end` sentinels
/// that delimit literal, operator, and keyword ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i32)]
pub enum Token {
    // Special tokens
    ILLEGAL = 0,
    EOF = 1,
    COMMENT = 2,

    LiteralBeg = 3,
    // Identifiers and basic type literals
    IDENT = 4,  // main
    INT = 5,    // 12345
    FLOAT = 6,  // 123.45
    IMAG = 7,   // 123.45i
    CHAR = 8,   // 'a'
    STRING = 9, // "abc"
    LiteralEnd = 10,

    OperatorBeg = 11,
    // Operators and delimiters
    ADD = 12, // +
    SUB = 13, // -
    MUL = 14, // *
    QUO = 15, // /
    REM = 16, // %

    AND = 17,    // &
    OR = 18,     // |
    XOR = 19,    // ^
    SHL = 20,    // <<
    SHR = 21,    // >>
    AndNot = 22, // &^

    AddAssign = 23, // +=
    SubAssign = 24, // -=
    MulAssign = 25, // *=
    QuoAssign = 26, // /=
    RemAssign = 27, // %=

    AndAssign = 28,    // &=
    OrAssign = 29,     // |=
    XorAssign = 30,    // ^=
    ShlAssign = 31,    // <<=
    ShrAssign = 32,    // >>=
    AndNotAssign = 33, // &^=

    LAND = 34,  // &&
    LOR = 35,   // ||
    ARROW = 36, // <-
    INC = 37,   // ++
    DEC = 38,   // --

    EQL = 39,    // ==
    LSS = 40,    // <
    GTR = 41,    // >
    ASSIGN = 42, // =
    NOT = 43,    // !

    NEQ = 44,      // !=
    LEQ = 45,      // <=
    GEQ = 46,      // >=
    DEFINE = 47,   // :=
    ELLIPSIS = 48, // ...

    LPAREN = 49, // (
    LBRACK = 50, // [
    LBRACE = 51, // {
    COMMA = 52,  // ,
    PERIOD = 53, // .

    RPAREN = 54,    // )
    RBRACK = 55,    // ]
    RBRACE = 56,    // }
    SEMICOLON = 57, // ;
    COLON = 58,     // :
    OperatorEnd = 59,

    KeywordBeg = 60,
    // Keywords
    BREAK = 61,
    CASE = 62,
    CHAN = 63,
    CONST = 64,
    CONTINUE = 65,

    DEFAULT = 66,
    DEFER = 67,
    ELSE = 68,
    FALLTHROUGH = 69,
    FOR = 70,

    FUNC = 71,
    GO = 72,
    GOTO = 73,
    IF = 74,
    IMPORT = 75,

    INTERFACE = 76,
    MAP = 77,
    PACKAGE = 78,
    RANGE = 79,
    RETURN = 80,

    SELECT = 81,
    STRUCT = 82,
    SWITCH = 83,
    TYPE = 84,
    VAR = 85,
    KeywordEnd = 86,

    AdditionalBeg = 87,
    TILDE = 88,
    AdditionalEnd = 89,
}

/// Number of distinct token discriminants (used to size the lookup table).
const TOKEN_COUNT: usize = 90;

impl Token {
    /// Convert an `i32` discriminant back to a `Token`. Returns `None` if
    /// the value is outside the known range.
    pub fn from_i32(v: i32) -> Option<Token> {
        if (0..TOKEN_COUNT as i32).contains(&v) {
            // SAFETY: discriminants are dense, sequential, and #[repr(i32)],
            // and we just verified `v` is within [0, TOKEN_COUNT).
            Some(unsafe { std::mem::transmute::<i32, Token>(v) })
        } else {
            None
        }
    }

    /// Integer discriminant of the token.
    pub fn as_i32(self) -> i32 {
        self as i32
    }

    /// String form of the token. For operators, delimiters, and keywords
    /// this is the token character sequence (e.g. `ADD` -> `"+"`); for all
    /// other tokens it's the constant name (e.g. `IDENT` -> `"IDENT"`).
    pub fn as_str(self) -> &'static str {
        // Fast path: the table covers every named token.
        if let Some(s) = TOKENS[self.as_i32() as usize] {
            return s;
        }
        ""
    }

    /// Operator precedence of the binary operator `op`. Non-operators
    /// return [`LOWEST_PREC`].
    pub fn precedence(self) -> i32 {
        match self {
            Token::LOR => 1,
            Token::LAND => 2,
            Token::EQL | Token::NEQ | Token::LSS | Token::LEQ | Token::GTR | Token::GEQ => 3,
            Token::ADD | Token::SUB | Token::OR | Token::XOR => 4,
            Token::MUL
            | Token::QUO
            | Token::REM
            | Token::SHL
            | Token::SHR
            | Token::AND
            | Token::AndNot => 5,
            _ => LOWEST_PREC,
        }
    }

    /// True for identifiers and basic-type literal tokens.
    pub fn is_literal(self) -> bool {
        let v = self.as_i32();
        Token::LiteralBeg.as_i32() < v && v < Token::LiteralEnd.as_i32()
    }

    /// True for operator and delimiter tokens (including `TILDE`).
    pub fn is_operator(self) -> bool {
        let v = self.as_i32();
        (Token::OperatorBeg.as_i32() < v && v < Token::OperatorEnd.as_i32()) || self == Token::TILDE
    }

    /// True for keyword tokens.
    pub fn is_keyword(self) -> bool {
        let v = self.as_i32();
        Token::KeywordBeg.as_i32() < v && v < Token::KeywordEnd.as_i32()
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = self.as_str();
        if s.is_empty() {
            write!(f, "token({})", self.as_i32())
        } else {
            f.write_str(s)
        }
    }
}

// Precedence constants matching Go's go/token package.
pub const LOWEST_PREC: i32 = 0;
pub const UNARY_PREC: i32 = 6;
pub const HIGHEST_PREC: i32 = 7;

/// Static table mapping token discriminant -> string representation.
/// `None` entries correspond to internal sentinels (`*_beg`, `*_end`).
static TOKENS: [Option<&'static str>; TOKEN_COUNT] = {
    let mut t: [Option<&'static str>; TOKEN_COUNT] = [None; TOKEN_COUNT];
    t[Token::ILLEGAL as usize] = Some("ILLEGAL");
    t[Token::EOF as usize] = Some("EOF");
    t[Token::COMMENT as usize] = Some("COMMENT");

    t[Token::IDENT as usize] = Some("IDENT");
    t[Token::INT as usize] = Some("INT");
    t[Token::FLOAT as usize] = Some("FLOAT");
    t[Token::IMAG as usize] = Some("IMAG");
    t[Token::CHAR as usize] = Some("CHAR");
    t[Token::STRING as usize] = Some("STRING");

    t[Token::ADD as usize] = Some("+");
    t[Token::SUB as usize] = Some("-");
    t[Token::MUL as usize] = Some("*");
    t[Token::QUO as usize] = Some("/");
    t[Token::REM as usize] = Some("%");

    t[Token::AND as usize] = Some("&");
    t[Token::OR as usize] = Some("|");
    t[Token::XOR as usize] = Some("^");
    t[Token::SHL as usize] = Some("<<");
    t[Token::SHR as usize] = Some(">>");
    t[Token::AndNot as usize] = Some("&^");

    t[Token::AddAssign as usize] = Some("+=");
    t[Token::SubAssign as usize] = Some("-=");
    t[Token::MulAssign as usize] = Some("*=");
    t[Token::QuoAssign as usize] = Some("/=");
    t[Token::RemAssign as usize] = Some("%=");

    t[Token::AndAssign as usize] = Some("&=");
    t[Token::OrAssign as usize] = Some("|=");
    t[Token::XorAssign as usize] = Some("^=");
    t[Token::ShlAssign as usize] = Some("<<=");
    t[Token::ShrAssign as usize] = Some(">>=");
    t[Token::AndNotAssign as usize] = Some("&^=");

    t[Token::LAND as usize] = Some("&&");
    t[Token::LOR as usize] = Some("||");
    t[Token::ARROW as usize] = Some("<-");
    t[Token::INC as usize] = Some("++");
    t[Token::DEC as usize] = Some("--");

    t[Token::EQL as usize] = Some("==");
    t[Token::LSS as usize] = Some("<");
    t[Token::GTR as usize] = Some(">");
    t[Token::ASSIGN as usize] = Some("=");
    t[Token::NOT as usize] = Some("!");

    t[Token::NEQ as usize] = Some("!=");
    t[Token::LEQ as usize] = Some("<=");
    t[Token::GEQ as usize] = Some(">=");
    t[Token::DEFINE as usize] = Some(":=");
    t[Token::ELLIPSIS as usize] = Some("...");

    t[Token::LPAREN as usize] = Some("(");
    t[Token::LBRACK as usize] = Some("[");
    t[Token::LBRACE as usize] = Some("{");
    t[Token::COMMA as usize] = Some(",");
    t[Token::PERIOD as usize] = Some(".");

    t[Token::RPAREN as usize] = Some(")");
    t[Token::RBRACK as usize] = Some("]");
    t[Token::RBRACE as usize] = Some("}");
    t[Token::SEMICOLON as usize] = Some(";");
    t[Token::COLON as usize] = Some(":");

    t[Token::BREAK as usize] = Some("break");
    t[Token::CASE as usize] = Some("case");
    t[Token::CHAN as usize] = Some("chan");
    t[Token::CONST as usize] = Some("const");
    t[Token::CONTINUE as usize] = Some("continue");

    t[Token::DEFAULT as usize] = Some("default");
    t[Token::DEFER as usize] = Some("defer");
    t[Token::ELSE as usize] = Some("else");
    t[Token::FALLTHROUGH as usize] = Some("fallthrough");
    t[Token::FOR as usize] = Some("for");

    t[Token::FUNC as usize] = Some("func");
    t[Token::GO as usize] = Some("go");
    t[Token::GOTO as usize] = Some("goto");
    t[Token::IF as usize] = Some("if");
    t[Token::IMPORT as usize] = Some("import");

    t[Token::INTERFACE as usize] = Some("interface");
    t[Token::MAP as usize] = Some("map");
    t[Token::PACKAGE as usize] = Some("package");
    t[Token::RANGE as usize] = Some("range");
    t[Token::RETURN as usize] = Some("return");

    t[Token::SELECT as usize] = Some("select");
    t[Token::STRUCT as usize] = Some("struct");
    t[Token::SWITCH as usize] = Some("switch");
    t[Token::TYPE as usize] = Some("type");
    t[Token::VAR as usize] = Some("var");

    t[Token::TILDE as usize] = Some("~");
    t
};

fn keywords() -> &'static HashMap<&'static str, Token> {
    static KEYWORDS: OnceLock<HashMap<&'static str, Token>> = OnceLock::new();
    KEYWORDS.get_or_init(|| {
        let mut m = HashMap::new();
        let begin = Token::KeywordBeg.as_i32() + 1;
        let end = Token::KeywordEnd.as_i32();
        for i in begin..end {
            let tok = Token::from_i32(i).expect("dense range");
            if let Some(s) = TOKENS[i as usize] {
                m.insert(s, tok);
            }
        }
        m
    })
}

/// Maps `ident` to its keyword token or [`Token::IDENT`] if it isn't a keyword.
pub fn lookup(ident: &str) -> Token {
    keywords().get(ident).copied().unwrap_or(Token::IDENT)
}

/// True iff `name` starts with an upper-case letter.
pub fn is_exported(name: &str) -> bool {
    match name.chars().next() {
        Some(ch) => ch.is_uppercase(),
        None => false,
    }
}

/// True iff `name` is a Go keyword (e.g. `"func"`, `"return"`).
pub fn is_keyword(name: &str) -> bool {
    keywords().contains_key(name)
}

/// True iff `name` is a Go identifier: a non-empty string of letters,
/// digits, and underscores, where the first character is not a digit.
/// Keywords are not identifiers.
pub fn is_identifier(name: &str) -> bool {
    if name.is_empty() || is_keyword(name) {
        return false;
    }
    for (i, c) in name.chars().enumerate() {
        let is_letter_like = c.is_alphabetic() || c == '_';
        let is_continuation_digit = i != 0 && c.is_numeric();
        if !is_letter_like && !is_continuation_digit {
            return false;
        }
    }
    true
}
