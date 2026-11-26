//! `//go:embed` directives: the scan `build.readGoInfo` does over the rest of
//! a file once it has seen an `import "embed"`.
//!
//! Upstream runs a real `go/scanner` in `ScanComments` mode over the whole
//! file and feeds every `COMMENT` starting with `//go:embed` to
//! `ast.ParseDirective` + `Directive.ParseArgs`. We do the same with a
//! comment-only lexer: the only thing the scanner is used for is to tell a
//! `//` that opens a comment from one inside a string, a rune literal, or a
//! block comment.
//!
//! Positions matter as much as the patterns. `go list` reports the embed error
//! at the position of the *pattern text*, not the directive — `//go:embed
//! have.txt nope.txt` reports column 21, the `n` of `nope.txt` — because
//! `cmd/go` calls `setPos(EmbedPatternPos[pattern])`. Columns are 1-based byte
//! offsets within the line, like `token.Position`.
//!
//! Anything malformed yields no patterns at all rather than a guess: upstream
//! writes "ignore badly-formed lines - the compiler will report them when it
//! finds them", and for us the safe direction is a missing finding, never an
//! invented one (a `typecheck` issue deletes every other issue in the run).

/// One `//go:embed` pattern and where its text starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEmbed {
    pub pattern: String,
    /// 1-based line of the directive comment.
    pub line: usize,
    /// 1-based byte column of the pattern text within that line.
    pub column: usize,
}

/// Scans `content` for `//go:embed` directives.
///
/// Call only for files that import `"embed"` — that is upstream's `hasEmbed`
/// gate, and it is what keeps a `//go:embed` in a package with no `embed`
/// import out of `EmbedPatterns` entirely (`go list` reports nothing there;
/// the compiler is the one that complains).
pub fn parse_go_embeds(content: &[u8]) -> Vec<FileEmbed> {
    let s = content;
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut line_start = 0usize;

    while i < s.len() {
        match s[i] {
            b'\n' => {
                line += 1;
                i += 1;
                line_start = i;
            }
            b'/' if i + 1 < s.len() && s[i + 1] == b'/' => {
                let start = i;
                let mut end = i;
                while end < s.len() && s[end] != b'\n' {
                    end += 1;
                }
                // go/scanner drops a trailing CR from a `//` comment literal.
                let mut text_end = end;
                if text_end > start && s[text_end - 1] == b'\r' {
                    text_end -= 1;
                }
                if let Ok(text) = std::str::from_utf8(&s[start..text_end]) {
                    if text.starts_with("//go:embed") {
                        if let Some(embs) = parse_directive_embeds(text, line, start - line_start + 1)
                        {
                            out.extend(embs);
                        }
                    }
                }
                i = end;
            }
            b'/' if i + 1 < s.len() && s[i + 1] == b'*' => {
                i += 2;
                while i < s.len() {
                    if s[i] == b'\n' {
                        line += 1;
                        i += 1;
                        line_start = i;
                        continue;
                    }
                    if s[i] == b'*' && i + 1 < s.len() && s[i + 1] == b'/' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
            }
            b'"' => {
                i += 1;
                while i < s.len() && s[i] != b'"' && s[i] != b'\n' {
                    if s[i] == b'\\' && i + 1 < s.len() && s[i + 1] != b'\n' {
                        i += 1;
                    }
                    i += 1;
                }
                if i < s.len() && s[i] == b'"' {
                    i += 1;
                }
            }
            b'\'' => {
                // A rune literal never spans a line; an unterminated `'` is a
                // scan error upstream, and stopping at the newline keeps us
                // from swallowing the rest of the file.
                let mut j = i + 1;
                let mut closed = false;
                while j < s.len() && s[j] != b'\n' {
                    if s[j] == b'\\' && j + 1 < s.len() && s[j + 1] != b'\n' {
                        j += 2;
                        continue;
                    }
                    if s[j] == b'\'' {
                        closed = true;
                        j += 1;
                        break;
                    }
                    j += 1;
                }
                i = if closed { j } else { i + 1 };
            }
            b'`' => {
                i += 1;
                while i < s.len() && s[i] != b'`' {
                    if s[i] == b'\n' {
                        line += 1;
                        line_start = i + 1;
                    }
                    i += 1;
                }
                if i < s.len() {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// `ast.ParseDirective` restricted to `//go:embed`, then `ParseArgs`.
///
/// `slash_column` is the 1-based column of the leading `/`; argument columns
/// are byte offsets from there, which is what `token.Position` counts.
fn parse_directive_embeds(text: &str, line: usize, slash_column: usize) -> Option<Vec<FileEmbed>> {
    let b = text.as_bytes();
    // Fast path from ParseDirective: `//` followed by [a-z0-9].
    if b.len() < 3 || b[0] != b'/' || b[1] != b'/' || !is_alnum(b[2]) {
        return None;
    }
    let body = &text[2..];
    let colon = body.find(':')?;
    if colon == 0 || colon + 1 >= body.len() {
        return None;
    }
    let bb = body.as_bytes();
    for (i, &c) in bb.iter().enumerate().take(colon + 2) {
        if i == colon {
            continue;
        }
        if !is_alnum(c) {
            return None;
        }
    }
    if &body[..colon] != "go" {
        return None;
    }
    // Name runs to the first space; `//go:embedded x` is a different directive.
    let after_colon = &body[colon + 1..];
    let name_len = non_space_len(after_colon);
    if &after_colon[..name_len] != "embed" {
        return None;
    }
    let mut rest = &after_colon[name_len..];
    let mut off = 2 + colon + 1 + name_len;
    let lead = space_len(rest);
    rest = &rest[lead..];
    off += lead;
    let args = rest.trim_end_matches(|c: char| c.is_whitespace());

    let mut list = Vec::new();
    let mut pos = 0usize;
    loop {
        let skip = space_len(&args[pos..]);
        pos += skip;
        if pos >= args.len() {
            break;
        }
        let arg_off = off + pos;
        let head = &args[pos..];
        let arg = match head.as_bytes()[0] {
            b'`' | b'"' => {
                let (len, value) = quoted_prefix(head)?;
                pos += len;
                // A quoted argument must be followed by a space or the end.
                if let Some(c) = args[pos..].chars().next() {
                    if !c.is_whitespace() {
                        return None;
                    }
                }
                value
            }
            _ => {
                let n = non_space_len(head);
                pos += n;
                head[..n].to_string()
            }
        };
        list.push(FileEmbed {
            pattern: arg,
            line,
            column: slash_column + arg_off,
        });
    }
    Some(list)
}

fn is_alnum(b: u8) -> bool {
    b.is_ascii_lowercase() || b.is_ascii_digit()
}

fn space_len(s: &str) -> usize {
    s.len() - s.trim_start_matches(|c: char| c.is_whitespace()).len()
}

fn non_space_len(s: &str) -> usize {
    s.find(char::is_whitespace).unwrap_or(s.len())
}

/// `strconv.QuotedPrefix` + `strconv.Unquote` for the two forms a directive
/// argument may be quoted in. Returns the consumed byte length and the value.
fn quoted_prefix(s: &str) -> Option<(usize, String)> {
    match s.as_bytes()[0] {
        b'`' => {
            let end = s[1..].find('`')? + 1;
            // Unquote strips CR from raw strings.
            Some((end + 1, s[1..end].replace('\r', "")))
        }
        b'"' => scan_interpreted(s),
        _ => None,
    }
}

/// The `"`-quoted arm of [`quoted_prefix`]: consumes through the closing quote,
/// decoding escapes. An unterminated literal, or one broken by a newline, is
/// `None` — the whole directive is then dropped, as upstream drops it.
fn scan_interpreted(s: &str) -> Option<(usize, String)> {
    let mut out = String::new();
    let mut at = 1usize;
    loop {
        let c = s[at..].chars().next()?;
        match c {
            '"' => return Some((at + 1, out)),
            '\n' => return None,
            '\\' => {
                let (ch, used) = unquote_escape(&s[at..])?;
                out.push(ch);
                at += used;
            }
            _ => {
                out.push(c);
                at += c.len_utf8();
            }
        }
    }
}

/// One `\`-escape from a Go interpreted string literal: the character it
/// denotes and how many bytes it spans (including the backslash).
fn unquote_escape(s: &str) -> Option<(char, usize)> {
    let b = s.as_bytes();
    if b.len() < 2 || b[0] != b'\\' {
        return None;
    }
    let simple = |c: char| Some((c, 2));
    match b[1] {
        b'a' => simple('\u{7}'),
        b'b' => simple('\u{8}'),
        b'f' => simple('\u{c}'),
        b'n' => simple('\n'),
        b'r' => simple('\r'),
        b't' => simple('\t'),
        b'v' => simple('\u{b}'),
        b'\\' => simple('\\'),
        b'\'' => simple('\''),
        b'"' => simple('"'),
        b'x' | b'u' | b'U' => {
            let n = match b[1] {
                b'x' => 2,
                b'u' => 4,
                _ => 8,
            };
            if b.len() < 2 + n {
                return None;
            }
            let hex = s.get(2..2 + n)?;
            let v = u32::from_str_radix(hex, 16).ok()?;
            if b[1] == b'x' {
                // `\xNN` is a byte; only ASCII round-trips through a Rust char,
                // and a non-ASCII byte cannot appear in a valid UTF-8 pattern.
                if v > 0x7f {
                    return None;
                }
                return Some((v as u8 as char, 2 + n));
            }
            Some((char::from_u32(v)?, 2 + n))
        }
        b'0'..=b'7' => {
            if b.len() < 4 {
                return None;
            }
            let oct = s.get(1..4)?;
            let v = u32::from_str_radix(oct, 8).ok()?;
            if v > 0x7f {
                return None;
            }
            Some((v as u8 as char, 4))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats(src: &str) -> Vec<(String, usize, usize)> {
        parse_go_embeds(src.as_bytes())
            .into_iter()
            .map(|e| (e.pattern, e.line, e.column))
            .collect()
    }

    #[test]
    fn one_pattern_reports_the_column_of_the_pattern_text() {
        // `//go:embed ` is 11 bytes, so the pattern starts at column 12 —
        // measured against `go list -e`: `a/a.go:5:12`.
        assert_eq!(
            pats("package a\n\n//go:embed app/dist\nvar x embed.FS\n"),
            vec![("app/dist".to_string(), 3, 12)]
        );
    }

    #[test]
    fn each_pattern_on_a_line_gets_its_own_column() {
        // `go list` blames the *failing* pattern: `d/d.go:5:21` for the second.
        assert_eq!(
            pats("//go:embed have.txt nope.txt\n"),
            vec![
                ("have.txt".to_string(), 1, 12),
                ("nope.txt".to_string(), 1, 21),
            ]
        );
    }

    #[test]
    fn quoted_and_raw_arguments_are_unquoted() {
        assert_eq!(
            pats("//go:embed \"no such.txt\"\n"),
            vec![("no such.txt".to_string(), 1, 12)]
        );
        assert_eq!(
            pats("//go:embed `raw dir`\n"),
            vec![("raw dir".to_string(), 1, 12)]
        );
        assert_eq!(
            pats("//go:embed \"a\\tb\"\n"),
            vec![("a\tb".to_string(), 1, 12)]
        );
    }

    #[test]
    fn the_all_prefix_stays_part_of_the_pattern() {
        assert_eq!(
            pats("//go:embed all:hidden\n"),
            vec![("all:hidden".to_string(), 1, 12)]
        );
    }

    #[test]
    fn a_malformed_line_contributes_nothing() {
        // Unterminated quote: upstream's parseGoEmbed errors and readGoInfo
        // drops the whole comment.
        assert!(pats("//go:embed \"unterminated\n").is_empty());
        // A quoted argument not followed by space is an error, not two args.
        assert!(pats("//go:embed \"a\"b\n").is_empty());
        // Not the `embed` directive.
        assert!(pats("//go:embedded x\n").is_empty());
        assert!(pats("//go:generate stringer\n").is_empty());
        // No arguments at all.
        assert!(pats("//go:embed\n").is_empty());
        assert!(pats("//go:embed   \n").is_empty());
    }

    #[test]
    fn directives_inside_strings_and_comments_are_not_directives() {
        let src = concat!(
            "package a\n",
            "var s = \"//go:embed in-string\"\n",
            "var r = `//go:embed in-raw`\n",
            "/*\n//go:embed in-block\n*/\n",
            "var c = '/'\n",
            "//go:embed real\n",
        );
        assert_eq!(pats(src), vec![("real".to_string(), 8, 12)]);
    }

    #[test]
    fn a_raw_string_advances_the_line_counter() {
        let src = "package a\nvar r = `one\ntwo\nthree`\n//go:embed after\n";
        assert_eq!(pats(src), vec![("after".to_string(), 5, 12)]);
    }

    #[test]
    fn an_indented_directive_keeps_its_column() {
        assert_eq!(
            pats("func f() {\n\t//go:embed x\n}\n"),
            vec![("x".to_string(), 2, 13)]
        );
    }

    #[test]
    fn several_directives_are_collected_in_file_order() {
        assert_eq!(
            pats("//go:embed a\nvar x int\n//go:embed b\n"),
            vec![("a".to_string(), 1, 12), ("b".to_string(), 3, 12)]
        );
    }
}
