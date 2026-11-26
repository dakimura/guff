// Port of Go's go/doc/comment/parse.go.
//
// Original: Copyright 2022 The Go Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license.
//
// The text-level routines work on bytes rather than `char`s because upstream
// does: `parseText` advances one byte at a time and re-slices the input at
// every offset, which Rust's `&str` indexing would reject in the middle of a
// multi-byte rune. Every position that actually reaches a `String` is a rune
// boundary — see the note above `parse_text`.

use std::collections::HashMap;

use super::std_pkgs::is_std_pkg;
use super::unicode_tables::{is_digit, is_letter, is_punct, is_upper};
use super::{Block, Code, Doc, DocLink, Heading, Link, LinkDef, List, ListItem, Paragraph, Text};

/// A doc comment parser.
///
/// The fields can be filled in before calling [`Parser::parse`] to customize
/// the details of the parsing process. `go/printer` uses the zero value.
#[derive(Default)]
pub struct Parser<'a> {
    /// Go identifier words that should be italicized and potentially linked.
    /// An empty value means the word is only italicized; otherwise the value
    /// is the link target.
    pub words: HashMap<String, String>,

    /// Resolves a package name to an import path.
    ///
    /// If it returns `Some(path)` then `[name]` (or `[name.Sym]`, or
    /// `[name.Sym.Method]`) is a documentation link to `path`'s package docs.
    /// `Some("")` means the current package.
    ///
    /// `None` here is equivalent to a function that always returns `None`.
    #[allow(clippy::type_complexity)]
    pub lookup_package: Option<&'a dyn Fn(&str) -> Option<String>>,

    /// Reports whether a symbol name or method name exists in the current
    /// package. `lookup_sym("", "Name")` asks about a const/func/type/var;
    /// `lookup_sym("Recv", "Name")` asks about type `Recv`'s method `Name`.
    pub lookup_sym: Option<&'a dyn Fn(&str, &str) -> bool>,
}

/// The default package lookup, used when [`Parser::lookup_package`] is `None`
/// (upstream `DefaultLookupPackage`).
///
/// It recognizes the standard library packages with single-element import
/// paths, such as `math`, which would otherwise be impossible to name.
pub fn default_lookup_package(name: &str) -> Option<String> {
    if is_std_pkg(name) {
        Some(name.to_string())
    } else {
        None
    }
}

/// Parsing state for a single doc comment.
struct ParseDoc<'p, 'a> {
    parser: &'p Parser<'a>,
    doc: Doc,
    /// Maps a link definition's text to its index in `doc.links`. Upstream
    /// stores `*LinkDef` pointers; the index is the same aliasing, spelled in
    /// a way the borrow checker accepts.
    links: HashMap<String, usize>,
}

impl<'a> Parser<'a> {
    /// Parses the doc comment text and returns the [`Doc`] form.
    ///
    /// Comment markers (`/*`, `//` and `*/`) must already have been removed.
    pub fn parse(&self, text: &str) -> Doc {
        let lines = unindent(text.split('\n').map(str::to_string).collect());
        let mut d = ParseDoc {
            parser: self,
            doc: Doc::default(),
            links: HashMap::new(),
        };

        // First pass: break into block structure and collect known links.
        // The text is all recorded as Plain for now.
        // Upstream keeps the whole previous span; only its end is ever read.
        let mut prev_end = 0usize;
        for s in parse_spans(&lines) {
            let b = match s.kind {
                SpanKind::List => Some(Block::List(d.list(&lines[s.start..s.end], prev_end < s.start))),
                SpanKind::Code => Some(Block::Code(d.code(&lines[s.start..s.end]))),
                SpanKind::OldHeading => Some(old_heading(&lines[s.start])),
                SpanKind::Heading => Some(heading(&lines[s.start])),
                SpanKind::Para => d.paragraph(&lines[s.start..s.end]),
            };
            if let Some(b) = b {
                d.doc.content.push(b);
            }
            prev_end = s.end;
        }

        // Second pass: interpret all the Plain text now that we know the links.
        // `content` is moved out so the link table stays independently borrowable.
        let mut content = std::mem::take(&mut d.doc.content);
        for b in &mut content {
            match b {
                Block::Paragraph(p) => {
                    let raw = take_plain(&mut p.text);
                    p.text = d.parse_linked_text(&raw);
                }
                Block::List(l) => {
                    for item in &mut l.items {
                        for c in &mut item.content {
                            let Block::Paragraph(p) = c else {
                                unreachable!("list item content is always a paragraph")
                            };
                            let raw = take_plain(&mut p.text);
                            p.text = d.parse_linked_text(&raw);
                        }
                    }
                }
                _ => {}
            }
        }
        d.doc.content = content;

        d.doc
    }
}

/// Recovers the single `Plain` element the first pass left behind.
fn take_plain(text: &mut Vec<Text>) -> String {
    match text.pop() {
        Some(Text::Plain(s)) => s,
        _ => unreachable!("first pass records exactly one Plain per paragraph"),
    }
}

impl ParseDoc<'_, '_> {
    /// Looks up the `pkg` in `[pkg]`, `[pkg.Name]` and `[pkg.Name.Recv]`.
    ///
    /// A `pkg` containing a slash is assumed to be a full import path. Single
    /// element standard library names like `math` are full import paths too
    /// but contain no slash, so `lookup_package` gets first refusal (in case a
    /// different package is imported as `math`) and the built-in list of
    /// single-element standard library names answers otherwise.
    fn lookup_pkg(&self, pkg: &str) -> Option<String> {
        if pkg.contains('/') {
            // Assume a full import path.
            return if valid_import_path(pkg) {
                Some(pkg.to_string())
            } else {
                None
            };
        }
        if let Some(f) = self.parser.lookup_package {
            if let Some(path) = f(pkg) {
                return Some(path);
            }
        }
        default_lookup_package(pkg)
    }

    fn lookup_sym(&self, recv: &str, name: &str) -> bool {
        match self.parser.lookup_sym {
            Some(f) => f(recv, name),
            None => false,
        }
    }
}

// ============================================================
// Block structure
// ============================================================

/// A single span of comment lines (`lines[start..end]`) of an identified kind.
#[derive(Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    kind: SpanKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpanKind {
    Code,
    Heading,
    List,
    OldHeading,
    Para,
}

fn parse_spans(lines: &[String]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();

    // The loop may process a line twice: once as unindented and again forced
    // indented. So the maximum expected number of iterations is 2*len(lines).
    // The repeating logic can be subtle, though, and to protect against
    // introduction of infinite loops in future changes, we watch to see that
    // we are not looping too much. A panic is better than a quiet infinite
    // loop.
    let mut watchdog: isize = 2 * lines.len() as isize;

    let mut i = 0usize;
    let mut force_indent = 0usize;
    'spans: loop {
        // Skip blank lines.
        while i < lines.len() && lines[i].is_empty() {
            i += 1;
        }
        if i >= lines.len() {
            break;
        }
        watchdog -= 1;
        if watchdog < 0 {
            panic!("go/doc/comment: internal error: not making progress");
        }

        let kind;
        let start = i;
        let end;
        if i < force_indent || indented(&lines[i]) {
            // Indented (or force indented).
            // Ends before next unindented. (Blank lines are OK.)
            // If this is an unindented list that we are heuristically treating
            // as indented, then accept unindented list item lines up to the
            // first blank lines. The heuristic is disabled at blank lines to
            // contain its effect to non-gofmt'ed sections of the comment.
            let mut unindented_list_ok = is_list(&lines[i]) && i < force_indent;
            i += 1;
            while i < lines.len()
                && (lines[i].is_empty()
                    || i < force_indent
                    || indented(&lines[i])
                    || (unindented_list_ok && is_list(&lines[i])))
            {
                if lines[i].is_empty() {
                    unindented_list_ok = false;
                }
                i += 1;
            }

            // Drop trailing blank lines.
            let mut e = i;
            while e > start && lines[e - 1].is_empty() {
                e -= 1;
            }

            // If indented lines are followed (without a blank line) by an
            // unindented line ending in a brace, take that one line too. This
            // fixes the common mistake of pasting in something like
            //
            //	func main() {
            //		fmt.Println("hello, world")
            //	}
            //
            // and forgetting to indent it. The heuristic will never trigger on
            // a gofmt'ed comment, because any gofmt'ed code block or list would
            // be followed by a blank line or end of comment.
            if e < lines.len() && lines[e].starts_with('}') {
                e += 1;
            }
            end = e;

            kind = if is_list(&lines[start]) {
                SpanKind::List
            } else {
                SpanKind::Code
            };
        } else {
            // Unindented. Ends at next blank or indented line.
            i += 1;
            while i < lines.len() && !lines[i].is_empty() && !indented(&lines[i]) {
                i += 1;
            }
            let mut e = i;

            // If unindented lines are followed (without a blank line) by an
            // indented line that would start a code block, check whether the
            // final unindented lines should be left for the indented section.
            // This can happen for the common mistakes of unindented code or
            // unindented lists. The heuristic will never trigger on a gofmt'ed
            // comment, because any gofmt'ed code block would have a blank line
            // preceding it after the unindented lines.
            if i < lines.len() && !lines[i].is_empty() && !is_list(&lines[i]) {
                if is_list(&lines[i - 1]) {
                    // If the final unindented line looks like a list item,
                    // this may be the first indented line wrap of a mistakenly
                    // unindented list. Leave all the unindented list items.
                    force_indent = e;
                    e -= 1;
                    while e > start && is_list(&lines[e - 1]) {
                        e -= 1;
                    }
                } else if lines[i - 1].ends_with('{') || lines[i - 1].ends_with('\\') {
                    // If the final unindented line ended in { or \ it is
                    // probably the start of a misindented code block. Give the
                    // user a single line fix. Often that's enough; if not, the
                    // user can fix the others themselves.
                    force_indent = e;
                    e -= 1;
                }

                if start == e && force_indent > start {
                    i = start;
                    continue 'spans;
                }
            }
            end = e;

            // Span is either paragraph or heading.
            kind = if end - start == 1 && is_heading(&lines[start]) {
                SpanKind::Heading
            } else if end - start == 1 && is_old_heading(&lines[start], lines, start) {
                SpanKind::OldHeading
            } else {
                SpanKind::Para
            };
        }

        spans.push(Span { start, end, kind });
        i = end;
    }

    spans
}

/// Reports whether `line` is indented (starts with a leading space or tab).
fn indented(line: &str) -> bool {
    let b = line.as_bytes();
    !b.is_empty() && (b[0] == b' ' || b[0] == b'\t')
}

/// Removes any common space/tab prefix from each line, returning a copy in
/// which those prefixes have been trimmed. Lines containing only spaces become
/// blank lines.
fn unindent(mut lines: Vec<String>) -> Vec<String> {
    // Trim leading and trailing blank lines.
    let mut first = 0;
    while first < lines.len() && is_blank(&lines[first]) {
        first += 1;
    }
    let mut last = lines.len();
    while last > first && is_blank(&lines[last - 1]) {
        last -= 1;
    }
    if first == last {
        return Vec::new();
    }
    lines = lines[first..last].to_vec();

    // Compute and remove common indentation.
    let mut prefix = leading_space(&lines[0]).to_string();
    for line in &lines[1..] {
        if !is_blank(line) {
            prefix = common_prefix(&prefix, leading_space(line)).to_string();
        }
    }

    let mut out: Vec<String> = lines
        .iter()
        .map(|line| {
            let line = line.strip_prefix(prefix.as_str()).unwrap_or(line);
            if line.trim().is_empty() {
                String::new()
            } else {
                line.to_string()
            }
        })
        .collect();
    while !out.is_empty() && out[0].is_empty() {
        out.remove(0);
    }
    while !out.is_empty() && out[out.len() - 1].is_empty() {
        out.pop();
    }
    out
}

/// Reports whether `s` is a blank line.
fn is_blank(s: &str) -> bool {
    s.is_empty() || s == "\n"
}

/// The longest common prefix of `a` and `b` (both are all spaces and tabs).
fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < ab.len() && i < bb.len() && ab[i] == bb[i] {
        i += 1;
    }
    &a[..i]
}

/// The longest prefix of `s` consisting of spaces and tabs.
fn leading_space(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() && (b[i] == b' ' || b[i] == b'\t') {
        i += 1;
    }
    &s[..i]
}

/// Reports whether `line` (which is `all[off]`) is an old-style section
/// heading.
fn is_old_heading(line: &str, all: &[String], off: usize) -> bool {
    if off == 0
        || !all[off - 1].is_empty()
        || off + 2 >= all.len()
        || !all[off + 1].is_empty()
        || !leading_space(&all[off + 2]).is_empty()
    {
        return false;
    }

    let line = line.trim();

    // A heading must start with an uppercase letter.
    match line.chars().next() {
        Some(r) if is_letter(r) && is_upper(r) => {}
        _ => return false,
    }

    // It must end in a letter or digit.
    match line.chars().next_back() {
        Some(r) if is_letter(r) || is_digit(r) => {}
        _ => return false,
    }

    // Exclude lines with illegal characters. We allow "(),".
    const ILLEGAL: &str = ";:!?+*/=[]{}_^°&§~%#@<\">\\";
    if line.chars().any(|c| ILLEGAL.contains(c)) {
        return false;
    }

    // Allow "'" for possessive "'s" only.
    let mut b = line;
    while let Some((_, rest)) = b.split_once('\'') {
        if rest != "s" && !rest.starts_with("s ") {
            return false; // ' not followed by s and then end-of-word
        }
        b = rest;
    }

    // Allow "." when followed by non-space.
    let mut b = line;
    while let Some((_, rest)) = b.split_once('.') {
        if rest.is_empty() || rest.starts_with(' ') {
            return false; // not followed by non-space
        }
        b = rest;
    }

    true
}

/// The [`Heading`] for the given old-style section heading line.
fn old_heading(line: &str) -> Block {
    Block::Heading(Heading {
        text: vec![Text::Plain(line.trim().to_string())],
    })
}

/// Reports whether `line` is a new-style section heading.
fn is_heading(line: &str) -> bool {
    let b = line.as_bytes();
    b.len() >= 2 && b[0] == b'#' && (b[1] == b' ' || b[1] == b'\t') && line.trim() != "#"
}

/// The [`Heading`] for the given new-style section heading line.
fn heading(line: &str) -> Block {
    Block::Heading(Heading {
        text: vec![Text::Plain(line[1..].trim().to_string())],
    })
}

impl ParseDoc<'_, '_> {
    /// A code block built from the lines.
    fn code(&self, lines: &[String]) -> Code {
        let mut body = unindent(lines.to_vec());
        body.push(String::new()); // to get the final \n from join
        Code {
            text: body.join("\n"),
        }
    }

    /// A paragraph block built from the lines. If the lines are link
    /// definitions, they are added to the doc and `None` is returned.
    fn paragraph(&mut self, lines: &[String]) -> Option<Block> {
        // Is this a block of known links? Handle.
        let mut defs = Vec::new();
        let mut all_defs = true;
        for line in lines {
            match parse_link(line) {
                Some(def) => defs.push(def),
                None => {
                    all_defs = false;
                    break;
                }
            }
        }
        if all_defs {
            for def in defs {
                let text = def.text.clone();
                self.doc.links.push(def);
                let idx = self.doc.links.len() - 1;
                self.links.entry(text).or_insert(idx);
            }
            return None;
        }

        Some(Block::Paragraph(Paragraph {
            text: vec![Text::Plain(lines.join("\n"))],
        }))
    }
}

/// Parses a single link definition line:
///
/// ```text
/// [text]: url
/// ```
fn parse_link(line: &str) -> Option<LinkDef> {
    let b = line.as_bytes();
    if b.is_empty() || b[0] != b'[' {
        return None;
    }
    let i = line.find("]:")?;
    if i + 3 >= b.len() || (b[i + 2] != b' ' && b[i + 2] != b'\t') {
        return None;
    }

    let text = &line[1..i];
    let url = line[i + 3..].trim();
    let j = url.find("://")?;
    if !is_scheme(&url[..j]) {
        return None;
    }

    // Line has the right form and a valid scheme://. That is good enough for
    // us — we are not as picky about the characters beyond the :// as we are
    // when extracting inline URLs from text.
    Some(LinkDef {
        text: text.to_string(),
        url: url.to_string(),
        used: false,
    })
}

impl ParseDoc<'_, '_> {
    /// A list built from the indented lines, using `force_blank_before` as the
    /// value of the list's `force_blank_before` field.
    fn list(&mut self, lines: &[String], force_blank_before: bool) -> List {
        let num = list_marker(&lines[0]).map(|(n, _)| n).unwrap_or_default();

        let mut list = List {
            items: Vec::new(),
            force_blank_before,
            force_blank_between: false,
        };
        // The paragraph for the item currently being accumulated. `None` means
        // no item has been started, which is upstream's nil `item`.
        let mut have_item = false;
        let mut text: Vec<String> = Vec::new();

        for line in lines {
            let mut line: &str = line;
            if let Some((n, after)) = list_marker(line) {
                if n.is_empty() == num.is_empty() {
                    // Start a new list item.
                    flush_item(self, &mut list, have_item, &mut text);
                    list.items.push(ListItem {
                        number: n,
                        content: Vec::new(),
                    });
                    have_item = true;
                    line = after;
                }
            }
            line = line.trim();
            if line.is_empty() {
                list.force_blank_between = true;
                flush_item(self, &mut list, have_item, &mut text);
                continue;
            }
            text.push(line.trim().to_string());
        }
        flush_item(self, &mut list, have_item, &mut text);
        list
    }
}

/// Upstream's closure `flush`: attach the accumulated text to the current item.
fn flush_item(d: &mut ParseDoc<'_, '_>, list: &mut List, have_item: bool, text: &mut Vec<String>) {
    if have_item {
        if let Some(para) = d.paragraph(text) {
            list.items.last_mut().expect("item exists").content.push(para);
        }
    }
    text.clear();
}

/// Parses the line as beginning with a list marker. On success returns the
/// numeric marker (empty for a bullet list) and the rest of the line.
///
/// The returned rest is a slice of the *trimmed* line, matching upstream.
fn list_marker(line: &str) -> Option<(String, &str)> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Can we find a marker?
    let first = line.chars().next().expect("non-empty");
    let (num, rest): (String, &str) = if matches!(first, '•' | '*' | '+' | '-') {
        (String::new(), &line[first.len_utf8()..])
    } else if line.as_bytes()[0].is_ascii_digit() {
        let b = line.as_bytes();
        let mut n = 1;
        while n < b.len() && b[n].is_ascii_digit() {
            n += 1;
        }
        if n >= b.len() || (b[n] != b'.' && b[n] != b')') {
            return None;
        }
        (line[..n].to_string(), &line[n + 1..])
    } else {
        return None;
    };

    if !indented(rest) || rest.trim().is_empty() {
        return None;
    }

    Some((num, rest))
}

/// Reports whether the line is the first line of a list, meaning it starts
/// with a list marker after any indentation. (The caller is responsible for
/// checking the line is indented, as appropriate.)
fn is_list(line: &str) -> bool {
    list_marker(line).is_some()
}

// ============================================================
// Text
// ============================================================

impl ParseDoc<'_, '_> {
    /// Parses text that is allowed to contain explicit links, such as
    /// `[math.Sin]` or `[Go home page]`, into a slice of [`Text`] items.
    ///
    /// A “pkg” is only assumed to be a full import path if it starts with a
    /// domain name (a path element with a dot) or is one of the packages from
    /// the standard library (`[os]`, `[encoding/json]`, and so on). To avoid
    /// problems with maps, generics and array types, doc links must be both
    /// preceded and followed by punctuation, spaces, tabs, or the start or end
    /// of a line. An example problem would be treating `map[ast.Expr]Type` as
    /// containing a link.
    fn parse_linked_text(&mut self, text: &str) -> Vec<Text> {
        let mut out: Vec<Text> = Vec::new();
        let mut wrote = 0usize;
        let bytes = text.as_bytes();

        let mut start: isize = -1;
        let mut buf: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < bytes.len() {
            let mut c = bytes[i];
            if c == b'\n' || c == b'\t' {
                c = b' ';
            }
            match c {
                b'[' => start = i as isize,
                b']' => {
                    if start >= 0 {
                        let s = start as usize;
                        let key = String::from_utf8_lossy(&buf).into_owned();
                        let known = self.links.get(&key).copied();
                        if let Some(idx) = known {
                            self.doc.links[idx].used = true;
                            let url = self.doc.links[idx].url.clone();
                            if wrote < s {
                                out = self.parse_text(out, &text[wrote..s], true);
                            }
                            let inner = self.parse_text(Vec::new(), &text[s + 1..i], false);
                            out.push(Text::Link(Link {
                                auto: false,
                                text: inner,
                                url,
                            }));
                            wrote = i + 1;
                        } else if let Some(mut link) =
                            self.doc_link(&text[s + 1..i], &text[..s], &text[i + 1..])
                        {
                            if wrote < s {
                                out = self.parse_text(out, &text[wrote..s], true);
                            }
                            link.text = self.parse_text(Vec::new(), &text[s + 1..i], false);
                            out.push(Text::DocLink(link));
                            wrote = i + 1;
                        }
                    }
                    start = -1;
                    buf.clear();
                }
                _ => {}
            }
            if start >= 0 && i != start as usize {
                buf.push(c);
            }
            i += 1;
        }

        if wrote < bytes.len() {
            out = self.parse_text(out, &text[wrote..], true);
        }
        out
    }

    /// Parses `text`, which was found inside `[ ]` brackets, as a doc link if
    /// possible. `before` and `after` are the text before the `[` and after
    /// the `]` on the same line: doc links must be preceded and followed by
    /// punctuation, spaces, tabs, or the start or end of a line.
    fn doc_link(&self, text: &str, before: &str, after: &str) -> Option<DocLink> {
        if !before.is_empty() {
            let r = before.chars().next_back().expect("non-empty");
            if !is_punct(r) && r != ' ' && r != '\t' && r != '\n' {
                return None;
            }
        }
        if !after.is_empty() {
            let r = after.chars().next().expect("non-empty");
            if !is_punct(r) && r != ' ' && r != '\t' && r != '\n' {
                return None;
            }
        }
        let text = text.strip_prefix('*').unwrap_or(text);
        let (pkg, name, ok) = split_doc_name(text);
        let mut pkg = pkg.to_string();
        let mut recv = String::new();
        if ok {
            let (p, r, _) = split_doc_name(&pkg);
            let (p, r) = (p.to_string(), r.to_string());
            pkg = p;
            recv = r;
        }
        if !pkg.is_empty() {
            pkg = self.lookup_pkg(&pkg)?;
        } else if !self.lookup_sym(&recv, name) {
            return None;
        }
        Some(DocLink {
            text: Vec::new(),
            import_path: pkg,
            recv,
            name: name.to_string(),
        })
    }
}

/// If `text` is of the form `before.Name`, where `Name` is a capitalized Go
/// identifier, returns `(before, name, true)`. Otherwise `(text, "", false)`.
fn split_doc_name(text: &str) -> (&str, &str, bool) {
    let i = text.rfind('.');
    let name = match i {
        Some(i) => &text[i + 1..],
        None => text,
    };
    if !is_name(name) {
        return (text, "", false);
    }
    let before = match i {
        Some(i) => &text[..i],
        None => "",
    };
    (before, name, true)
}

impl ParseDoc<'_, '_> {
    /// Parses `s` as text and appends the parsed [`Text`] elements to `out`.
    ///
    /// This does not handle explicit links like `[math.Sin]` or
    /// `[Go home page]`: those are handled by [`ParseDoc::parse_linked_text`].
    /// If `auto_link` is true, URLs and words from `words` become links.
    ///
    /// Upstream walks `s` one byte at a time and re-slices at every offset.
    /// Every offset that reaches a `String` here is still a rune boundary: the
    /// `` `` `` and `''` arms only fire on ASCII bytes (a continuation byte is
    /// ≥ 0x80, so it can never look like a backtick or a quote), and the
    /// `auto_url` / `ident` arms consume whole runes.
    fn parse_text(&self, mut out: Vec<Text>, s: &str, auto_link: bool) -> Vec<Text> {
        let bytes = s.as_bytes();
        let mut w: Vec<u8> = Vec::new();
        let mut wrote = 0usize;
        let mut i = 0usize;
        while i < bytes.len() {
            let t = &bytes[i..];
            if auto_link {
                if let Some(url) = auto_url(t) {
                    // flush(i)
                    w.extend_from_slice(&bytes[wrote..i]);
                    if !w.is_empty() {
                        out.push(Text::Plain(bytes_to_string(&w)));
                        w.clear();
                    }
                    // Note: the old comment parser would look up the URL in
                    // words and replace the target with words[URL] if it was
                    // non-empty. That would allow creating links that display
                    // as one URL but when clicked go to a different URL. Not
                    // sure what the point of that is, so we're not doing that
                    // lookup here.
                    let url_len = url.len();
                    let url = bytes_to_string(url);
                    out.push(Text::Link(Link {
                        auto: true,
                        text: vec![Text::Plain(url.clone())],
                        url,
                    }));
                    i += url_len;
                    wrote = i;
                    continue;
                }
                if let Some(id) = ident(t) {
                    let id_len = id.len();
                    let id = bytes_to_string(id);
                    let Some(url) = self.parser.words.get(&id) else {
                        i += id_len;
                        continue;
                    };
                    let url = url.clone();
                    // flush(i)
                    w.extend_from_slice(&bytes[wrote..i]);
                    if !w.is_empty() {
                        out.push(Text::Plain(bytes_to_string(&w)));
                        w.clear();
                    }
                    if url.is_empty() {
                        out.push(Text::Italic(id));
                    } else {
                        out.push(Text::Link(Link {
                            auto: true,
                            text: vec![Text::Italic(id)],
                            url,
                        }));
                    }
                    i += id_len;
                    wrote = i;
                    continue;
                }
            }
            if t.starts_with(b"``") {
                if t.len() >= 3 && t[2] == b'`' {
                    // Do not convert `` inside ```, in case people are
                    // mistakenly writing Markdown.
                    i += 3;
                    while i < t.len() && t[i] == b'`' {
                        i += 1;
                    }
                    continue;
                }
                w.extend_from_slice(&bytes[wrote..i]);
                w.extend_from_slice("\u{201c}".as_bytes());
                i += 2;
                wrote = i;
            } else if t.starts_with(b"''") {
                w.extend_from_slice(&bytes[wrote..i]);
                w.extend_from_slice("\u{201d}".as_bytes());
                i += 2;
                wrote = i;
            } else {
                i += 1;
            }
        }
        // flush(len(s))
        w.extend_from_slice(&bytes[wrote..]);
        if !w.is_empty() {
            out.push(Text::Plain(bytes_to_string(&w)));
        }
        out
    }
}

/// Upstream slices a `string`, so the bytes are always valid UTF-8; the lossy
/// fallback only exists so a future caller cannot turn a bug into a panic.
fn bytes_to_string(b: &[u8]) -> String {
    match std::str::from_utf8(b) {
        Ok(s) => s.to_string(),
        Err(_) => String::from_utf8_lossy(b).into_owned(),
    }
}

// ============================================================
// URLs and identifiers
// ============================================================

/// Checks whether `s` begins with a URL that should be hyperlinked. On success
/// returns the URL, which is a prefix of `s`. The caller should skip over the
/// first `url.len()` bytes before further processing.
fn auto_url(s: &[u8]) -> Option<&[u8]> {
    // Find the ://. Fast path to pick off non-URL, since we call this at every
    // position in the string. The shortest possible URL is ftp://x, 7 bytes.
    if s.len() < 7 {
        return None;
    }
    let mut i = if s[3] == b':' {
        3
    } else if s[4] == b':' {
        4
    } else if s[5] == b':' {
        5
    } else if s[6] == b':' {
        6
    } else {
        return None;
    };
    if i + 3 > s.len() || &s[i..i + 3] != b"://" {
        return None;
    }

    // Check valid scheme.
    if !is_scheme(std::str::from_utf8(&s[..i]).ok()?) {
        return None;
    }

    // Scan host part. Must have at least one byte, and must start and end in
    // non-punctuation.
    i += 3;
    if i >= s.len() || !is_host(s[i]) || is_punct_byte(s[i]) {
        return None;
    }
    i += 1;
    let mut end = i;
    while i < s.len() && is_host(s[i]) {
        if !is_punct_byte(s[i]) {
            end = i + 1;
        }
        i += 1;
    }
    i = end;

    // At this point we are definitely returning a URL (scheme://host). We just
    // have to find the longest path we can add to it. Heuristics abound. We
    // allow parens, braces and brackets, but only if they match (#5043,
    // #22285). We allow .,:;?! in the path but not at the end, to avoid
    // end-of-sentence punctuation (#18139, #16565).
    let mut stk: Vec<u8> = Vec::new();
    end = i;
    while i < s.len() {
        if is_punct_byte(s[i]) {
            i += 1;
            continue;
        }
        if !is_path(s[i]) {
            break;
        }
        match s[i] {
            b'(' => stk.push(b')'),
            b'{' => stk.push(b'}'),
            b'[' => stk.push(b']'),
            b')' | b'}' | b']' => {
                if stk.last() != Some(&s[i]) {
                    break;
                }
                stk.pop();
            }
            _ => {}
        }
        if stk.is_empty() {
            end = i + 1;
        }
        i += 1;
    }

    Some(&s[..end])
}

/// Reports whether `s` is a recognized URL scheme. Note that if strings of new
/// length (beyond 3–7) are added here, the fast path at the top of
/// [`auto_url`] will need updating.
fn is_scheme(s: &str) -> bool {
    matches!(
        s,
        "file" | "ftp" | "gopher" | "http" | "https" | "mailto" | "nntp"
    )
}

/// Reports whether `c` is a byte that can appear in a URL host, like
/// `www.example.com` or `user@[::1]:8080`.
fn is_host(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'_' | b'@' | b'-' | b'.' | b'[' | b']' | b':')
}

/// Reports whether `c` is a punctuation byte that can appear inside a path but
/// not at the end. (Upstream calls this `isPunct`; renamed here to keep it
/// apart from `unicode.IsPunct`, which the parser also uses.)
fn is_punct_byte(c: u8) -> bool {
    matches!(c, b'.' | b',' | b':' | b';' | b'?' | b'!')
}

/// Reports whether `c` is a (non-punctuation) path byte.
fn is_path(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'$' | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b'&'
                | b'#'
                | b'='
                | b'@'
                | b'~'
                | b'_'
                | b'/'
                | b'-'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'%'
        )
}

/// Reports whether `s` is a capitalized Go identifier (like `Name`).
fn is_name(s: &str) -> bool {
    match ident(s.as_bytes()) {
        Some(t) if t == s.as_bytes() => {}
        _ => return false,
    }
    s.chars().next().is_some_and(is_upper)
}

/// Checks whether `s` begins with a Go identifier — `[\pL_][\pL_0-9]*`. On
/// success returns the identifier, which is a prefix of `s`.
fn ident(s: &[u8]) -> Option<&[u8]> {
    let mut n = 0usize;
    while n < s.len() {
        let c = s[n];
        if c < 0x80 {
            if is_ident_ascii(c) && (n > 0 || !c.is_ascii_digit()) {
                n += 1;
                continue;
            }
            break;
        }
        match decode_rune(&s[n..]) {
            Some((r, nr)) if is_letter(r) => n += nr,
            _ => break,
        }
    }
    if n > 0 {
        Some(&s[..n])
    } else {
        None
    }
}

/// Reports whether `c` is an ASCII identifier byte.
fn is_ident_ascii(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// `utf8.DecodeRune`, restricted to what [`ident`] needs: a decode failure is
/// reported as `None` rather than as Go's `(RuneError, 1)`, because the only
/// caller treats a non-letter and a decode error the same way.
fn decode_rune(s: &[u8]) -> Option<(char, usize)> {
    let len = match s[0] {
        0xC2..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF4 => 4,
        _ => return None,
    };
    if s.len() < len {
        return None;
    }
    let c = std::str::from_utf8(&s[..len]).ok()?.chars().next()?;
    Some((c, len))
}

// ============================================================
// Import paths
// ============================================================

/// Reports whether `path` is a valid import path. Lightly edited copy of
/// `golang.org/x/mod/module.CheckImportPath`.
fn valid_import_path(path: &str) -> bool {
    // A Rust `&str` is UTF-8 by construction, so upstream's utf8.ValidString
    // check cannot fail here.
    if path.is_empty() || path.starts_with('-') || path.contains("//") || path.ends_with('/') {
        return false;
    }
    path.split('/').all(valid_import_path_elem)
}

fn valid_import_path_elem(elem: &str) -> bool {
    if elem.is_empty() || elem.starts_with('.') || elem.ends_with('.') {
        return false;
    }
    elem.bytes().all(import_path_ok)
}

fn import_path_ok(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.' | b'~' | b'_' | b'+')
}
