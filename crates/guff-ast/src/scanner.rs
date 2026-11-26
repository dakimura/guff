// Port of Go's go/scanner/scanner.go to Rust.
//
// Original: Copyright 2009 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// `Scanner` mirrors `go/scanner.Scanner`. The Go field `*token.File`
// becomes `Option<Arc<File>>`; the byte buffer is owned by the scanner
// (a clone of the source passed to `init`). The error handler is stored
// as `Option<Box<dyn FnMut(Position, &str) + 'eh>>`, which lets callers
// capture mutable state by reference using a per-scanner lifetime.
//
// `scannerhooks.StringEnd` from Go is exposed as a public accessor
// `Scanner::string_end()` rather than via a global side-channel.
//
// Note: Go returns literals as `string` (= `[]byte`), preserving any
// invalid UTF-8 in the source verbatim. This port returns `String`, so
// the invalid bytes are replaced with U+FFFD via `from_utf8_lossy`. The
// scanner still reports an `illegal UTF-8 encoding` error for those
// bytes, matching Go's behavior — only the literal's byte content
// differs.

use std::borrow::Cow;
use std::sync::Arc;

use crate::position::{File, Pos, Position, NO_POS};
use crate::token::{self, Token};

// -- Mode flags --------------------------------------------------------

/// Bitset controlling [`Scanner`] behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mode(pub u32);

impl Mode {
    pub const NONE: Mode = Mode(0);
    /// Equivalent to checking `self & other != 0`.
    pub fn contains(self, other: Mode) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for Mode {
    type Output = Mode;
    fn bitor(self, rhs: Mode) -> Mode {
        Mode(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for Mode {
    type Output = Mode;
    fn bitand(self, rhs: Mode) -> Mode {
        Mode(self.0 & rhs.0)
    }
}

/// Return comments as `COMMENT` tokens instead of skipping them.
pub const SCAN_COMMENTS: Mode = Mode(1 << 0);

/// Do not automatically insert semicolons (test-only in Go).
pub(crate) const DONT_INSERT_SEMIS: Mode = Mode(1 << 1);

// -- Constants ---------------------------------------------------------

const BOM: i32 = 0xFEFF;
const EOF: i32 = -1;
const RUNE_SELF: i32 = 0x80;
const RUNE_ERROR: i32 = 0xFFFD;
const MAX_RUNE: u32 = 0x0010_FFFF;

// -- Error handler type ------------------------------------------------

/// Closure invoked by the scanner on each syntax error. Lifetime `'eh`
/// lets the closure borrow non-`'static` state from the caller.
pub type ErrorHandler<'eh> = Box<dyn FnMut(Position, &str) + 'eh>;

// -- Scanner -----------------------------------------------------------

pub struct Scanner<'eh> {
    // immutable state (set by init)
    file: Option<Arc<File>>,
    dir: String,
    src: Vec<u8>,
    err: Option<ErrorHandler<'eh>>,
    mode: Mode,

    // scanning state
    ch: i32,
    offset: usize,
    rd_offset: usize,
    line_offset: usize,
    insert_semi: bool,
    nl_pos: Pos,
    string_end: Pos,

    /// Number of errors encountered (public — okay to read or reset).
    pub error_count: usize,
}

impl<'eh> Default for Scanner<'eh> {
    fn default() -> Self {
        Scanner {
            file: None,
            dir: String::new(),
            src: Vec::new(),
            err: None,
            mode: Mode::NONE,
            ch: ' ' as i32,
            offset: 0,
            rd_offset: 0,
            line_offset: 0,
            insert_semi: false,
            nl_pos: NO_POS,
            string_end: NO_POS,
            error_count: 0,
        }
    }
}

impl<'eh> Scanner<'eh> {
    /// Allocate a new, uninitialized scanner. Call [`Scanner::init`] before
    /// [`Scanner::scan`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Prepare the scanner to tokenize `src`. The file's [`File::size`]
    /// must equal `src.len()`.
    pub fn init(
        &mut self,
        file: Arc<File>,
        src: &[u8],
        err: Option<ErrorHandler<'eh>>,
        mode: Mode,
    ) {
        assert_eq!(
            file.size(),
            src.len() as i64,
            "file size ({}) does not match src len ({})",
            file.size(),
            src.len()
        );
        let (dir, _) = split_path(file.name());
        self.dir = dir.to_string();
        self.file = Some(file);
        self.src = src.to_vec();
        self.err = err;
        self.mode = mode;

        self.ch = ' ' as i32;
        self.offset = 0;
        self.rd_offset = 0;
        self.line_offset = 0;
        self.insert_semi = false;
        self.nl_pos = NO_POS;
        self.string_end = NO_POS;
        self.error_count = 0;

        self.next();
        if self.ch == BOM {
            self.next(); // ignore BOM at file beginning
        }
    }

    /// End position of the most recently scanned STRING token, or
    /// [`NO_POS`] if no string has been scanned yet.
    pub fn string_end(&self) -> Pos {
        self.string_end
    }

    /// Skip from the current character to the `}` that closes `depth` open
    /// braces, without building tokens. Used by [`crate::parser::SKIP_FUNC_BODIES`]:
    /// the seed only needs brace positions, and full tokenization of bodies is
    /// pure waste (identifiers, keyword lookup, literal allocation).
    ///
    /// Precondition: the opening `{` has already been consumed (scanner sits on
    /// the first character inside the block) and `depth >= 1`.
    ///
    /// On success, returns the [`Pos`] of the matching `}`, leaves the scanner
    /// ready to [`Scanner::scan`] the next token, and sets `insert_semi` as a
    /// normal `}` token would. Braces inside strings, runes and comments are
    /// ignored — same rules as token-based skipping. Newlines still update the
    /// [`File`] line table via [`Scanner::next`].
    pub(crate) fn skip_to_closing_brace(&mut self, mut depth: usize) -> Option<Pos> {
        debug_assert!(depth >= 1);
        // Pending newline-as-semicolon from a prior token must not fire mid-skip:
        // we are inside a block, not between statements the parser will see.
        self.nl_pos = NO_POS;
        loop {
            let ch = self.ch;
            if ch < 0 {
                return None;
            }
            let offs = self.offset;
            self.next();
            match ch {
                c if c == '{' as i32 => depth += 1,
                c if c == '}' as i32 => {
                    depth -= 1;
                    if depth == 0 {
                        let pos = self.file.as_ref().unwrap().pos(offs as i64);
                        self.insert_semi = true;
                        return Some(pos);
                    }
                }
                c if c == '"' as i32 => self.skip_string(),
                c if c == '\'' as i32 => self.skip_rune(),
                c if c == '`' as i32 => self.skip_raw_string(),
                c if c == '/' as i32 => {
                    if self.ch == '/' as i32 || self.ch == '*' as i32 {
                        self.skip_comment();
                    }
                }
                _ => {}
            }
        }
    }

    /// Like [`Scanner::scan_string`] but does not allocate the literal.
    fn skip_string(&mut self) {
        let offs = self.offset - 1;
        loop {
            let ch = self.ch;
            if ch == '\n' as i32 || ch < 0 {
                self.error(offs, "string literal not terminated");
                break;
            }
            self.next();
            if ch == '"' as i32 {
                break;
            }
            if ch == '\\' as i32 {
                self.scan_escape('"' as i32);
            }
        }
    }

    /// Like [`Scanner::scan_rune`] but does not allocate the literal.
    fn skip_rune(&mut self) {
        let offs = self.offset - 1;
        let mut valid = true;
        let mut n = 0usize;
        loop {
            let ch = self.ch;
            if ch == '\n' as i32 || ch < 0 {
                if valid {
                    self.error(offs, "rune literal not terminated");
                    valid = false;
                }
                break;
            }
            self.next();
            if ch == '\'' as i32 {
                break;
            }
            n += 1;
            if ch == '\\' as i32 && !self.scan_escape('\'' as i32) {
                valid = false;
            }
        }
        if valid && n != 1 {
            self.error(offs, "illegal rune literal");
        }
    }

    /// Like [`Scanner::scan_raw_string`] but does not allocate the literal.
    fn skip_raw_string(&mut self) {
        let offs = self.offset - 1;
        loop {
            let ch = self.ch;
            if ch < 0 {
                self.error(offs, "raw string literal not terminated");
                break;
            }
            self.next();
            if ch == '`' as i32 {
                break;
            }
        }
    }

    /// Like [`Scanner::scan_comment`] but does not allocate the comment text.
    fn skip_comment(&mut self) {
        if self.ch == '/' as i32 {
            self.next();
            while self.ch != '\n' as i32 && self.ch >= 0 {
                self.next();
            }
        } else {
            // /* … */
            self.next();
            while self.ch >= 0 {
                let ch = self.ch;
                self.next();
                if ch == '*' as i32 && self.ch == '/' as i32 {
                    self.next();
                    break;
                }
            }
        }
    }

    /// Read the next Unicode char into `self.ch`. `self.ch < 0` means EOF.
    fn next(&mut self) {
        if self.rd_offset < self.src.len() {
            self.offset = self.rd_offset;
            if self.ch == '\n' as i32 {
                self.line_offset = self.offset;
                let f = Arc::clone(self.file.as_ref().unwrap());
                f.add_line(self.offset as i64);
            }
            let b0 = self.src[self.rd_offset];
            let r: i32;
            let mut w: usize = 1;
            if b0 == 0 {
                self.error(self.offset, "illegal character NUL");
                r = 0;
            } else if (b0 as i32) >= RUNE_SELF {
                let (dr, dw) = decode_rune(&self.src[self.rd_offset..]);
                if dr == RUNE_ERROR && dw == 1 {
                    let rem = &self.src[self.rd_offset..];
                    if self.offset == 0
                        && rem.len() >= 2
                        && ((rem[0] == 0xFF && rem[1] == 0xFE)
                            || (rem[0] == 0xFE && rem[1] == 0xFF))
                    {
                        let to_consume = rem.len();
                        self.error(self.offset, "illegal UTF-8 encoding (got UTF-16)");
                        self.rd_offset += to_consume;
                    } else {
                        self.error(self.offset, "illegal UTF-8 encoding");
                    }
                } else if dr == BOM && self.offset > 0 {
                    self.error(self.offset, "illegal byte order mark");
                }
                r = dr;
                w = dw;
            } else {
                r = b0 as i32;
            }
            self.rd_offset += w;
            self.ch = r;
        } else {
            self.offset = self.src.len();
            if self.ch == '\n' as i32 {
                self.line_offset = self.offset;
                let f = Arc::clone(self.file.as_ref().unwrap());
                f.add_line(self.offset as i64);
            }
            self.ch = EOF;
        }
    }

    /// Byte following the most recently read character (0 at EOF).
    fn peek(&self) -> u8 {
        if self.rd_offset < self.src.len() {
            self.src[self.rd_offset]
        } else {
            0
        }
    }

    fn error(&mut self, offs: usize, msg: &str) {
        if self.err.is_some() {
            let file = Arc::clone(self.file.as_ref().unwrap());
            let pos = file.position(file.pos(offs as i64));
            (self.err.as_mut().unwrap())(pos, msg);
        }
        self.error_count += 1;
    }

    fn errorf(&mut self, offs: usize, args: std::fmt::Arguments<'_>) {
        let msg = std::fmt::format(args);
        self.error(offs, &msg);
    }

    // -- Comments ------------------------------------------------------

    fn scan_comment(&mut self) -> (String, usize) {
        let offs = self.offset - 1; // position of initial '/'
        let mut next: Option<usize> = None;
        let mut num_cr = 0usize;
        let mut nl_offset: usize = 0;

        if self.ch == '/' as i32 {
            // -- //-style comment --
            self.next();
            while self.ch != '\n' as i32 && self.ch >= 0 {
                if self.ch == '\r' as i32 {
                    num_cr += 1;
                }
                self.next();
            }
            let mut end = self.offset;
            if self.ch == '\n' as i32 {
                end += 1;
            }
            next = Some(end);
        } else {
            // -- /*…*/-style comment --
            self.next();
            while self.ch >= 0 {
                let ch = self.ch;
                if ch == '\r' as i32 {
                    num_cr += 1;
                } else if ch == '\n' as i32 && nl_offset == 0 {
                    nl_offset = self.offset;
                }
                self.next();
                if ch == '*' as i32 && self.ch == '/' as i32 {
                    self.next();
                    next = Some(self.offset);
                    break;
                }
            }
            if next.is_none() {
                self.error(offs, "comment not terminated");
            }
        }

        let mut lit: Vec<u8> = self.src[offs..self.offset].to_vec();

        // On Windows, //-comment lines may end in "\r\n".
        if num_cr > 0 && lit.len() >= 2 && lit[1] == b'/' && *lit.last().unwrap() == b'\r' {
            let n = lit.len();
            lit.truncate(n - 1);
            num_cr -= 1;
        }

        // Interpret line directives.
        if let Some(n) = next {
            let is_block = lit[1] == b'*';
            let at_line_start = is_block || offs == self.line_offset;
            if at_line_start && lit.len() >= 2 + PREFIX.len() && lit[2..2 + PREFIX.len()] == *PREFIX
            {
                self.update_line_info(n, offs, &lit);
            }
        }

        if num_cr > 0 {
            lit = strip_cr(&lit, lit[1] == b'*');
        }

        let s = String::from_utf8_lossy(&lit).into_owned();
        (s, nl_offset)
    }

    fn update_line_info(&mut self, next: usize, offs: usize, text: &[u8]) {
        // Strip the "//" or "/*" prefix and possibly the "*/" suffix.
        let mut text: Vec<u8> = if text[1] == b'*' {
            text[..text.len() - 2].to_vec()
        } else {
            text.to_vec()
        };
        // Strip "//line " or "/*line " (7 bytes).
        text.drain(0..2 + PREFIX.len()); // drop leading "//line " or "/*line "
        let offs = offs + 2 + PREFIX.len();

        let (mut i, n, ok) = trailing_digits(&text);
        if i == 0 {
            return;
        }

        if !ok {
            let msg = format!(
                "invalid line number: {}",
                String::from_utf8_lossy(&text[i..])
            );
            self.error(offs + i, &msg);
            return;
        }

        const MAX_LINE_COL: i64 = 1 << 30;
        let line: i64;
        let col: i64;

        // i >= 1; check for a second trailing-digit section ("line:col" form).
        let (i2, n2, ok2) = trailing_digits(&text[..i - 1]);
        if ok2 {
            let new_i = i2;
            let new_i2 = i;
            i = new_i;
            let i2 = new_i2;
            line = n2;
            col = n;
            if col == 0 || col > MAX_LINE_COL {
                let msg = format!(
                    "invalid column number: {}",
                    String::from_utf8_lossy(&text[i2..])
                );
                self.error(offs + i2, &msg);
                return;
            }
            text.truncate(i2 - 1);
        } else {
            line = n;
            col = 0;
        }

        if line == 0 || line > MAX_LINE_COL {
            let msg = format!(
                "invalid line number: {}",
                String::from_utf8_lossy(&text[i..])
            );
            self.error(offs + i, &msg);
            return;
        }

        let mut filename = String::from_utf8_lossy(&text[..i - 1]).into_owned();
        if filename.is_empty() && ok2 {
            let file = Arc::clone(self.file.as_ref().unwrap());
            filename = file.position(file.pos(offs as i64)).filename;
        } else if !filename.is_empty() {
            filename = clean_path(&filename);
            if !is_absolute(&filename) {
                filename = path_join(&self.dir, &filename);
            }
        }

        let _ = offs; // offs not needed after this point
        let file = Arc::clone(self.file.as_ref().unwrap());
        file.add_line_column_info(next as i64, &filename, line, col);
    }

    // -- Numbers -------------------------------------------------------

    /// Consume digits in the given base. Returns a bitset: bit 0 set iff a
    /// digit was seen, bit 1 set iff a `_` was seen. When `base <= 10` and
    /// `invalid` is `Some`, records the position of the first digit >= base.
    fn digits(&mut self, base: i32, invalid: &mut Option<usize>) -> i32 {
        let mut digsep = 0i32;
        if base <= 10 {
            let max = ('0' as i32) + base;
            while is_decimal(self.ch) || self.ch == '_' as i32 {
                let mut ds = 1;
                if self.ch == '_' as i32 {
                    ds = 2;
                } else if self.ch >= max && invalid.is_none() {
                    *invalid = Some(self.offset);
                }
                digsep |= ds;
                self.next();
            }
        } else {
            while is_hex(self.ch) || self.ch == '_' as i32 {
                let ds = if self.ch == '_' as i32 { 2 } else { 1 };
                digsep |= ds;
                self.next();
            }
        }
        digsep
    }

    fn scan_number(&mut self) -> (Token, String) {
        let offs = self.offset;
        let mut tok = Token::ILLEGAL;
        let mut base = 10i32;
        let mut prefix: i32 = 0; // 0 (decimal), '0' (0-octal), 'x', 'o', 'b'
        let mut digsep = 0i32;
        let mut invalid: Option<usize> = None;

        // integer part
        if self.ch != '.' as i32 {
            tok = Token::INT;
            if self.ch == '0' as i32 {
                self.next();
                match lower(self.ch) {
                    c if c == 'x' as i32 => {
                        self.next();
                        base = 16;
                        prefix = 'x' as i32;
                    }
                    c if c == 'o' as i32 => {
                        self.next();
                        base = 8;
                        prefix = 'o' as i32;
                    }
                    c if c == 'b' as i32 => {
                        self.next();
                        base = 2;
                        prefix = 'b' as i32;
                    }
                    _ => {
                        base = 8;
                        prefix = '0' as i32;
                        digsep = 1; // leading 0
                    }
                }
            }
            digsep |= self.digits(base, &mut invalid);
        }

        // fractional part
        if self.ch == '.' as i32 {
            tok = Token::FLOAT;
            if prefix == 'o' as i32 || prefix == 'b' as i32 {
                let name = litname(prefix);
                let off = self.offset;
                self.error(off, &format!("invalid radix point in {}", name));
            }
            self.next();
            digsep |= self.digits(base, &mut invalid);
        }

        if digsep & 1 == 0 {
            let off = self.offset;
            let name = litname(prefix);
            self.error(off, &format!("{} has no digits", name));
        }

        // exponent
        let e = lower(self.ch);
        if e == 'e' as i32 || e == 'p' as i32 {
            if e == 'e' as i32 && prefix != 0 && prefix != '0' as i32 {
                let ch = self.ch;
                let off = self.offset;
                self.errorf(
                    off,
                    format_args!("{} exponent requires decimal mantissa", quote_rune(ch)),
                );
            } else if e == 'p' as i32 && prefix != 'x' as i32 {
                let ch = self.ch;
                let off = self.offset;
                self.errorf(
                    off,
                    format_args!("{} exponent requires hexadecimal mantissa", quote_rune(ch)),
                );
            }
            self.next();
            tok = Token::FLOAT;
            if self.ch == '+' as i32 || self.ch == '-' as i32 {
                self.next();
            }
            let mut none = None;
            let ds = self.digits(10, &mut none);
            digsep |= ds;
            if ds & 1 == 0 {
                let off = self.offset;
                self.error(off, "exponent has no digits");
            }
        } else if prefix == 'x' as i32 && tok == Token::FLOAT {
            let off = self.offset;
            self.error(off, "hexadecimal mantissa requires a 'p' exponent");
        }

        // suffix 'i'
        if self.ch == 'i' as i32 {
            tok = Token::IMAG;
            self.next();
        }

        let lit = String::from_utf8_lossy(&self.src[offs..self.offset]).into_owned();
        if tok == Token::INT {
            if let Some(invalid) = invalid {
                let bad = lit.as_bytes()[invalid - offs] as char;
                let name = litname(prefix);
                self.errorf(invalid, format_args!("invalid digit '{}' in {}", bad, name));
            }
        }
        if digsep & 2 != 0 {
            if let Some(i) = invalid_sep(&lit) {
                self.error(offs + i, "'_' must separate successive digits");
            }
        }

        (tok, lit)
    }

    // -- Escape / rune / string ---------------------------------------

    fn scan_escape(&mut self, quote: i32) -> bool {
        let offs = self.offset;

        let (n, base, max): (usize, u32, u32) = match self.ch {
            c if c == 'a' as i32
                || c == 'b' as i32
                || c == 'f' as i32
                || c == 'n' as i32
                || c == 'r' as i32
                || c == 't' as i32
                || c == 'v' as i32
                || c == '\\' as i32
                || c == quote =>
            {
                self.next();
                return true;
            }
            c if c >= '0' as i32 && c <= '7' as i32 => (3, 8, 255),
            c if c == 'x' as i32 => {
                self.next();
                (2, 16, 255)
            }
            c if c == 'u' as i32 => {
                self.next();
                (4, 16, MAX_RUNE)
            }
            c if c == 'U' as i32 => {
                self.next();
                (8, 16, MAX_RUNE)
            }
            _ => {
                let msg = if self.ch < 0 {
                    "escape sequence not terminated".to_string()
                } else {
                    "unknown escape sequence".to_string()
                };
                self.error(offs, &msg);
                return false;
            }
        };

        let mut x: u32 = 0;
        let mut n = n;
        while n > 0 {
            let d = digit_val(self.ch) as u32;
            if d >= base {
                let ch = self.ch;
                let off = self.offset;
                let msg = if ch < 0 {
                    "escape sequence not terminated".to_string()
                } else {
                    format!(
                        "illegal character {} in escape sequence",
                        format_pound_u(ch)
                    )
                };
                self.error(off, &msg);
                return false;
            }
            x = x * base + d;
            self.next();
            n -= 1;
        }

        if x > max || (0xD800..0xE000).contains(&x) {
            self.error(offs, "escape sequence is invalid Unicode code point");
            return false;
        }

        true
    }

    fn scan_rune(&mut self) -> String {
        let offs = self.offset - 1; // '\'' already consumed
        let mut valid = true;
        let mut n = 0usize;
        loop {
            let ch = self.ch;
            if ch == '\n' as i32 || ch < 0 {
                if valid {
                    self.error(offs, "rune literal not terminated");
                    valid = false;
                }
                break;
            }
            self.next();
            if ch == '\'' as i32 {
                break;
            }
            n += 1;
            if ch == '\\' as i32 {
                if !self.scan_escape('\'' as i32) {
                    valid = false;
                }
            }
        }

        if valid && n != 1 {
            self.error(offs, "illegal rune literal");
        }

        String::from_utf8_lossy(&self.src[offs..self.offset]).into_owned()
    }

    fn scan_string(&mut self) -> String {
        let offs = self.offset - 1; // '"' already consumed
        loop {
            let ch = self.ch;
            if ch == '\n' as i32 || ch < 0 {
                self.error(offs, "string literal not terminated");
                break;
            }
            self.next();
            if ch == '"' as i32 {
                break;
            }
            if ch == '\\' as i32 {
                self.scan_escape('"' as i32);
            }
        }

        String::from_utf8_lossy(&self.src[offs..self.offset]).into_owned()
    }

    fn scan_raw_string(&mut self) -> (String, usize) {
        let offs = self.offset - 1; // '`' already consumed
        let mut has_cr = false;
        loop {
            let ch = self.ch;
            if ch < 0 {
                self.error(offs, "raw string literal not terminated");
                break;
            }
            self.next();
            if ch == '`' as i32 {
                break;
            }
            if ch == '\r' as i32 {
                has_cr = true;
            }
        }

        let lit_bytes = &self.src[offs..self.offset];
        let raw_len = lit_bytes.len();
        let lit = if has_cr {
            strip_cr(lit_bytes, false)
        } else {
            lit_bytes.to_vec()
        };
        (String::from_utf8_lossy(&lit).into_owned(), raw_len)
    }

    fn skip_whitespace(&mut self) {
        while self.ch == ' ' as i32
            || self.ch == '\t' as i32
            || (self.ch == '\n' as i32 && !self.insert_semi)
            || self.ch == '\r' as i32
        {
            self.next();
        }
    }

    // -- Identifier ---------------------------------------------------

    fn scan_identifier(&mut self) -> String {
        let offs = self.offset;
        while is_letter(self.ch) || is_digit(self.ch) {
            self.next();
        }
        String::from_utf8_lossy(&self.src[offs..self.offset]).into_owned()
    }

    // -- Multi-char operator helpers ----------------------------------

    fn switch2(&mut self, tok0: Token, tok1: Token) -> Token {
        if self.ch == '=' as i32 {
            self.next();
            tok1
        } else {
            tok0
        }
    }

    fn switch3(&mut self, tok0: Token, tok1: Token, ch2: i32, tok2: Token) -> Token {
        if self.ch == '=' as i32 {
            self.next();
            return tok1;
        }
        if self.ch == ch2 {
            self.next();
            return tok2;
        }
        tok0
    }

    fn switch4(&mut self, tok0: Token, tok1: Token, ch2: i32, tok2: Token, tok3: Token) -> Token {
        if self.ch == '=' as i32 {
            self.next();
            return tok1;
        }
        if self.ch == ch2 {
            self.next();
            if self.ch == '=' as i32 {
                self.next();
                return tok3;
            }
            return tok2;
        }
        tok0
    }

    // -- Scan ----------------------------------------------------------

    /// Scan the next token. Returns `(pos, token, literal)`. The end of
    /// source is signaled by `Token::EOF`.
    pub fn scan(&mut self) -> (Pos, Token, Cow<'static, str>) {
        loop {
            if self.nl_pos.is_valid() {
                let p = self.nl_pos;
                self.nl_pos = NO_POS;
                return (p, Token::SEMICOLON, Cow::Borrowed("\n"));
            }

            self.skip_whitespace();

            let pos = self.file.as_ref().unwrap().pos(self.offset as i64);

            let mut insert_semi = false;
            let tok;
            let mut lit: Cow<'static, str> = Cow::Borrowed("");

            let ch = self.ch;
            if is_letter(ch) {
                lit = Cow::Owned(self.scan_identifier());
                if lit.len() > 1 {
                    tok = token::lookup(lit.as_ref());
                    match tok {
                        Token::IDENT
                        | Token::BREAK
                        | Token::CONTINUE
                        | Token::FALLTHROUGH
                        | Token::RETURN => {
                            insert_semi = true;
                        }
                        _ => {}
                    }
                } else {
                    insert_semi = true;
                    tok = Token::IDENT;
                }
            } else if is_decimal(ch) || (ch == '.' as i32 && is_decimal(self.peek() as i32)) {
                insert_semi = true;
                let (t, l) = self.scan_number();
                tok = t;
                lit = Cow::Owned(l);
            } else {
                self.next(); // always make progress
                match ch {
                    EOF => {
                        if self.insert_semi {
                            self.insert_semi = false;
                            return (pos, Token::SEMICOLON, Cow::Borrowed("\n"));
                        }
                        tok = Token::EOF;
                    }
                    c if c == '\n' as i32 => {
                        // Only reachable when insert_semi was set.
                        self.insert_semi = false;
                        return (pos, Token::SEMICOLON, Cow::Borrowed("\n"));
                    }
                    c if c == '"' as i32 => {
                        insert_semi = true;
                        tok = Token::STRING;
                        lit = Cow::Owned(self.scan_string());
                        self.string_end = Pos(pos.0 + lit.len() as i64);
                    }
                    c if c == '\'' as i32 => {
                        insert_semi = true;
                        tok = Token::CHAR;
                        lit = Cow::Owned(self.scan_rune());
                    }
                    c if c == '`' as i32 => {
                        insert_semi = true;
                        tok = Token::STRING;
                        let (l, raw_len) = self.scan_raw_string();
                        lit = Cow::Owned(l);
                        self.string_end = Pos(pos.0 + raw_len as i64);
                    }
                    c if c == ':' as i32 => {
                        tok = self.switch2(Token::COLON, Token::DEFINE);
                    }
                    c if c == '.' as i32 => {
                        tok = if self.ch == '.' as i32 && self.peek() == b'.' {
                            self.next();
                            self.next();
                            Token::ELLIPSIS
                        } else {
                            Token::PERIOD
                        };
                    }
                    c if c == ',' as i32 => tok = Token::COMMA,
                    c if c == ';' as i32 => {
                        tok = Token::SEMICOLON;
                        lit = Cow::Borrowed(";");
                    }
                    c if c == '(' as i32 => tok = Token::LPAREN,
                    c if c == ')' as i32 => {
                        insert_semi = true;
                        tok = Token::RPAREN;
                    }
                    c if c == '[' as i32 => tok = Token::LBRACK,
                    c if c == ']' as i32 => {
                        insert_semi = true;
                        tok = Token::RBRACK;
                    }
                    c if c == '{' as i32 => tok = Token::LBRACE,
                    c if c == '}' as i32 => {
                        insert_semi = true;
                        tok = Token::RBRACE;
                    }
                    c if c == '+' as i32 => {
                        tok = self.switch3(Token::ADD, Token::AddAssign, '+' as i32, Token::INC);
                        if tok == Token::INC {
                            insert_semi = true;
                        }
                    }
                    c if c == '-' as i32 => {
                        tok = self.switch3(Token::SUB, Token::SubAssign, '-' as i32, Token::DEC);
                        if tok == Token::DEC {
                            insert_semi = true;
                        }
                    }
                    c if c == '*' as i32 => {
                        tok = self.switch2(Token::MUL, Token::MulAssign);
                    }
                    c if c == '/' as i32 => {
                        if self.ch == '/' as i32 || self.ch == '*' as i32 {
                            let (comment, nl_offset) = self.scan_comment();
                            if self.insert_semi && nl_offset != 0 {
                                let f = Arc::clone(self.file.as_ref().unwrap());
                                self.nl_pos = f.pos(nl_offset as i64);
                                self.insert_semi = false;
                            } else {
                                insert_semi = self.insert_semi;
                            }
                            if !self.mode.contains(SCAN_COMMENTS) {
                                continue;
                            }
                            tok = Token::COMMENT;
                            lit = Cow::Owned(comment);
                        } else {
                            tok = self.switch2(Token::QUO, Token::QuoAssign);
                        }
                    }
                    c if c == '%' as i32 => {
                        tok = self.switch2(Token::REM, Token::RemAssign);
                    }
                    c if c == '^' as i32 => {
                        tok = self.switch2(Token::XOR, Token::XorAssign);
                    }
                    c if c == '<' as i32 => {
                        if self.ch == '-' as i32 {
                            self.next();
                            tok = Token::ARROW;
                        } else {
                            tok = self.switch4(
                                Token::LSS,
                                Token::LEQ,
                                '<' as i32,
                                Token::SHL,
                                Token::ShlAssign,
                            );
                        }
                    }
                    c if c == '>' as i32 => {
                        tok = self.switch4(
                            Token::GTR,
                            Token::GEQ,
                            '>' as i32,
                            Token::SHR,
                            Token::ShrAssign,
                        );
                    }
                    c if c == '=' as i32 => {
                        tok = self.switch2(Token::ASSIGN, Token::EQL);
                    }
                    c if c == '!' as i32 => {
                        tok = self.switch2(Token::NOT, Token::NEQ);
                    }
                    c if c == '&' as i32 => {
                        if self.ch == '^' as i32 {
                            self.next();
                            tok = self.switch2(Token::AndNot, Token::AndNotAssign);
                        } else {
                            tok =
                                self.switch3(Token::AND, Token::AndAssign, '&' as i32, Token::LAND);
                        }
                    }
                    c if c == '|' as i32 => {
                        tok = self.switch3(Token::OR, Token::OrAssign, '|' as i32, Token::LOR);
                    }
                    c if c == '~' as i32 => tok = Token::TILDE,
                    _ => {
                        if ch != BOM {
                            let off = self.file.as_ref().unwrap().offset(pos) as usize;
                            if ch == '\u{201C}' as i32 || ch == '\u{201D}' as i32 {
                                self.errorf(
                                    off,
                                    format_args!(
                                        "curly quotation mark {} (use neutral {})",
                                        quote_rune(ch),
                                        "'\"'"
                                    ),
                                );
                            } else {
                                self.errorf(
                                    off,
                                    format_args!("illegal character {}", format_pound_u(ch)),
                                );
                            }
                        }
                        insert_semi = self.insert_semi;
                        tok = Token::ILLEGAL;
                        lit = match char::from_u32(ch as u32) {
                            Some(c) => Cow::Owned(c.to_string()),
                            None => Cow::Borrowed(""),
                        };
                    }
                }
            }

            if !self.mode.contains(DONT_INSERT_SEMIS) {
                self.insert_semi = insert_semi;
            }

            return (pos, tok, lit);
        }
    }
}

// -- Module-private helpers -------------------------------------------

static PREFIX: &[u8] = b"line ";

/// Returns the largest `i` with all bytes `<= ch` in some order. Used by
/// the line-directive parser: locate the last `:` in `text`, parse the
/// suffix as an integer.
fn trailing_digits(text: &[u8]) -> (usize, i64, bool) {
    let i = match text.iter().rposition(|&b| b == b':') {
        Some(i) => i,
        None => return (0, 0, false),
    };
    let suffix = match std::str::from_utf8(&text[i + 1..]) {
        Ok(s) => s,
        Err(_) => return (i + 1, 0, false),
    };
    match suffix.parse::<u64>() {
        Ok(n) => (i + 1, n as i64, true),
        Err(_) => (i + 1, 0, false),
    }
}

fn lower(ch: i32) -> i32 {
    (('a' as i32 - 'A' as i32) | ch) as i32
}

fn is_decimal(ch: i32) -> bool {
    ch >= '0' as i32 && ch <= '9' as i32
}

fn is_hex(ch: i32) -> bool {
    is_decimal(ch) || ('a' as i32 <= lower(ch) && lower(ch) <= 'f' as i32)
}

fn digit_val(ch: i32) -> i32 {
    if is_decimal(ch) {
        ch - '0' as i32
    } else if 'a' as i32 <= lower(ch) && lower(ch) <= 'f' as i32 {
        lower(ch) - 'a' as i32 + 10
    } else {
        16 // larger than any legal digit val
    }
}

fn is_letter(ch: i32) -> bool {
    if ch < 0 {
        return false;
    }
    let lo = lower(ch);
    if ('a' as i32 <= lo && lo <= 'z' as i32) || ch == '_' as i32 {
        return true;
    }
    if ch >= RUNE_SELF {
        if let Some(c) = char::from_u32(ch as u32) {
            return c.is_alphabetic();
        }
    }
    false
}

fn is_digit(ch: i32) -> bool {
    if ch < 0 {
        return false;
    }
    if is_decimal(ch) {
        return true;
    }
    if ch >= RUNE_SELF {
        if let Some(c) = char::from_u32(ch as u32) {
            return c.is_numeric();
        }
    }
    false
}

fn litname(prefix: i32) -> &'static str {
    match char::from_u32(prefix as u32) {
        Some('x') => "hexadecimal literal",
        Some('o') | Some('0') => "octal literal",
        Some('b') => "binary literal",
        _ => "decimal literal",
    }
}

/// Returns the index of the first invalid `_` separator in `x`, or `None`.
pub(crate) fn invalid_sep(x: &str) -> Option<usize> {
    let bytes = x.as_bytes();
    let mut x1: u8 = b' '; // prefix flag, only care if it's 'x'
    let mut d: u8 = b'.'; // digit class: '_', '0' (digit), '.' (other)
    let mut i = 0usize;

    if bytes.len() >= 2 && bytes[0] == b'0' {
        x1 = lower(bytes[1] as i32) as u8;
        if x1 == b'x' || x1 == b'o' || x1 == b'b' {
            d = b'0';
            i = 2;
        }
    }

    while i < bytes.len() {
        let p = d;
        d = bytes[i];
        let is_dec = d.is_ascii_digit();
        let is_hex_x = x1 == b'x' && (is_dec || matches!(lower(d as i32) as u8, b'a'..=b'f'));
        if d == b'_' {
            if p != b'0' {
                return Some(i);
            }
        } else if is_dec || is_hex_x {
            d = b'0';
        } else {
            if p == b'_' {
                return Some(i - 1);
            }
            d = b'.';
        }
        i += 1;
    }
    if d == b'_' {
        return Some(bytes.len() - 1);
    }
    None
}

/// Strip '\r' bytes from `b`, preserving the careful "*\r/" exception for
/// `/*…*/` comments.
pub(crate) fn strip_cr(b: &[u8], comment: bool) -> Vec<u8> {
    let mut c = vec![0u8; b.len()];
    let mut i = 0usize;
    for (j, &ch) in b.iter().enumerate() {
        // In a /*-style comment, don't strip '\r' from "*\r/" sequences (issue
        // #11151), unless the '\r' is immediately after the opening "/*".
        if ch != b'\r'
            || (comment
                && i > 2 // len("/*")
                && c[i - 1] == b'*'
                && j + 1 < b.len()
                && b[j + 1] == b'/')
        {
            c[i] = ch;
            i += 1;
        }
    }
    c.truncate(i);
    c
}

/// Minimal UTF-8 decoder: returns `(rune, bytes_consumed)`. On invalid
/// input returns `(RUNE_ERROR, 1)` matching Go's `utf8.DecodeRune`.
fn decode_rune(b: &[u8]) -> (i32, usize) {
    if b.is_empty() {
        return (RUNE_ERROR, 0);
    }
    let b0 = b[0];
    if b0 < 0x80 {
        return (b0 as i32, 1);
    }
    let (need, mut r): (usize, u32) = match b0 {
        0xC2..=0xDF => (2, (b0 & 0x1F) as u32),
        0xE0..=0xEF => (3, (b0 & 0x0F) as u32),
        0xF0..=0xF4 => (4, (b0 & 0x07) as u32),
        _ => return (RUNE_ERROR, 1),
    };
    if b.len() < need {
        return (RUNE_ERROR, 1);
    }
    for &bx in &b[1..need] {
        if bx & 0xC0 != 0x80 {
            return (RUNE_ERROR, 1);
        }
        r = (r << 6) | (bx & 0x3F) as u32;
    }
    let min = match need {
        2 => 0x80,
        3 => 0x800,
        4 => 0x10000,
        _ => 0,
    };
    if r < min || r > MAX_RUNE || (0xD800..=0xDFFF).contains(&r) {
        return (RUNE_ERROR, 1);
    }
    (r as i32, need)
}

/// Format a rune as Go's `%#U` does: `U+XXXX` plus, when printable,
/// `' '` containing the character itself.
fn format_pound_u(r: i32) -> String {
    let u = r as u32;
    let hex = format!("U+{:04X}", u);
    if let Some(c) = char::from_u32(u) {
        if is_printable_for_diag(c) {
            return format!("{} '{}'", hex, c);
        }
    }
    hex
}

fn is_printable_for_diag(c: char) -> bool {
    !c.is_control()
}

/// Render a rune as a single-quoted literal — matches Go's `%q` for the
/// scanner's diagnostic uses (no surrogate handling needed).
fn quote_rune(r: i32) -> String {
    match char::from_u32(r as u32) {
        Some(c) => format!("'{}'", c),
        None => format!("'{}'", r),
    }
}

// -- Tiny Unix-only path helpers (mirrors filepath.{Split,Clean,Join}) --

fn split_path(s: &str) -> (&str, &str) {
    match s.rfind('/') {
        Some(i) => (&s[..i + 1], &s[i + 1..]),
        None => ("", s),
    }
}

fn is_absolute(p: &str) -> bool {
    p.starts_with('/')
}

fn path_join(dir: &str, p: &str) -> String {
    let cleaned = if dir.is_empty() {
        p.to_string()
    } else if dir.ends_with('/') {
        format!("{}{}", dir, p)
    } else {
        format!("{}/{}", dir, p)
    };
    clean_path(&cleaned)
}

fn clean_path(p: &str) -> String {
    if p.is_empty() {
        return ".".to_string();
    }
    let absolute = p.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                if let Some(last) = out.last() {
                    if *last != ".." {
                        out.pop();
                        continue;
                    }
                }
                if !absolute {
                    out.push("..");
                }
            }
            other => out.push(other),
        }
    }
    let joined = out.join("/");
    if absolute {
        format!("/{}", joined)
    } else if joined.is_empty() {
        ".".to_string()
    } else {
        joined
    }
}

// ====================================================================
// Tests — port of go/scanner/scanner_test.go and example_test.go.
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::position::FileSet;

    fn fset() -> Arc<FileSet> {
        FileSet::new()
    }

    // ---- token class -------------------------------------------------

    const SPECIAL: i32 = 0;
    const LITERAL: i32 = 1;
    const OPERATOR: i32 = 2;
    const KEYWORD: i32 = 3;

    fn tokenclass(tok: Token) -> i32 {
        if tok.is_literal() {
            LITERAL
        } else if tok.is_operator() {
            OPERATOR
        } else if tok.is_keyword() {
            KEYWORD
        } else {
            SPECIAL
        }
    }

    struct Elt {
        tok: Token,
        lit: &'static str,
        class: i32,
    }

    fn tokens_table() -> Vec<Elt> {
        use Token::*;
        let raw_string_multiline: &'static str = "`foo\n\t\t                        bar`";
        vec![
            // Special tokens
            Elt {
                tok: COMMENT,
                lit: "/* a comment */",
                class: SPECIAL,
            },
            Elt {
                tok: COMMENT,
                lit: "// a comment \n",
                class: SPECIAL,
            },
            Elt {
                tok: COMMENT,
                lit: "/*\r*/",
                class: SPECIAL,
            },
            Elt {
                tok: COMMENT,
                lit: "/**\r/*/",
                class: SPECIAL,
            },
            Elt {
                tok: COMMENT,
                lit: "/**\r\r/*/",
                class: SPECIAL,
            },
            Elt {
                tok: COMMENT,
                lit: "//\r\n",
                class: SPECIAL,
            },
            // Identifiers and basic type literals
            Elt {
                tok: IDENT,
                lit: "foobar",
                class: LITERAL,
            },
            Elt {
                tok: IDENT,
                lit: "a۰۱۸",
                class: LITERAL,
            },
            Elt {
                tok: IDENT,
                lit: "foo६४",
                class: LITERAL,
            },
            Elt {
                tok: IDENT,
                lit: "bar９８７６",
                class: LITERAL,
            },
            Elt {
                tok: IDENT,
                lit: "ŝ",
                class: LITERAL,
            },
            Elt {
                tok: IDENT,
                lit: "ŝfoo",
                class: LITERAL,
            },
            Elt {
                tok: INT,
                lit: "0",
                class: LITERAL,
            },
            Elt {
                tok: INT,
                lit: "1",
                class: LITERAL,
            },
            Elt {
                tok: INT,
                lit: "123456789012345678890",
                class: LITERAL,
            },
            Elt {
                tok: INT,
                lit: "01234567",
                class: LITERAL,
            },
            Elt {
                tok: INT,
                lit: "0xcafebabe",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "0.",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: ".0",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "3.14159265",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "1e0",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "1e+100",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "1e-100",
                class: LITERAL,
            },
            Elt {
                tok: FLOAT,
                lit: "2.71828e-1000",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "0i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "1i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "012345678901234567889i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "123456789012345678890i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "0.i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: ".0i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "3.14159265i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "1e0i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "1e+100i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "1e-100i",
                class: LITERAL,
            },
            Elt {
                tok: IMAG,
                lit: "2.71828e-1000i",
                class: LITERAL,
            },
            Elt {
                tok: CHAR,
                lit: "'a'",
                class: LITERAL,
            },
            Elt {
                tok: CHAR,
                lit: "'\\000'",
                class: LITERAL,
            },
            Elt {
                tok: CHAR,
                lit: "'\\xFF'",
                class: LITERAL,
            },
            Elt {
                tok: CHAR,
                lit: "'\\uff16'",
                class: LITERAL,
            },
            Elt {
                tok: CHAR,
                lit: "'\\U0000ff16'",
                class: LITERAL,
            },
            Elt {
                tok: STRING,
                lit: "`foobar`",
                class: LITERAL,
            },
            Elt {
                tok: STRING,
                lit: raw_string_multiline,
                class: LITERAL,
            },
            Elt {
                tok: STRING,
                lit: "`\r`",
                class: LITERAL,
            },
            Elt {
                tok: STRING,
                lit: "`foo\r\nbar`",
                class: LITERAL,
            },
            // Operators and delimiters
            Elt {
                tok: ADD,
                lit: "+",
                class: OPERATOR,
            },
            Elt {
                tok: SUB,
                lit: "-",
                class: OPERATOR,
            },
            Elt {
                tok: MUL,
                lit: "*",
                class: OPERATOR,
            },
            Elt {
                tok: QUO,
                lit: "/",
                class: OPERATOR,
            },
            Elt {
                tok: REM,
                lit: "%",
                class: OPERATOR,
            },
            Elt {
                tok: AND,
                lit: "&",
                class: OPERATOR,
            },
            Elt {
                tok: OR,
                lit: "|",
                class: OPERATOR,
            },
            Elt {
                tok: XOR,
                lit: "^",
                class: OPERATOR,
            },
            Elt {
                tok: SHL,
                lit: "<<",
                class: OPERATOR,
            },
            Elt {
                tok: SHR,
                lit: ">>",
                class: OPERATOR,
            },
            Elt {
                tok: AndNot,
                lit: "&^",
                class: OPERATOR,
            },
            Elt {
                tok: AddAssign,
                lit: "+=",
                class: OPERATOR,
            },
            Elt {
                tok: SubAssign,
                lit: "-=",
                class: OPERATOR,
            },
            Elt {
                tok: MulAssign,
                lit: "*=",
                class: OPERATOR,
            },
            Elt {
                tok: QuoAssign,
                lit: "/=",
                class: OPERATOR,
            },
            Elt {
                tok: RemAssign,
                lit: "%=",
                class: OPERATOR,
            },
            Elt {
                tok: AndAssign,
                lit: "&=",
                class: OPERATOR,
            },
            Elt {
                tok: OrAssign,
                lit: "|=",
                class: OPERATOR,
            },
            Elt {
                tok: XorAssign,
                lit: "^=",
                class: OPERATOR,
            },
            Elt {
                tok: ShlAssign,
                lit: "<<=",
                class: OPERATOR,
            },
            Elt {
                tok: ShrAssign,
                lit: ">>=",
                class: OPERATOR,
            },
            Elt {
                tok: AndNotAssign,
                lit: "&^=",
                class: OPERATOR,
            },
            Elt {
                tok: LAND,
                lit: "&&",
                class: OPERATOR,
            },
            Elt {
                tok: LOR,
                lit: "||",
                class: OPERATOR,
            },
            Elt {
                tok: ARROW,
                lit: "<-",
                class: OPERATOR,
            },
            Elt {
                tok: INC,
                lit: "++",
                class: OPERATOR,
            },
            Elt {
                tok: DEC,
                lit: "--",
                class: OPERATOR,
            },
            Elt {
                tok: EQL,
                lit: "==",
                class: OPERATOR,
            },
            Elt {
                tok: LSS,
                lit: "<",
                class: OPERATOR,
            },
            Elt {
                tok: GTR,
                lit: ">",
                class: OPERATOR,
            },
            Elt {
                tok: ASSIGN,
                lit: "=",
                class: OPERATOR,
            },
            Elt {
                tok: NOT,
                lit: "!",
                class: OPERATOR,
            },
            Elt {
                tok: NEQ,
                lit: "!=",
                class: OPERATOR,
            },
            Elt {
                tok: LEQ,
                lit: "<=",
                class: OPERATOR,
            },
            Elt {
                tok: GEQ,
                lit: ">=",
                class: OPERATOR,
            },
            Elt {
                tok: DEFINE,
                lit: ":=",
                class: OPERATOR,
            },
            Elt {
                tok: ELLIPSIS,
                lit: "...",
                class: OPERATOR,
            },
            Elt {
                tok: LPAREN,
                lit: "(",
                class: OPERATOR,
            },
            Elt {
                tok: LBRACK,
                lit: "[",
                class: OPERATOR,
            },
            Elt {
                tok: LBRACE,
                lit: "{",
                class: OPERATOR,
            },
            Elt {
                tok: COMMA,
                lit: ",",
                class: OPERATOR,
            },
            Elt {
                tok: PERIOD,
                lit: ".",
                class: OPERATOR,
            },
            Elt {
                tok: RPAREN,
                lit: ")",
                class: OPERATOR,
            },
            Elt {
                tok: RBRACK,
                lit: "]",
                class: OPERATOR,
            },
            Elt {
                tok: RBRACE,
                lit: "}",
                class: OPERATOR,
            },
            Elt {
                tok: SEMICOLON,
                lit: ";",
                class: OPERATOR,
            },
            Elt {
                tok: COLON,
                lit: ":",
                class: OPERATOR,
            },
            Elt {
                tok: TILDE,
                lit: "~",
                class: OPERATOR,
            },
            // Keywords
            Elt {
                tok: BREAK,
                lit: "break",
                class: KEYWORD,
            },
            Elt {
                tok: CASE,
                lit: "case",
                class: KEYWORD,
            },
            Elt {
                tok: CHAN,
                lit: "chan",
                class: KEYWORD,
            },
            Elt {
                tok: CONST,
                lit: "const",
                class: KEYWORD,
            },
            Elt {
                tok: CONTINUE,
                lit: "continue",
                class: KEYWORD,
            },
            Elt {
                tok: DEFAULT,
                lit: "default",
                class: KEYWORD,
            },
            Elt {
                tok: DEFER,
                lit: "defer",
                class: KEYWORD,
            },
            Elt {
                tok: ELSE,
                lit: "else",
                class: KEYWORD,
            },
            Elt {
                tok: FALLTHROUGH,
                lit: "fallthrough",
                class: KEYWORD,
            },
            Elt {
                tok: FOR,
                lit: "for",
                class: KEYWORD,
            },
            Elt {
                tok: FUNC,
                lit: "func",
                class: KEYWORD,
            },
            Elt {
                tok: GO,
                lit: "go",
                class: KEYWORD,
            },
            Elt {
                tok: GOTO,
                lit: "goto",
                class: KEYWORD,
            },
            Elt {
                tok: IF,
                lit: "if",
                class: KEYWORD,
            },
            Elt {
                tok: IMPORT,
                lit: "import",
                class: KEYWORD,
            },
            Elt {
                tok: INTERFACE,
                lit: "interface",
                class: KEYWORD,
            },
            Elt {
                tok: MAP,
                lit: "map",
                class: KEYWORD,
            },
            Elt {
                tok: PACKAGE,
                lit: "package",
                class: KEYWORD,
            },
            Elt {
                tok: RANGE,
                lit: "range",
                class: KEYWORD,
            },
            Elt {
                tok: RETURN,
                lit: "return",
                class: KEYWORD,
            },
            Elt {
                tok: SELECT,
                lit: "select",
                class: KEYWORD,
            },
            Elt {
                tok: STRUCT,
                lit: "struct",
                class: KEYWORD,
            },
            Elt {
                tok: SWITCH,
                lit: "switch",
                class: KEYWORD,
            },
            Elt {
                tok: TYPE,
                lit: "type",
                class: KEYWORD,
            },
            Elt {
                tok: VAR,
                lit: "var",
                class: KEYWORD,
            },
        ]
    }

    const WHITESPACE: &str = "  \t  \n\n\n";

    fn build_source() -> Vec<u8> {
        let mut src = Vec::new();
        for t in tokens_table() {
            src.extend_from_slice(t.lit.as_bytes());
            src.extend_from_slice(WHITESPACE.as_bytes());
        }
        src
    }

    fn newline_count(s: &[u8]) -> i64 {
        s.iter().filter(|&&b| b == b'\n').count() as i64
    }

    fn check_pos(lit: &str, fset: &FileSet, p: Pos, expected: &Position) {
        let pos = fset.position(p);
        let pos_clean = clean_path(&pos.filename);
        let exp_clean = clean_path(&expected.filename);
        assert!(
            pos.filename == expected.filename || pos_clean == exp_clean,
            "bad filename for {:?}: got {}, expected {}",
            lit,
            pos.filename,
            expected.filename
        );
        assert_eq!(
            pos.offset, expected.offset,
            "bad offset for {:?}: got {}, expected {}",
            lit, pos.offset, expected.offset
        );
        assert_eq!(
            pos.line, expected.line,
            "bad line for {:?}: got {}, expected {}",
            lit, pos.line, expected.line
        );
        assert_eq!(
            pos.column, expected.column,
            "bad column for {:?}: got {}, expected {}",
            lit, pos.column, expected.column
        );
    }

    // ---- TestScan ---------------------------------------------------

    #[test]
    fn test_scan() {
        let source = build_source();
        let whitespace_linecount = newline_count(WHITESPACE.as_bytes());

        let fset = fset();
        let file = fset.add_file("", fset.base(), source.len() as i64);

        let mut s: Scanner<'_> = Scanner::new();
        let eh: ErrorHandler<'_> =
            Box::new(|_pos: Position, msg: &str| panic!("error handler called (msg = {})", msg));
        s.init(file, &source, Some(eh), SCAN_COMMENTS | DONT_INSERT_SEMIS);

        let table = tokens_table();
        let mut epos = Position {
            filename: String::new(),
            offset: 0,
            line: 1,
            column: 1,
        };

        let mut index = 0usize;
        loop {
            let (pos, tok, lit) = s.scan();
            if tok == Token::EOF {
                epos.line = newline_count(&source);
                epos.column = 2;
            }
            check_pos(&lit, &fset, pos, &epos);

            // Expected element
            let (etok, elit, eclass) = if index < table.len() {
                let e = &table[index];
                index += 1;
                (e.tok, e.lit, e.class)
            } else {
                (Token::EOF, "", SPECIAL)
            };
            assert_eq!(tok, etok, "bad token for {:?}: got {:?}", lit, tok);
            assert_eq!(
                tokenclass(tok),
                eclass,
                "bad class for {:?}: got {}",
                lit,
                tokenclass(tok)
            );

            // Expected literal
            let expected_lit = match etok {
                Token::COMMENT => {
                    let stripped = strip_cr(elit.as_bytes(), elit.as_bytes()[1] == b'*');
                    let mut s = String::from_utf8(stripped).unwrap();
                    if s.as_bytes()[1] == b'/' {
                        s.pop(); // remove trailing '\n' from //-comments
                    }
                    s
                }
                Token::IDENT => elit.to_string(),
                Token::SEMICOLON => ";".to_string(),
                t if t.is_literal() => {
                    let raw = elit.as_bytes();
                    if raw[0] == b'`' {
                        String::from_utf8(strip_cr(raw, false)).unwrap()
                    } else {
                        elit.to_string()
                    }
                }
                t if t.is_keyword() => elit.to_string(),
                _ => String::new(),
            };
            assert_eq!(
                lit, expected_lit,
                "bad literal for {:?}: got {:?}, expected {:?}",
                lit, lit, expected_lit
            );

            if tok == Token::EOF {
                break;
            }

            epos.offset += (elit.len() + WHITESPACE.len()) as i64;
            epos.line += newline_count(elit.as_bytes()) + whitespace_linecount;
        }

        assert_eq!(s.error_count, 0, "found {} errors", s.error_count);
    }

    // ---- TestStripCR -------------------------------------------------

    #[test]
    fn test_strip_cr() {
        let cases = [
            ("//\n", "//\n"),
            ("//\r\n", "//\n"),
            ("//\r\r\r\n", "//\n"),
            ("//\r*\r/\r\n", "//*/\n"),
            ("/**/", "/**/"),
            ("/*\r/*/", "/*/*/"),
            ("/*\r*/", "/**/"),
            ("/**\r/*/", "/**\r/*/"),
            ("/*\r/\r*\r/*/", "/*/*\r/*/"),
            ("/*\r\r\r\r*/", "/**/"),
        ];
        for (have, want) in cases {
            let comment = have.as_bytes().len() >= 2 && have.as_bytes()[1] == b'*';
            let got = String::from_utf8(strip_cr(have.as_bytes(), comment)).unwrap();
            assert_eq!(got, want, "strip_cr({:?})", have);
        }
    }

    // ---- TestSemicolons ----------------------------------------------

    fn check_semi(input: &str, want: &str, mode: Mode) {
        let want = if !mode.contains(SCAN_COMMENTS) {
            want.replace("COMMENT ", "")
                .replace(" COMMENT", "")
                .replace("COMMENT", "")
        } else {
            want.to_string()
        };

        let fset = fset();
        let file = fset.add_file("TestSemis", fset.base(), input.len() as i64);
        let mut s: Scanner<'_> = Scanner::new();
        s.init(file.clone(), input.as_bytes(), None, mode);

        let mut tokens: Vec<String> = Vec::new();
        loop {
            let (pos, tok, lit) = s.scan();
            if tok == Token::EOF {
                break;
            }
            if tok == Token::SEMICOLON && lit != ";" {
                let off = file.offset(pos) as usize;
                if off != input.len() && input.as_bytes()[off] != b'\n' {
                    panic!(
                        "scanning <<{}>>, got SEMICOLON at offset {}, want newline or EOF",
                        input, off
                    );
                }
            }
            tokens.push(tok.to_string());
        }
        let got = tokens.join(" ");
        assert_eq!(got, want, "scanning <<{}>>", input);
    }

    #[test]
    fn test_semicolons() {
        let cases = semicolon_cases();
        for (input, want) in &cases {
            check_semi(input, want, Mode::NONE);
            check_semi(input, want, SCAN_COMMENTS);
            // Trim trailing newlines and re-test.
            let bytes = input.as_bytes();
            let mut end = bytes.len();
            while end > 0 && bytes[end - 1] == b'\n' {
                end -= 1;
                let trimmed = std::str::from_utf8(&bytes[..end]).unwrap();
                check_semi(trimmed, want, Mode::NONE);
                check_semi(trimmed, want, SCAN_COMMENTS);
            }
        }
    }

    fn semicolon_cases() -> Vec<(&'static str, &'static str)> {
        vec![
            ("", ""),
            ("\u{feff};", ";"),
            (";", ";"),
            ("foo\n", "IDENT ;"),
            ("123\n", "INT ;"),
            ("1.2\n", "FLOAT ;"),
            ("'x'\n", "CHAR ;"),
            ("\"x\"\n", "STRING ;"),
            ("`x`\n", "STRING ;"),
            ("+\n", "+"),
            ("-\n", "-"),
            ("*\n", "*"),
            ("/\n", "/"),
            ("%\n", "%"),
            ("&\n", "&"),
            ("|\n", "|"),
            ("^\n", "^"),
            ("<<\n", "<<"),
            (">>\n", ">>"),
            ("&^\n", "&^"),
            ("+=\n", "+="),
            ("-=\n", "-="),
            ("*=\n", "*="),
            ("/=\n", "/="),
            ("%=\n", "%="),
            ("&=\n", "&="),
            ("|=\n", "|="),
            ("^=\n", "^="),
            ("<<=\n", "<<="),
            (">>=\n", ">>="),
            ("&^=\n", "&^="),
            ("&&\n", "&&"),
            ("||\n", "||"),
            ("<-\n", "<-"),
            ("++\n", "++ ;"),
            ("--\n", "-- ;"),
            ("==\n", "=="),
            ("<\n", "<"),
            (">\n", ">"),
            ("=\n", "="),
            ("!\n", "!"),
            ("!=\n", "!="),
            ("<=\n", "<="),
            (">=\n", ">="),
            (":=\n", ":="),
            ("...\n", "..."),
            ("(\n", "("),
            ("[\n", "["),
            ("{\n", "{"),
            (",\n", ","),
            (".\n", "."),
            (")\n", ") ;"),
            ("]\n", "] ;"),
            ("}\n", "} ;"),
            (";\n", ";"),
            (":\n", ":"),
            ("break\n", "break ;"),
            ("case\n", "case"),
            ("chan\n", "chan"),
            ("const\n", "const"),
            ("continue\n", "continue ;"),
            ("default\n", "default"),
            ("defer\n", "defer"),
            ("else\n", "else"),
            ("fallthrough\n", "fallthrough ;"),
            ("for\n", "for"),
            ("func\n", "func"),
            ("go\n", "go"),
            ("goto\n", "goto"),
            ("if\n", "if"),
            ("import\n", "import"),
            ("interface\n", "interface"),
            ("map\n", "map"),
            ("package\n", "package"),
            ("range\n", "range"),
            ("return\n", "return ;"),
            ("select\n", "select"),
            ("struct\n", "struct"),
            ("switch\n", "switch"),
            ("type\n", "type"),
            ("var\n", "var"),
            ("foo//comment\n", "IDENT COMMENT ;"),
            ("foo//comment", "IDENT COMMENT ;"),
            ("foo/*comment*/\n", "IDENT COMMENT ;"),
            ("foo/*\n*/", "IDENT COMMENT ;"),
            ("foo/*comment*/    \n", "IDENT COMMENT ;"),
            ("foo/*\n*/    ", "IDENT COMMENT ;"),
            ("foo    // comment\n", "IDENT COMMENT ;"),
            ("foo    // comment", "IDENT COMMENT ;"),
            ("foo    /*comment*/\n", "IDENT COMMENT ;"),
            ("foo    /*\n*/", "IDENT COMMENT ;"),
            (
                "foo    /*  */ /* \n */ bar/**/\n",
                "IDENT COMMENT COMMENT ; IDENT COMMENT ;",
            ),
            (
                "foo    /*0*/ /*1*/ /*2*/\n",
                "IDENT COMMENT COMMENT COMMENT ;",
            ),
            ("foo    /*comment*/    \n", "IDENT COMMENT ;"),
            (
                "foo    /*0*/ /*1*/ /*2*/    \n",
                "IDENT COMMENT COMMENT COMMENT ;",
            ),
            (
                "foo\t/**/ /*-------------*/       /*----\n*/bar       /*  \n*/baa\n",
                "IDENT COMMENT COMMENT COMMENT ; IDENT COMMENT ; IDENT ;",
            ),
            ("foo    /* an EOF terminates a line */", "IDENT COMMENT ;"),
            (
                "foo    /* an EOF terminates a line */ /*",
                "IDENT COMMENT COMMENT ;",
            ),
            (
                "foo    /* an EOF terminates a line */ //",
                "IDENT COMMENT COMMENT ;",
            ),
            (
                "package main\n\nfunc main() {\n\tif {\n\t\treturn /* */ }\n}\n",
                "package IDENT ; func IDENT ( ) { if { return COMMENT } ; } ;",
            ),
            ("package main", "package IDENT ;"),
        ]
    }

    // ---- TestLineDirectives -----------------------------------------

    struct Segment {
        srcline: &'static str,
        filename: &'static str,
        line: i64,
        column: i64,
    }

    fn line_segments() -> Vec<Segment> {
        vec![
            Segment {
                srcline: "  line1",
                filename: "TestLineDirectives",
                line: 1,
                column: 3,
            },
            Segment {
                srcline: "\nline2",
                filename: "TestLineDirectives",
                line: 2,
                column: 1,
            },
            Segment {
                srcline: "\nline3  //line File1.go:100",
                filename: "TestLineDirectives",
                line: 3,
                column: 1,
            },
            Segment {
                srcline: "\nline4",
                filename: "TestLineDirectives",
                line: 4,
                column: 1,
            },
            Segment {
                srcline: "\n//line File1.go:100\n  line100",
                filename: "File1.go",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n//line  \t :42\n  line1",
                filename: " \t ",
                line: 42,
                column: 0,
            },
            Segment {
                srcline: "\n//line File2.go:200\n  line200",
                filename: "File2.go",
                line: 200,
                column: 0,
            },
            Segment {
                srcline: "\n//line foo\t:42\n  line42",
                filename: "foo\t",
                line: 42,
                column: 0,
            },
            Segment {
                srcline: "\n //line foo:42\n  line43",
                filename: "foo\t",
                line: 44,
                column: 0,
            },
            Segment {
                srcline: "\n//line foo 42\n  line44",
                filename: "foo\t",
                line: 46,
                column: 0,
            },
            Segment {
                srcline: "\n//line /bar:42\n  line45",
                filename: "/bar",
                line: 42,
                column: 0,
            },
            Segment {
                srcline: "\n//line ./foo:42\n  line46",
                filename: "foo",
                line: 42,
                column: 0,
            },
            Segment {
                srcline: "\n//line a/b/c/File1.go:100\n  line100",
                filename: "a/b/c/File1.go",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n//line c:\\bar:42\n  line200",
                filename: "c:\\bar",
                line: 42,
                column: 0,
            },
            Segment {
                srcline: "\n//line c:\\dir\\File1.go:100\n  line201",
                filename: "c:\\dir\\File1.go",
                line: 100,
                column: 0,
            },
            // new syntax
            Segment {
                srcline: "\n//line :100\na1",
                filename: "",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n//line bar:100\nb1",
                filename: "bar",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n//line :100:10\nc1",
                filename: "bar",
                line: 100,
                column: 10,
            },
            Segment {
                srcline: "\n//line foo:100:10\nd1",
                filename: "foo",
                line: 100,
                column: 10,
            },
            Segment {
                srcline: "\n/*line :100*/a2",
                filename: "",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n/*line bar:100*/b2",
                filename: "bar",
                line: 100,
                column: 0,
            },
            Segment {
                srcline: "\n/*line :100:10*/c2",
                filename: "bar",
                line: 100,
                column: 10,
            },
            Segment {
                srcline: "\n/*line foo:100:10*/d2",
                filename: "foo",
                line: 100,
                column: 10,
            },
            Segment {
                srcline: "\n/*line foo:100:10*/    e2",
                filename: "foo",
                line: 100,
                column: 14,
            },
            Segment {
                srcline: "\n/*line foo:100:10*/\n\nf2",
                filename: "foo",
                line: 102,
                column: 1,
            },
        ]
    }

    fn run_segments(segments: &[Segment], filename: &str) {
        let mut src = String::new();
        for s in segments {
            src.push_str(s.srcline);
        }
        let fset = fset();
        let file = fset.add_file(filename, fset.base(), src.len() as i64);
        let mut sc: Scanner<'_> = Scanner::new();
        let eh: ErrorHandler<'_> = Box::new(|pos: Position, msg: &str| {
            panic!("unexpected scanner error: {}: {}", pos, msg);
        });
        sc.init(file.clone(), src.as_bytes(), Some(eh), DONT_INSERT_SEMIS);
        for s in segments {
            let (p, _, lit) = sc.scan();
            let pos = file.position(p);
            check_pos(
                &lit,
                &fset,
                p,
                &Position {
                    filename: s.filename.to_string(),
                    offset: pos.offset,
                    line: s.line,
                    column: s.column,
                },
            );
        }
        assert_eq!(sc.error_count, 0);
    }

    #[test]
    fn test_line_directives() {
        run_segments(&line_segments(), "TestLineDirectives");
        let dirseg = [
            Segment {
                srcline: "  line1",
                filename: "TestLineDir/TestLineDirectives",
                line: 1,
                column: 3,
            },
            Segment {
                srcline: "\n//line File1.go:100\n  line100",
                filename: "TestLineDir/File1.go",
                line: 100,
                column: 0,
            },
        ];
        run_segments(&dirseg, "TestLineDir/TestLineDirectives");
        // Unix-only equivalent of dirUnixSegments
        let unixseg = [Segment {
            srcline: "\n//line /bar:42\n  line42",
            filename: "/bar",
            line: 42,
            column: 0,
        }];
        run_segments(&unixseg, "TestLineDir/TestLineDirectives");
    }

    // ---- TestInvalidLineDirectives -----------------------------------

    struct InvalidSeg {
        srcline: &'static str,
        msg: &'static str,
        line: i64,
        column: i64,
    }

    fn invalid_segments() -> Vec<InvalidSeg> {
        vec![
            InvalidSeg {
                srcline: "\n//line :1:1\n//line foo:42 extra text\ndummy",
                msg: "invalid line number: 42 extra text",
                line: 1,
                column: 12,
            },
            InvalidSeg {
                srcline: "\n//line :2:1\n//line foobar:\ndummy",
                msg: "invalid line number: ",
                line: 2,
                column: 15,
            },
            InvalidSeg {
                srcline: "\n//line :5:1\n//line :0\ndummy",
                msg: "invalid line number: 0",
                line: 5,
                column: 9,
            },
            InvalidSeg {
                srcline: "\n//line :10:1\n//line :1:0\ndummy",
                msg: "invalid column number: 0",
                line: 10,
                column: 11,
            },
            InvalidSeg {
                srcline: "\n//line :1:1\n//line :foo:0\ndummy",
                msg: "invalid line number: 0",
                line: 1,
                column: 13,
            },
        ]
    }

    #[test]
    fn test_invalid_line_directives() {
        let segs = invalid_segments();
        let mut src = String::new();
        for s in &segs {
            src.push_str(s.srcline);
        }

        let fset = fset();
        let file = fset.add_file(
            &path_join("dir", "TestInvalidLineDirectives"),
            fset.base(),
            src.len() as i64,
        );
        let observed: std::sync::Arc<std::sync::Mutex<Vec<(String, Position)>>> =
            std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let collector = std::sync::Arc::clone(&observed);
        let eh: ErrorHandler<'_> = Box::new(move |pos: Position, msg: &str| {
            collector.lock().unwrap().push((msg.to_string(), pos));
        });

        let mut sc: Scanner<'_> = Scanner::new();
        sc.init(file, src.as_bytes(), Some(eh), DONT_INSERT_SEMIS);
        for _ in &segs {
            sc.scan();
        }

        let recorded = observed.lock().unwrap();
        assert_eq!(
            sc.error_count,
            segs.len(),
            "got {} errors; want {}",
            sc.error_count,
            segs.len()
        );
        assert_eq!(recorded.len(), segs.len(), "handler invocations");
        for (i, s) in segs.iter().enumerate() {
            let (got_msg, got_pos) = &recorded[i];
            assert_eq!(got_msg, s.msg, "msg #{}", i);
            assert_eq!(got_pos.line, s.line, "line #{}", i);
            assert_eq!(got_pos.column, s.column, "column #{}", i);
        }
    }

    // ---- TestInit ----------------------------------------------------

    #[test]
    fn test_init() {
        let fset = fset();
        let mut s: Scanner<'_> = Scanner::new();
        let src1 = "if true { }";
        let f1 = fset.add_file("src1", fset.base(), src1.len() as i64);
        s.init(f1.clone(), src1.as_bytes(), None, DONT_INSERT_SEMIS);
        assert_eq!(f1.size(), src1.len() as i64);
        s.scan(); // if
        s.scan(); // true
        let (_, tok, _) = s.scan(); // {
        assert_eq!(tok, Token::LBRACE);

        let src2 = "go true { ]";
        let f2 = fset.add_file("src2", fset.base(), src2.len() as i64);
        s.init(f2.clone(), src2.as_bytes(), None, DONT_INSERT_SEMIS);
        assert_eq!(f2.size(), src2.len() as i64);
        let (_, tok, _) = s.scan();
        assert_eq!(tok, Token::GO);
        assert_eq!(s.error_count, 0);
    }

    // ---- TestStdErrorHandler -----------------------------------------

    #[test]
    fn test_std_error_handler() {
        use crate::errors::ErrorList;
        let src = "@\n@ @\n//line File2:20\n@\n//line File2:1\n@ @\n//line File1:1\n@ @ @";

        let collected: std::sync::Arc<std::sync::Mutex<ErrorList>> =
            std::sync::Arc::new(std::sync::Mutex::new(ErrorList::new()));
        let inner = std::sync::Arc::clone(&collected);
        let eh: ErrorHandler<'_> = Box::new(move |pos: Position, msg: &str| {
            inner.lock().unwrap().add(pos, msg);
        });

        let fset = fset();
        let mut s: Scanner<'_> = Scanner::new();
        s.init(
            fset.add_file("File1", fset.base(), src.len() as i64),
            src.as_bytes(),
            Some(eh),
            DONT_INSERT_SEMIS,
        );
        loop {
            let (_, tok, _) = s.scan();
            if tok == Token::EOF {
                break;
            }
        }

        let mut list = collected.lock().unwrap().clone();
        assert_eq!(list.len(), s.error_count, "raw vs ErrorCount");
        assert_eq!(list.len(), 9, "found {} raw errors, expected 9", list.len());

        list.sort();
        assert_eq!(list.len(), 9, "after sort");

        list.remove_multiples();
        assert_eq!(list.len(), 4, "after remove_multiples");
    }

    // ---- TestScanErrors ---------------------------------------------

    struct ErrCase {
        src: Vec<u8>,
        tok: Token,
        pos: i64,
        lit: Vec<u8>,
        err: &'static str,
    }

    fn case_str(src: &str, tok: Token, pos: i64, lit: &str, err: &'static str) -> ErrCase {
        ErrCase {
            src: src.as_bytes().to_vec(),
            tok,
            pos,
            lit: lit.as_bytes().to_vec(),
            err,
        }
    }

    fn check_error(case: &ErrCase) {
        let fset = fset();
        let collected: std::sync::Arc<std::sync::Mutex<(usize, String, Position)>> =
            std::sync::Arc::new(std::sync::Mutex::new((
                0usize,
                String::new(),
                Position::default(),
            )));
        let inner = std::sync::Arc::clone(&collected);
        let eh: ErrorHandler<'_> = Box::new(move |pos: Position, msg: &str| {
            let mut g = inner.lock().unwrap();
            g.0 += 1;
            g.1 = msg.to_string();
            g.2 = pos;
        });
        let src_for_msg = String::from_utf8_lossy(&case.src).into_owned();
        let mut s: Scanner<'_> = Scanner::new();
        s.init(
            fset.add_file("", fset.base(), case.src.len() as i64),
            &case.src,
            Some(eh),
            SCAN_COMMENTS | DONT_INSERT_SEMIS,
        );
        let (_, tok0, lit0) = s.scan();
        assert_eq!(
            tok0, case.tok,
            "{:?}: got token {:?}, expected {:?}",
            src_for_msg, tok0, case.tok
        );
        if tok0 != Token::ILLEGAL {
            assert_eq!(
                lit0.as_bytes(),
                case.lit.as_slice(),
                "{:?}: got literal {:?}, expected {:?}",
                src_for_msg,
                lit0,
                String::from_utf8_lossy(&case.lit)
            );
        }
        let want_cnt = if case.err.is_empty() { 0 } else { 1 };
        let g = collected.lock().unwrap();
        assert_eq!(g.0, want_cnt, "{:?}: cnt", src_for_msg);
        assert_eq!(g.1, case.err, "{:?}: msg", src_for_msg);
        assert_eq!(g.2.offset, case.pos, "{:?}: offset", src_for_msg);
    }

    #[test]
    fn test_scan_errors() {
        let mut cases = vec![
            case_str(
                "\u{0007}",
                Token::ILLEGAL,
                0,
                "",
                "illegal character U+0007",
            ),
            case_str("#", Token::ILLEGAL, 0, "", "illegal character U+0023 '#'"),
            case_str("…", Token::ILLEGAL, 0, "", "illegal character U+2026 '…'"),
            case_str("..", Token::PERIOD, 0, "", ""),
            case_str("' '", Token::CHAR, 0, "' '", ""),
            case_str("''", Token::CHAR, 0, "''", "illegal rune literal"),
            case_str("'12'", Token::CHAR, 0, "'12'", "illegal rune literal"),
            case_str("'123'", Token::CHAR, 0, "'123'", "illegal rune literal"),
            case_str(
                "'\\0'",
                Token::CHAR,
                3,
                "'\\0'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\07'",
                Token::CHAR,
                4,
                "'\\07'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str("'\\8'", Token::CHAR, 2, "'\\8'", "unknown escape sequence"),
            case_str(
                "'\\08'",
                Token::CHAR,
                3,
                "'\\08'",
                "illegal character U+0038 '8' in escape sequence",
            ),
            case_str(
                "'\\x'",
                Token::CHAR,
                3,
                "'\\x'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\x0'",
                Token::CHAR,
                4,
                "'\\x0'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\x0g'",
                Token::CHAR,
                4,
                "'\\x0g'",
                "illegal character U+0067 'g' in escape sequence",
            ),
            case_str(
                "'\\u'",
                Token::CHAR,
                3,
                "'\\u'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\u0'",
                Token::CHAR,
                4,
                "'\\u0'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\u00'",
                Token::CHAR,
                5,
                "'\\u00'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\u000'",
                Token::CHAR,
                6,
                "'\\u000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\u000",
                Token::CHAR,
                6,
                "'\\u000",
                "escape sequence not terminated",
            ),
            case_str("'\\u0000'", Token::CHAR, 0, "'\\u0000'", ""),
            case_str(
                "'\\U'",
                Token::CHAR,
                3,
                "'\\U'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U0'",
                Token::CHAR,
                4,
                "'\\U0'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U00'",
                Token::CHAR,
                5,
                "'\\U00'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U000'",
                Token::CHAR,
                6,
                "'\\U000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U0000'",
                Token::CHAR,
                7,
                "'\\U0000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U00000'",
                Token::CHAR,
                8,
                "'\\U00000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U000000'",
                Token::CHAR,
                9,
                "'\\U000000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U0000000'",
                Token::CHAR,
                10,
                "'\\U0000000'",
                "illegal character U+0027 ''' in escape sequence",
            ),
            case_str(
                "'\\U0000000",
                Token::CHAR,
                10,
                "'\\U0000000",
                "escape sequence not terminated",
            ),
            case_str("'\\U00000000'", Token::CHAR, 0, "'\\U00000000'", ""),
            case_str(
                "'\\Uffffffff'",
                Token::CHAR,
                2,
                "'\\Uffffffff'",
                "escape sequence is invalid Unicode code point",
            ),
            case_str("'", Token::CHAR, 0, "'", "rune literal not terminated"),
            case_str(
                "'\\",
                Token::CHAR,
                2,
                "'\\",
                "escape sequence not terminated",
            ),
            case_str("'\n", Token::CHAR, 0, "'", "rune literal not terminated"),
            case_str("'\n   ", Token::CHAR, 0, "'", "rune literal not terminated"),
            case_str("\"\"", Token::STRING, 0, "\"\"", ""),
            case_str(
                "\"abc",
                Token::STRING,
                0,
                "\"abc",
                "string literal not terminated",
            ),
            case_str(
                "\"abc\n",
                Token::STRING,
                0,
                "\"abc",
                "string literal not terminated",
            ),
            case_str(
                "\"abc\n   ",
                Token::STRING,
                0,
                "\"abc",
                "string literal not terminated",
            ),
            case_str("``", Token::STRING, 0, "``", ""),
            case_str(
                "`",
                Token::STRING,
                0,
                "`",
                "raw string literal not terminated",
            ),
            case_str("/**/", Token::COMMENT, 0, "/**/", ""),
            case_str("/*", Token::COMMENT, 0, "/*", "comment not terminated"),
            case_str("077", Token::INT, 0, "077", ""),
            case_str("078.", Token::FLOAT, 0, "078.", ""),
            case_str("07801234567.", Token::FLOAT, 0, "07801234567.", ""),
            case_str("078e0", Token::FLOAT, 0, "078e0", ""),
            case_str("0E", Token::FLOAT, 2, "0E", "exponent has no digits"),
            case_str(
                "078",
                Token::INT,
                2,
                "078",
                "invalid digit '8' in octal literal",
            ),
            case_str(
                "07090000008",
                Token::INT,
                3,
                "07090000008",
                "invalid digit '9' in octal literal",
            ),
            case_str(
                "0x",
                Token::INT,
                2,
                "0x",
                "hexadecimal literal has no digits",
            ),
            case_str(
                "\"abc\x00def\"",
                Token::STRING,
                4,
                "\"abc\x00def\"",
                "illegal character NUL",
            ),
            case_str(
                "\u{feff}\u{feff}",
                Token::ILLEGAL,
                3,
                "\u{feff}\u{feff}",
                "illegal byte order mark",
            ),
            case_str(
                "//\u{feff}",
                Token::COMMENT,
                2,
                "//\u{feff}",
                "illegal byte order mark",
            ),
            case_str(
                "'\u{feff}'",
                Token::CHAR,
                1,
                "'\u{feff}'",
                "illegal byte order mark",
            ),
            case_str(
                "\"abc\u{feff}def\"",
                Token::STRING,
                4,
                "\"abc\u{feff}def\"",
                "illegal byte order mark",
            ),
            case_str(
                "abc\x00def",
                Token::IDENT,
                3,
                "abc",
                "illegal character NUL",
            ),
            case_str("abc\x00", Token::IDENT, 3, "abc", "illegal character NUL"),
            case_str(
                "“abc”",
                Token::ILLEGAL,
                0,
                "abc",
                "curly quotation mark '“' (use neutral '\"')",
            ),
        ];
        // Invalid-UTF-8 case: source contains a stray 0x80 continuation byte.
        // Go preserves the raw bytes in the returned literal (its strings are
        // []byte). This port returns String, so the invalid byte is replaced
        // with U+FFFD (encoded as 0xEF 0xBF 0xBD).
        cases.push(ErrCase {
            src: b"\"abc\x80def\"".to_vec(),
            tok: Token::STRING,
            pos: 4,
            lit: b"\"abc\xef\xbf\xbddef\"".to_vec(),
            err: "illegal UTF-8 encoding",
        });
        for c in &cases {
            check_error(c);
        }
    }

    // ---- TestUTF16 ---------------------------------------------------

    #[test]
    fn test_utf16() {
        let srcs = [
            // BE BOM + "package p" UTF-16
            b"\xfe\xff\x00p\x00a\x00c\x00k\x00a\x00g\x00e\x00 \x00p".to_vec(),
            // LE BOM + "package p" UTF-16
            b"\xff\xfep\x00a\x00c\x00k\x00a\x00g\x00e\x00 \x00p\x00".to_vec(),
        ];
        for src in srcs {
            let collected: std::sync::Arc<std::sync::Mutex<Vec<String>>> =
                std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
            let inner = std::sync::Arc::clone(&collected);
            let eh: ErrorHandler<'_> = Box::new(move |pos: Position, msg: &str| {
                inner
                    .lock()
                    .unwrap()
                    .push(format!("#{}: {}", pos.offset, msg));
            });
            let fset = fset();
            let mut sc: Scanner<'_> = Scanner::new();
            sc.init(
                fset.add_file("", fset.base(), src.len() as i64),
                &src,
                Some(eh),
                Mode::NONE,
            );
            sc.scan();
            let got = collected.lock().unwrap().clone();
            let want = vec![
                "#0: illegal UTF-8 encoding (got UTF-16)".to_string(),
                "#0: illegal character U+FFFD '\u{FFFD}'".to_string(),
            ];
            assert_eq!(got, want, "for source {:?}", src);
        }
    }

    // ---- TestIssue10213 ---------------------------------------------

    #[test]
    fn test_issue_10213() {
        let src = r#"
            var (
                A = 1 // foo
            )

            var (
                B = 2
                // foo
            )

            var C = 3 // foo

            var D = 4
            // foo

            func anycode() {
            // foo
            }
        "#;
        let fset = fset();
        let mut s: Scanner<'_> = Scanner::new();
        s.init(
            fset.add_file("", fset.base(), src.len() as i64),
            src.as_bytes(),
            None,
            Mode::NONE,
        );
        loop {
            let (_, tok, lit) = s.scan();
            let class = tokenclass(tok);
            if !lit.is_empty() && class != KEYWORD && class != LITERAL && tok != Token::SEMICOLON {
                panic!("tok = {:?}, lit = {:?}", tok, lit);
            }
            if (tok as i32) <= (Token::EOF as i32) {
                break;
            }
        }
    }

    // ---- TestIssue28112 ---------------------------------------------

    #[test]
    fn test_issue_28112() {
        let src = "... .. 0.. ..";
        let want_tokens = [
            Token::ELLIPSIS,
            Token::PERIOD,
            Token::PERIOD,
            Token::FLOAT,
            Token::PERIOD,
            Token::PERIOD,
            Token::PERIOD,
            Token::EOF,
        ];
        let fset = fset();
        let mut s: Scanner<'_> = Scanner::new();
        s.init(
            fset.add_file("", fset.base(), src.len() as i64),
            src.as_bytes(),
            None,
            Mode::NONE,
        );
        for want in want_tokens {
            let (_, got, lit) = s.scan();
            assert_eq!(got, want);
            if tokenclass(got) == LITERAL {
                assert!(!lit.is_empty(), "empty literal for {:?}", got);
            }
        }
    }

    // ---- TestNumbers -------------------------------------------------

    struct NumCase {
        tok: Token,
        src: &'static str,
        tokens: &'static str,
        err: &'static str,
    }

    fn numbers_cases() -> Vec<NumCase> {
        use Token::*;
        vec![
            // binaries
            NumCase {
                tok: INT,
                src: "0b0",
                tokens: "0b0",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0b1010",
                tokens: "0b1010",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0B1110",
                tokens: "0B1110",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0b",
                tokens: "0b",
                err: "binary literal has no digits",
            },
            NumCase {
                tok: INT,
                src: "0b0190",
                tokens: "0b0190",
                err: "invalid digit '9' in binary literal",
            },
            NumCase {
                tok: INT,
                src: "0b01a0",
                tokens: "0b01 a0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0b.",
                tokens: "0b.",
                err: "invalid radix point in binary literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0b.1",
                tokens: "0b.1",
                err: "invalid radix point in binary literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0b1.0",
                tokens: "0b1.0",
                err: "invalid radix point in binary literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0b1e10",
                tokens: "0b1e10",
                err: "'e' exponent requires decimal mantissa",
            },
            NumCase {
                tok: FLOAT,
                src: "0b1P-1",
                tokens: "0b1P-1",
                err: "'P' exponent requires hexadecimal mantissa",
            },
            NumCase {
                tok: IMAG,
                src: "0b10i",
                tokens: "0b10i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0b10.0i",
                tokens: "0b10.0i",
                err: "invalid radix point in binary literal",
            },
            // octals
            NumCase {
                tok: INT,
                src: "0o0",
                tokens: "0o0",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0o1234",
                tokens: "0o1234",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0O1234",
                tokens: "0O1234",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0o",
                tokens: "0o",
                err: "octal literal has no digits",
            },
            NumCase {
                tok: INT,
                src: "0o8123",
                tokens: "0o8123",
                err: "invalid digit '8' in octal literal",
            },
            NumCase {
                tok: INT,
                src: "0o1293",
                tokens: "0o1293",
                err: "invalid digit '9' in octal literal",
            },
            NumCase {
                tok: INT,
                src: "0o12a3",
                tokens: "0o12 a3",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0o.",
                tokens: "0o.",
                err: "invalid radix point in octal literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0o.2",
                tokens: "0o.2",
                err: "invalid radix point in octal literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0o1.2",
                tokens: "0o1.2",
                err: "invalid radix point in octal literal",
            },
            NumCase {
                tok: FLOAT,
                src: "0o1E+2",
                tokens: "0o1E+2",
                err: "'E' exponent requires decimal mantissa",
            },
            NumCase {
                tok: FLOAT,
                src: "0o1p10",
                tokens: "0o1p10",
                err: "'p' exponent requires hexadecimal mantissa",
            },
            NumCase {
                tok: IMAG,
                src: "0o10i",
                tokens: "0o10i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0o10e0i",
                tokens: "0o10e0i",
                err: "'e' exponent requires decimal mantissa",
            },
            // 0-octals
            NumCase {
                tok: INT,
                src: "0",
                tokens: "0",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0123",
                tokens: "0123",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "08123",
                tokens: "08123",
                err: "invalid digit '8' in octal literal",
            },
            NumCase {
                tok: INT,
                src: "01293",
                tokens: "01293",
                err: "invalid digit '9' in octal literal",
            },
            NumCase {
                tok: INT,
                src: "0F.",
                tokens: "0 F .",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0123F.",
                tokens: "0123 F .",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0123456x",
                tokens: "0123456 x",
                err: "",
            },
            // decimals
            NumCase {
                tok: INT,
                src: "1",
                tokens: "1",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "1234",
                tokens: "1234",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "1f",
                tokens: "1 f",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0i",
                tokens: "0i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0678i",
                tokens: "0678i",
                err: "",
            },
            // decimal floats
            NumCase {
                tok: FLOAT,
                src: "0.",
                tokens: "0.",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "123.",
                tokens: "123.",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0123.",
                tokens: "0123.",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".0",
                tokens: ".0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".123",
                tokens: ".123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".0123",
                tokens: ".0123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0.0",
                tokens: "0.0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "123.123",
                tokens: "123.123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0123.0123",
                tokens: "0123.0123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0e0",
                tokens: "0e0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "123e+0",
                tokens: "123e+0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0123E-1",
                tokens: "0123E-1",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0.e+1",
                tokens: "0.e+1",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "123.E-10",
                tokens: "123.E-10",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0123.e123",
                tokens: "0123.e123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".0e-1",
                tokens: ".0e-1",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".123E+10",
                tokens: ".123E+10",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: ".0123E123",
                tokens: ".0123E123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0.0e1",
                tokens: "0.0e1",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "123.123E-10",
                tokens: "123.123E-10",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0123.0123e+456",
                tokens: "0123.0123e+456",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0e",
                tokens: "0e",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0E+",
                tokens: "0E+",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "1e+f",
                tokens: "1e+ f",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0p0",
                tokens: "0p0",
                err: "'p' exponent requires hexadecimal mantissa",
            },
            NumCase {
                tok: FLOAT,
                src: "1.0P-1",
                tokens: "1.0P-1",
                err: "'P' exponent requires hexadecimal mantissa",
            },
            NumCase {
                tok: IMAG,
                src: "0.i",
                tokens: "0.i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: ".123i",
                tokens: ".123i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "123.123i",
                tokens: "123.123i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "123e+0i",
                tokens: "123e+0i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "123.E-10i",
                tokens: "123.E-10i",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: ".123E+10i",
                tokens: ".123E+10i",
                err: "",
            },
            // hex
            NumCase {
                tok: INT,
                src: "0x0",
                tokens: "0x0",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0x1234",
                tokens: "0x1234",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0xcafef00d",
                tokens: "0xcafef00d",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0XCAFEF00D",
                tokens: "0XCAFEF00D",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0x",
                tokens: "0x",
                err: "hexadecimal literal has no digits",
            },
            NumCase {
                tok: INT,
                src: "0x1g",
                tokens: "0x1 g",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0xf00i",
                tokens: "0xf00i",
                err: "",
            },
            // hex floats
            NumCase {
                tok: FLOAT,
                src: "0x0p0",
                tokens: "0x0p0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0x12efp-123",
                tokens: "0x12efp-123",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0xABCD.p+0",
                tokens: "0xABCD.p+0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0x.0189P-0",
                tokens: "0x.0189P-0",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.ffffp+1023",
                tokens: "0x1.ffffp+1023",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0x.",
                tokens: "0x.",
                err: "hexadecimal literal has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0x0.",
                tokens: "0x0.",
                err: "hexadecimal mantissa requires a 'p' exponent",
            },
            NumCase {
                tok: FLOAT,
                src: "0x.0",
                tokens: "0x.0",
                err: "hexadecimal mantissa requires a 'p' exponent",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.1",
                tokens: "0x1.1",
                err: "hexadecimal mantissa requires a 'p' exponent",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.1e0",
                tokens: "0x1.1e0",
                err: "hexadecimal mantissa requires a 'p' exponent",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.2gp1a",
                tokens: "0x1.2 gp1a",
                err: "hexadecimal mantissa requires a 'p' exponent",
            },
            NumCase {
                tok: FLOAT,
                src: "0x0p",
                tokens: "0x0p",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0xeP-",
                tokens: "0xeP-",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1234PAB",
                tokens: "0x1234P AB",
                err: "exponent has no digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.2p1a",
                tokens: "0x1.2p1 a",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "0xf00.bap+12i",
                tokens: "0xf00.bap+12i",
                err: "",
            },
            // separators
            NumCase {
                tok: INT,
                src: "0b_1000_0001",
                tokens: "0b_1000_0001",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0o_600",
                tokens: "0o_600",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0_466",
                tokens: "0_466",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "1_000",
                tokens: "1_000",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "1_000.000_1",
                tokens: "1_000.000_1",
                err: "",
            },
            NumCase {
                tok: IMAG,
                src: "10e+1_2_3i",
                tokens: "10e+1_2_3i",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0x_f00d",
                tokens: "0x_f00d",
                err: "",
            },
            NumCase {
                tok: FLOAT,
                src: "0x_f00d.0p1_2",
                tokens: "0x_f00d.0p1_2",
                err: "",
            },
            NumCase {
                tok: INT,
                src: "0b__1000",
                tokens: "0b__1000",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: INT,
                src: "0o60___0",
                tokens: "0o60___0",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: INT,
                src: "0466_",
                tokens: "0466_",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: FLOAT,
                src: "1_.",
                tokens: "1_.",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0._1",
                tokens: "0._1",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: FLOAT,
                src: "2.7_e0",
                tokens: "2.7_e0",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: IMAG,
                src: "10e+12_i",
                tokens: "10e+12_i",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: INT,
                src: "0x___0",
                tokens: "0x___0",
                err: "'_' must separate successive digits",
            },
            NumCase {
                tok: FLOAT,
                src: "0x1.0_p0",
                tokens: "0x1.0_p0",
                err: "'_' must separate successive digits",
            },
        ]
    }

    #[test]
    fn test_numbers() {
        for case in numbers_cases() {
            let err_box: std::sync::Arc<std::sync::Mutex<String>> =
                std::sync::Arc::new(std::sync::Mutex::new(String::new()));
            let inner = std::sync::Arc::clone(&err_box);
            let eh: ErrorHandler<'_> = Box::new(move |_pos: Position, msg: &str| {
                let mut g = inner.lock().unwrap();
                if g.is_empty() {
                    *g = msg.to_string();
                }
            });

            let fset = fset();
            let mut s: Scanner<'_> = Scanner::new();
            s.init(
                fset.add_file("", fset.base(), case.src.len() as i64),
                case.src.as_bytes(),
                Some(eh),
                Mode::NONE,
            );

            let want_pieces: Vec<&str> = case.tokens.split(' ').collect();
            for (i, want) in want_pieces.iter().enumerate() {
                {
                    let mut g = err_box.lock().unwrap();
                    g.clear();
                }
                let (_, tok, mut lit) = s.scan();
                match tok {
                    Token::PERIOD => lit = Cow::Borrowed("."),
                    Token::ADD => lit = Cow::Borrowed("+"),
                    Token::SUB => lit = Cow::Borrowed("-"),
                    _ => {}
                }
                if i == 0 {
                    assert_eq!(tok, case.tok, "{:?}: token", case.src);
                    let g = err_box.lock().unwrap();
                    assert_eq!(g.as_str(), case.err, "{:?}: err", case.src);
                }
                assert_eq!(lit, *want, "{:?}: piece #{}", case.src, i);
            }
            let (_, tok, _) = s.scan();
            let tok = if tok == Token::SEMICOLON {
                s.scan().1
            } else {
                tok
            };
            assert_eq!(tok, Token::EOF, "{:?}: trailing", case.src);
        }
    }

    // ---- Example: ExampleScanner_Scan -------------------------------

    #[test]
    fn example_scanner_scan() {
        let src = b"cos(x) + 1i*sin(x) // Euler";
        let fset = fset();
        let file = fset.add_file("", fset.base(), src.len() as i64);
        let mut s: Scanner<'_> = Scanner::new();
        s.init(file, src, None, SCAN_COMMENTS);

        let mut lines: Vec<String> = Vec::new();
        loop {
            let (pos, tok, lit) = s.scan();
            if tok == Token::EOF {
                break;
            }
            lines.push(format!("{}\t{}\t{:?}", fset.position(pos), tok, lit));
        }
        let expected = vec![
            "1:1\tIDENT\t\"cos\"",
            "1:4\t(\t\"\"",
            "1:5\tIDENT\t\"x\"",
            "1:6\t)\t\"\"",
            "1:8\t+\t\"\"",
            "1:10\tIMAG\t\"1i\"",
            "1:12\t*\t\"\"",
            "1:13\tIDENT\t\"sin\"",
            "1:16\t(\t\"\"",
            "1:17\tIDENT\t\"x\"",
            "1:18\t)\t\"\"",
            "1:20\tCOMMENT\t\"// Euler\"",
            "1:28\t;\t\"\\n\"",
        ];
        assert_eq!(lines, expected);
    }
}
