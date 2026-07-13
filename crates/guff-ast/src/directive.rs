// Port of Go's go/ast/directive.go to Rust.
//
// Original: Copyright 2025 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// `Directive` and the parser for `//tool:name args` comments. The Go
// implementation leans on `strconv.QuotedPrefix` and `strconv.Unquote`
// for argument parsing; this port carries minimal in-module ports of
// both, sufficient for what the tests exercise.

use crate::position::Pos;

/// A `//tool:name args` style comment directive (e.g.
/// `//go:generate stringer -type Op`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Directive {
    pub tool: String,
    pub name: String,
    /// No leading or trailing whitespace.
    pub args: String,
    /// Position of the leading `//`.
    pub slash: Pos,
    /// Position of the first byte of `args` (post-name, post-space).
    pub args_pos: Pos,
}

impl Directive {
    pub fn pos(&self) -> Pos {
        self.slash
    }
    pub fn end(&self) -> Pos {
        Pos(self.args_pos.0 + self.args.len() as i64)
    }
}

/// Parse a single comment line as a directive. Returns [`None`] if `c`
/// isn't a directive comment.
///
/// `c` must include the leading `//`. `pos` is the position of `c[0]`;
/// callers that don't care about positions may pass [`Pos::default()`].
pub fn parse_directive(pos: Pos, c: &str) -> Option<Directive> {
    // Fast path: must start with "//" followed by [a-z0-9].
    let bytes = c.as_bytes();
    if bytes.len() < 3 || bytes[0] != b'/' || bytes[1] != b'/' || !is_alnum(bytes[2]) {
        return None;
    }

    let mut buf = DirectiveScanner { s: c, pos: pos.0 };
    buf.skip(2);

    // Find ':'. Tool occupies [..colon], name starts at colon+1.
    let colon = match buf.s.find(':') {
        Some(i) if i > 0 => i,
        _ => return None,
    };
    if colon + 1 >= buf.s.len() {
        return None;
    }
    let body = buf.s.as_bytes();
    for i in 0..=colon + 1 {
        if i == colon {
            continue;
        }
        if !is_alnum(body[i]) {
            return None;
        }
    }
    let tool = buf.take(colon).to_string();
    buf.skip(1); // ':'

    let name = buf.take_non_space().to_string();
    buf.skip_space();
    let args_pos = Pos(buf.pos);
    let args = buf
        .s
        .trim_end_matches(|c: char| c.is_whitespace())
        .to_string();

    Some(Directive {
        tool,
        name,
        args,
        slash: pos,
        args_pos,
    })
}

fn is_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

/// Single argument extracted from a [`Directive::parse_args`] call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectiveArg {
    /// Parsed argument string. For a quoted argument, the unquoted form.
    pub arg: String,
    /// Position of the first character of this argument in source.
    pub pos: Pos,
}

impl Directive {
    /// Parse `self.args` as a whitespace-separated sequence of bare
    /// words, `"…"`-quoted Go strings, or `` `…` ``-quoted raw strings.
    pub fn parse_args(&self) -> Result<Vec<DirectiveArg>, String> {
        let mut args = DirectiveScanner {
            s: &self.args,
            pos: self.args_pos.0,
        };
        let mut list: Vec<DirectiveArg> = Vec::new();

        loop {
            args.skip_space();
            if args.s.is_empty() {
                break;
            }
            let arg_pos = Pos(args.pos);
            let first = args.s.as_bytes()[0];
            let arg = match first {
                b'`' | b'"' => {
                    let qlen = quoted_prefix_len(args.s).ok_or_else(|| {
                        format!(
                            "invalid quoted string in //{}:{}: {}",
                            self.tool, self.name, args.s
                        )
                    })?;
                    let raw = args.take(qlen);
                    let unq = unquote(raw).map_err(|_| {
                        format!(
                            "invalid quoted string in //{}:{}: {}",
                            self.tool, self.name, raw
                        )
                    })?;
                    // The quote must be followed by whitespace or EOL.
                    if !args.s.is_empty() {
                        let next = args.s.chars().next().unwrap();
                        if !next.is_whitespace() {
                            return Err(format!(
                                "invalid quoted string in //{}:{}: {}",
                                self.tool, self.name, args.s
                            ));
                        }
                    }
                    unq
                }
                _ => args.take_non_space().to_string(),
            };
            list.push(DirectiveArg { arg, pos: arg_pos });
        }
        Ok(list)
    }
}

// ====================================================================
// directiveScanner
// ====================================================================

struct DirectiveScanner<'a> {
    s: &'a str,
    pos: i64,
}

impl<'a> DirectiveScanner<'a> {
    fn skip(&mut self, n: usize) {
        self.pos += n as i64;
        self.s = &self.s[n..];
    }

    fn take(&mut self, n: usize) -> &'a str {
        let head = &self.s[..n];
        self.skip(n);
        head
    }

    fn take_non_space(&mut self) -> &'a str {
        let i = self
            .s
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i)
            .unwrap_or(self.s.len());
        self.take(i)
    }

    fn skip_space(&mut self) {
        let trimmed = self.s.trim_start_matches(|c: char| c.is_whitespace());
        let consumed = self.s.len() - trimmed.len();
        self.skip(consumed);
    }
}

// ====================================================================
// Minimal Go-string Quoted/Unquote helpers
// ====================================================================

/// Length (in bytes) of the longest valid quoted Go string prefix of
/// `s`, including the surrounding quotes. Returns `None` if `s` does
/// not start with a complete quoted string.
fn quoted_prefix_len(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    match b[0] {
        b'`' => {
            // Raw string: scan until the next backtick. No escapes.
            let mut i = 1;
            while i < b.len() {
                if b[i] == b'`' {
                    return Some(i + 1);
                }
                i += 1;
            }
            None
        }
        b'"' => {
            let mut i = 1;
            while i < b.len() {
                let c = b[i];
                if c == b'\n' {
                    return None;
                }
                if c == b'\\' {
                    // Skip the escape; for \x \u \U \ooo we need extra bytes.
                    if i + 1 >= b.len() {
                        return None;
                    }
                    let esc = b[i + 1];
                    let need = match esc {
                        b'x' => 4,        // \xHH
                        b'u' => 6,        // \uHHHH
                        b'U' => 10,       // \UHHHHHHHH
                        b'0'..=b'7' => 4, // \ooo
                        _ => 2,           // single-char escape
                    };
                    if i + need > b.len() {
                        return None;
                    }
                    i += need;
                    continue;
                }
                if c == b'"' {
                    return Some(i + 1);
                }
                i += 1;
            }
            None
        }
        _ => None,
    }
}

/// Unquote a Go double-quoted or back-quoted string literal.
pub(crate) fn unquote(s: &str) -> Result<String, String> {
    let b = s.as_bytes();
    if b.len() < 2 {
        return Err("string too short".to_string());
    }
    let q = b[0];
    if b[b.len() - 1] != q {
        return Err("mismatched quotes".to_string());
    }
    let inner = &s[1..s.len() - 1];
    match q {
        b'`' => {
            // Raw strings strip '\r'. No escapes interpreted.
            if inner.contains('`') {
                return Err("backtick inside raw string".to_string());
            }
            Ok(inner.replace('\r', ""))
        }
        b'"' => {
            let mut out = String::with_capacity(inner.len());
            let mut chars = inner.chars().peekable();
            while let Some(c) = chars.next() {
                if c != '\\' {
                    out.push(c);
                    continue;
                }
                let esc = chars.next().ok_or("dangling escape")?;
                match esc {
                    'a' => out.push('\x07'),
                    'b' => out.push('\x08'),
                    'f' => out.push('\x0c'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    'v' => out.push('\x0b'),
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    '\'' => out.push('\''),
                    'x' => out.push(read_hex_char(&mut chars, 2)?),
                    'u' => out.push(read_hex_char(&mut chars, 4)?),
                    'U' => out.push(read_hex_char(&mut chars, 8)?),
                    d if ('0'..='7').contains(&d) => {
                        let mut v = (d as u32) - ('0' as u32);
                        for _ in 0..2 {
                            let nd = chars.next().ok_or("incomplete octal")?;
                            let dv = nd.to_digit(8).ok_or("bad octal digit")?;
                            v = v * 8 + dv;
                        }
                        out.push(char::from_u32(v).ok_or("invalid octal codepoint")?);
                    }
                    other => return Err(format!("unknown escape \\{}", other)),
                }
            }
            Ok(out)
        }
        _ => Err("not a quoted string".to_string()),
    }
}

fn read_hex_char<I: Iterator<Item = char>>(
    chars: &mut std::iter::Peekable<I>,
    n: usize,
) -> Result<char, String> {
    let mut v = 0u32;
    for _ in 0..n {
        let d = chars.next().ok_or("incomplete hex escape")?;
        let h = d.to_digit(16).ok_or("bad hex digit")?;
        v = v * 16 + h;
    }
    char::from_u32(v).ok_or_else(|| "invalid hex codepoint".to_string())
}

// ====================================================================
// Tests — port of go/ast/directive_test.go (plus the supporting
// `isDirectiveTests` from ast_test.go).
// ====================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn is_directive_tests() -> Vec<(&'static str, bool)> {
        vec![
            ("abc", false),
            ("go:inline", true),
            ("Go:inline", false),
            ("go:Inline", false),
            (":inline", false),
            ("lint:ignore", true),
            ("lint:1234", true),
            ("1234:lint", true),
            ("go: inline", false),
            ("go:", false),
            ("go:*", false),
            ("go:x*", true),
            ("export foo", true),
            ("extern foo", true),
            ("expert foo", false),
        ]
    }

    #[test]
    fn parse_directive_matches_is_directive() {
        for (input, ok) in is_directive_tests() {
            // ParseDirective does NOT support extern/export prefixes.
            let want = if input.starts_with("extern ") || input.starts_with("export ") {
                false
            } else {
                ok
            };
            let full = format!("//{}", input);
            let got = parse_directive(Pos(0), &full).is_some();
            assert_eq!(
                got, want,
                "parse_directive({:?}) returned {}, want {}",
                full, got, want
            );
        }
    }

    #[test]
    fn parse_directive_valid() {
        let got =
            parse_directive(Pos(10), "//go:generate stringer -type Op -trimprefix Op").unwrap();
        assert_eq!(
            got,
            Directive {
                tool: "go".to_string(),
                name: "generate".to_string(),
                args: "stringer -type Op -trimprefix Op".to_string(),
                slash: Pos(10),
                args_pos: Pos(10 + "//go:generate ".len() as i64),
            }
        );
    }

    #[test]
    fn parse_directive_no_args() {
        let got = parse_directive(Pos(20), "//go:build ignore").unwrap();
        assert_eq!(
            got,
            Directive {
                tool: "go".to_string(),
                name: "build".to_string(),
                args: "ignore".to_string(),
                slash: Pos(20),
                args_pos: Pos(20 + "//go:build ".len() as i64),
            }
        );
    }

    #[test]
    fn parse_directive_rejects_non_directives() {
        for input in [
            "// not a directive",
            "go:generate",
            "",
            "//",
            "//go:",
            "//:generate",
        ] {
            assert!(
                parse_directive(Pos(0), input).is_none(),
                "should reject: {:?}",
                input
            );
        }
    }

    #[test]
    fn parse_directive_multiple_spaces() {
        let got = parse_directive(Pos(90), "//go:build  foo bar").unwrap();
        assert_eq!(
            got,
            Directive {
                tool: "go".to_string(),
                name: "build".to_string(),
                args: "foo bar".to_string(),
                slash: Pos(90),
                args_pos: Pos(90 + "//go:build  ".len() as i64),
            }
        );
    }

    #[test]
    fn parse_directive_trailing_space() {
        let got = parse_directive(Pos(100), "//go:build foo ").unwrap();
        assert_eq!(
            got,
            Directive {
                tool: "go".to_string(),
                name: "build".to_string(),
                args: "foo".to_string(),
                slash: Pos(100),
                args_pos: Pos(100 + "//go:build ".len() as i64),
            }
        );
    }

    #[test]
    fn parse_args_simple() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "stringer -type Op".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        let got = d.parse_args().unwrap();
        assert_eq!(
            got,
            vec![
                DirectiveArg {
                    arg: "stringer".to_string(),
                    pos: Pos(10)
                },
                DirectiveArg {
                    arg: "-type".to_string(),
                    pos: Pos(10 + "stringer ".len() as i64)
                },
                DirectiveArg {
                    arg: "Op".to_string(),
                    pos: Pos(10 + "stringer -type ".len() as i64)
                },
            ]
        );
    }

    #[test]
    fn parse_args_quoted() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "\"foo bar\" baz".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        let got = d.parse_args().unwrap();
        assert_eq!(
            got,
            vec![
                DirectiveArg {
                    arg: "foo bar".to_string(),
                    pos: Pos(10)
                },
                DirectiveArg {
                    arg: "baz".to_string(),
                    pos: Pos(10 + "\"foo bar\" ".len() as i64)
                },
            ]
        );
    }

    #[test]
    fn parse_args_raw_quoted() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "`foo bar` baz".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        let got = d.parse_args().unwrap();
        assert_eq!(
            got,
            vec![
                DirectiveArg {
                    arg: "foo bar".to_string(),
                    pos: Pos(10)
                },
                DirectiveArg {
                    arg: "baz".to_string(),
                    pos: Pos(10 + "`foo bar` ".len() as i64)
                },
            ]
        );
    }

    #[test]
    fn parse_args_escapes() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "\"foo\\U0001F60Abar\" `a\\tb`".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        let got = d.parse_args().unwrap();
        assert_eq!(
            got,
            vec![
                DirectiveArg {
                    arg: "foo😊bar".to_string(),
                    pos: Pos(10)
                },
                DirectiveArg {
                    arg: "a\\tb".to_string(),
                    pos: Pos(10 + "\"foo\\U0001F60Abar\" ".len() as i64),
                },
            ]
        );
    }

    #[test]
    fn parse_args_empty() {
        let d = Directive {
            tool: "go".to_string(),
            name: "build".to_string(),
            args: "".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        assert_eq!(d.parse_args().unwrap(), vec![]);
    }

    #[test]
    fn parse_args_spaces() {
        let d = Directive {
            tool: "go".to_string(),
            name: "build".to_string(),
            args: "  foo   bar  ".to_string(),
            slash: Pos(0),
            args_pos: Pos(10),
        };
        let got = d.parse_args().unwrap();
        assert_eq!(
            got,
            vec![
                DirectiveArg {
                    arg: "foo".to_string(),
                    pos: Pos(10 + "  ".len() as i64)
                },
                DirectiveArg {
                    arg: "bar".to_string(),
                    pos: Pos(10 + "  foo   ".len() as i64)
                },
            ]
        );
    }

    #[test]
    fn parse_args_unterminated_quote() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "`foo".to_string(),
            slash: Pos(0),
            args_pos: Pos(0),
        };
        assert!(d.parse_args().is_err());
    }

    #[test]
    fn parse_args_no_space_after_quote() {
        let d = Directive {
            tool: "go".to_string(),
            name: "generate".to_string(),
            args: "\"foo\"bar".to_string(),
            slash: Pos(0),
            args_pos: Pos(0),
        };
        assert!(d.parse_args().is_err());
    }

    // Bonus: also port TestIsDirective from ast_test.go (the shared
    // table is what makes the round-trip test possible).
    #[test]
    fn is_directive_table() {
        use crate::ast::is_directive;
        for (input, want) in is_directive_tests() {
            assert_eq!(is_directive(input), want, "is_directive({:?})", input);
        }
    }
}
