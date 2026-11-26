//! Port of go-critic `badRegexp` (suspicious regexp patterns).
//!
//! Uses `regex-syntax` AST (Go RE2-like) instead of quasilyte/regex/syntax.
//! Full dangling-anchor / flag edge-case parity with upstream is DEFERRED.
//!
//! The two parsers do not agree about escapes. quasilyte's lexer turns **any**
//! `\X` into one `tokEscapeChar`/`tokEscapeMeta` token, while `regex-syntax`
//! knows a fixed set — and reads `\<` / `\>` as Rust's own word-boundary
//! assertions, which inside a character class is a parse error. A parse error
//! here means the pattern is skipped in silence, so one `\<` used to hide
//! every finding in its regexp. [`Pat`] rewrites those two escapes away and
//! keeps an offset map so the messages still quote the source text.

use std::cmp::Ordering;
use std::collections::HashSet;

use regex_syntax::ast::parse::ParserBuilder;
use regex_syntax::ast::{
    AssertionKind, Ast, ClassPerl, ClassPerlKind, ClassSet, ClassSetItem, Flag, Flags,
    FlagsItemKind, GroupKind, LiteralKind, RepetitionKind, Span,
};

const MAX_RUNE: u32 = 0x10FFFF;

#[derive(Clone)]
struct CharRange {
    low: u32,
    high: u32,
    source: String,
}

/// A pattern in two forms: the source text every message quotes, and the
/// rewritten text `regex-syntax` can parse.
///
/// The only rewrite is dropping the backslash of `\<` and `\>`, which
/// go-critic's parser reads as the literal characters and Rust's reads as
/// word-boundary assertions. `map` carries each `san` byte offset back to the
/// `orig` offset it came from, so a span over the rewritten `<` still slices
/// `\<` out of the source.
struct Pat<'a> {
    orig: &'a str,
    san: String,
    /// `san` byte offset -> `orig` byte offset. Length is `san.len() + 1`.
    map: Vec<usize>,
    /// `san` byte offsets of characters that lost a backslash.
    escaped: HashSet<usize>,
}

fn sanitize(orig: &str) -> Pat<'_> {
    let mut san = String::with_capacity(orig.len());
    let mut map: Vec<usize> = Vec::with_capacity(orig.len() + 1);
    let mut escaped = HashSet::new();
    let mut push = |san: &mut String, map: &mut Vec<usize>, at: usize, ch: char| {
        let before = san.len();
        san.push(ch);
        map.resize(san.len(), at);
        debug_assert!(map.len() > before);
    };
    let mut it = orig.char_indices().peekable();
    while let Some((i, ch)) = it.next() {
        if ch == '\\' {
            if let Some(&(_, c2)) = it.peek() {
                it.next();
                if c2 == '<' || c2 == '>' {
                    escaped.insert(san.len());
                    // Both bytes of `\<` map to the backslash, so the slice of
                    // the rewritten character is the whole escape.
                    push(&mut san, &mut map, i, c2);
                    continue;
                }
                // Copy the escape whole, so a `\\` can never be mistaken for
                // the start of one.
                push(&mut san, &mut map, i, ch);
                push(&mut san, &mut map, i + ch.len_utf8(), c2);
                continue;
            }
        }
        push(&mut san, &mut map, i, ch);
    }
    map.push(orig.len());
    Pat {
        orig,
        san,
        map,
        escaped,
    }
}

/// Analyze a regexp pattern string and return diagnostic messages
/// (without the `badRegexp: ` prefix — caller adds context).
pub fn check_pattern(pat: &str) -> Vec<String> {
    let pat = sanitize(pat);
    let mut parser = ParserBuilder::new().octal(true).build();
    let Ok(ast) = parser.parse(&pat.san) else {
        return Vec::new();
    };
    let pat = &pat;
    let mut msgs = Vec::new();
    let mut good_carets: Vec<Span> = Vec::new();
    mark_good_carets(pat, &ast, &mut good_carets);
    let mut flag_states: Vec<[bool; 128]> = vec![[false; 128]];
    walk(
        pat,
        &ast,
        &mut msgs,
        &mut flag_states,
        &good_carets,
    );
    msgs
}

fn slice<'a>(pat: &'a Pat<'_>, span: &Span) -> &'a str {
    slice_offsets(pat, span.start.offset, span.end.offset)
}

/// `slice` for a span synthesized from two node offsets.
fn slice_offsets<'a>(pat: &'a Pat<'_>, start: usize, end: usize) -> &'a str {
    let start = start.min(pat.san.len());
    let end = end.min(pat.san.len()).max(start);
    &pat.orig[pat.map[start]..pat.map[end]]
}

fn can_skip(e: &Ast) -> bool {
    match e {
        Ast::Flags(_) => true,
        Ast::Group(g) => match g.ast.as_ref() {
            Ast::Concat(c) if c.asts.is_empty() => true,
            Ast::Empty(_) => true,
            _ => false,
        },
        Ast::Empty(_) => true,
        _ => false,
    }
}

fn mark_good_carets(pat: &Pat<'_>, e: &Ast, out: &mut Vec<Span>) {
    if let Ast::Concat(c) = e {
        if c.asts.len() > 1 {
            let mut i = 0;
            while i < c.asts.len() && can_skip(&c.asts[i]) {
                i += 1;
            }
            if i < c.asts.len() {
                mark_good_carets(pat, &c.asts[i], out);
            }
            return;
        }
    }
    if let Ast::Assertion(a) = e {
        if a.kind == AssertionKind::StartLine {
            out.push(a.span);
        }
    }
    match e {
        Ast::Group(g) => mark_good_carets(pat, &g.ast, out),
        Ast::Repetition(r) => mark_good_carets(pat, &r.ast, out),
        Ast::Alternation(alt) => {
            for a in &alt.asts {
                mark_good_carets(pat, a, out);
            }
        }
        Ast::Concat(c) => {
            for a in &c.asts {
                mark_good_carets(pat, a, out);
            }
        }
        _ => {}
    }
}

fn is_good_anchor(good: &[Span], span: &Span) -> bool {
    good.iter().any(|g| g == span)
}

fn walk(
    pat: &Pat<'_>,
    e: &Ast,
    msgs: &mut Vec<String>,
    flag_states: &mut Vec<[bool; 128]>,
    good_carets: &[Span],
) {
    match e {
        Ast::Alternation(alt) => {
            check_alt_anchor(pat, alt, msgs);
            check_alt_dups(pat, alt, msgs);
            for a in &alt.asts {
                walk(pat, a, msgs, flag_states, good_carets);
            }
        }
        Ast::ClassBracketed(cc) => {
            if check_char_class_ranges(pat, cc, msgs) {
                check_char_class_dups(pat, cc, msgs);
            }
        }
        Ast::Repetition(r)
            if matches!(
                r.op.kind,
                RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore
            ) && r.greedy =>
        {
            check_nested_quantifier(pat, r, msgs);
            walk(pat, &r.ast, msgs, flag_states, good_carets);
        }
        Ast::Flags(sf) => {
            update_flag_state(pat, flag_states.last_mut().unwrap(), &sf.flags, &sf.span, msgs);
        }
        Ast::Group(g) => {
            let nflags = flag_states.len();
            flag_states.push(*flag_states.last().unwrap());
            if let GroupKind::NonCapturing(ref flags) = g.kind {
                if !flags.items.is_empty() {
                    update_flag_state(
                        pat,
                        flag_states.last_mut().unwrap(),
                        flags,
                        &flags.span,
                        msgs,
                    );
                }
            }
            walk(pat, &g.ast, msgs, flag_states, good_carets);
            flag_states.truncate(nflags);
        }
        Ast::Assertion(a) if a.kind == AssertionKind::StartLine => {
            if !is_good_anchor(good_carets, &a.span) {
                msgs.push("dangling or redundant ^, maybe \\^ is intended?".to_string());
            }
        }
        Ast::Repetition(r) => walk(pat, &r.ast, msgs, flag_states, good_carets),
        Ast::Concat(c) => {
            for a in &c.asts {
                walk(pat, a, msgs, flag_states, good_carets);
            }
        }
        _ => {}
    }
}

fn flag_char(f: Flag) -> Option<u8> {
    Some(match f {
        Flag::CaseInsensitive => b'i',
        Flag::MultiLine => b'm',
        Flag::DotMatchesNewLine => b's',
        Flag::IgnoreWhitespace => b'x',
        Flag::Unicode => b'u',
        Flag::SwapGreed => b'U',
        Flag::CRLF => return None,
    })
}

fn update_flag_state(
    pat: &Pat<'_>,
    state: &mut [bool; 128],
    flags: &Flags,
    span: &Span,
    msgs: &mut Vec<String>,
) {
    let flag_str = slice(pat, span);
    // Prefer original go-critic message which uses the group value including `(?…)`.
    let display_full = if flag_str.is_empty() {
        slice(pat, span)
    } else {
        flag_str
    };

    let mut clearing = false;
    for item in &flags.items {
        match &item.kind {
            FlagsItemKind::Negation => clearing = true,
            FlagsItemKind::Flag(f) => {
                let Some(ch) = flag_char(*f) else { continue };
                let idx = ch as usize;
                if clearing {
                    if !state[idx] {
                        msgs.push(format!(
                            "clearing unset flag {} in {}",
                            ch as char, display_full
                        ));
                    }
                } else if state[idx] {
                    msgs.push(format!("redundant flag {} in {}", ch as char, display_full));
                }
                state[idx] = !clearing;
            }
        }
    }
}

fn check_nested_quantifier(
    pat: &Pat<'_>,
    r: &regex_syntax::ast::Repetition,
    msgs: &mut Vec<String>,
) {
    let mut x = r.ast.as_ref();
    if let Ast::Group(g) = x {
        x = g.ast.as_ref();
    }
    if let Ast::Repetition(inner) = x {
        if matches!(
            inner.op.kind,
            RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore
        ) && inner.greedy
        {
            msgs.push(format!(
                "repeated greedy quantifier in {}",
                slice(pat, &r.span)
            ));
        }
    }
}

fn check_alt_dups(pat: &Pat<'_>, alt: &regex_syntax::ast::Alternation, msgs: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    for a in &alt.asts {
        let v = slice(pat, a.span()).to_string();
        if !seen.insert(v.clone()) {
            msgs.push(format!("`{v}` is duplicated in {}", slice(pat, &alt.span)));
        }
    }
}

fn is_char_or_lit(e: &Ast) -> bool {
    match e {
        Ast::Literal(_) => true,
        // quasilyte OpLiteral can span multiple chars; regex-syntax splits them.
        Ast::Concat(c) if !c.asts.is_empty() && c.asts.iter().all(|a| matches!(a, Ast::Literal(_))) => {
            true
        }
        _ => false,
    }
}

/// If `e` is `^` followed by a literal/literal-seq (possibly as Concat), return
/// the span of the part after `^`.
fn caret_prefix_lit<'a>(pat: &'a Pat<'_>, e: &'a Ast) -> Option<&'a str> {
    match e {
        Ast::Concat(c) if c.asts.len() >= 2 => {
            let first = &c.asts[0];
            if !matches!(
                first,
                Ast::Assertion(ref a) if a.kind == AssertionKind::StartLine
            ) {
                return None;
            }
            let rest = if c.asts.len() == 2 {
                &c.asts[1]
            } else {
                // Synthesize: treat remaining as literal-seq if all literals.
                if !c.asts[1..].iter().all(|a| matches!(a, Ast::Literal(_))) {
                    return None;
                }
                // Span from second start to end.
                let start = c.asts[1].span().start.offset;
                let end = c.span.end.offset;
                return Some(slice_offsets(pat, start, end));
            };
            if is_char_or_lit(rest) {
                Some(slice(pat, rest.span()))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn dollar_suffix_lit<'a>(pat: &'a Pat<'_>, e: &'a Ast) -> Option<&'a str> {
    match e {
        Ast::Concat(c) if c.asts.len() >= 2 => {
            let last = &c.asts[c.asts.len() - 1];
            if !matches!(
                last,
                Ast::Assertion(ref a) if a.kind == AssertionKind::EndLine
            ) {
                return None;
            }
            if c.asts.len() == 2 {
                let first = &c.asts[0];
                if is_char_or_lit(first) {
                    return Some(slice(pat, first.span()));
                }
                return None;
            }
            if !c.asts[..c.asts.len() - 1]
                .iter()
                .all(|a| matches!(a, Ast::Literal(_)))
            {
                return None;
            }
            let start = c.asts[0].span().start.offset;
            let end = c.asts[c.asts.len() - 2].span().end.offset;
            Some(slice_offsets(pat, start, end))
        }
        _ => None,
    }
}

fn check_alt_anchor(pat: &Pat<'_>, alt: &regex_syntax::ast::Alternation, msgs: &mut Vec<String>) {
    if alt.asts.is_empty() {
        return;
    }
    // Case 1: ^foo|bar|baz
    if let Some(without_caret) = caret_prefix_lit(pat, &alt.asts[0]) {
        let matched = alt.asts[1..].iter().all(is_char_or_lit);
        if matched {
            msgs.push(format!(
                "^ applied only to `{without_caret}` in {}",
                slice(pat, &alt.span)
            ));
        }
    }

    // Case 2: foo|bar|baz$
    let last = &alt.asts[alt.asts.len() - 1];
    if let Some(without_dollar) = dollar_suffix_lit(pat, last) {
        let matched = alt.asts[..alt.asts.len() - 1]
            .iter()
            .all(is_char_or_lit);
        if matched {
            msgs.push(format!(
                "$ applied only to `{without_dollar}` in {}",
                slice(pat, &alt.span)
            ));
        }
    }
}

fn is_letter_or_digit(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch.is_ascii_digit()
}

fn check_char_class_ranges(
    pat: &Pat<'_>,
    cc: &regex_syntax::ast::ClassBracketed,
    msgs: &mut Vec<String>,
) -> bool {
    let Some(items) = class_items(&cc.kind) else {
        return false;
    };
    for item in items {
        if let ClassSetItem::Range(r) = item {
            // Upstream reads the **low** bound only — `e.Args[0]` — and it
            // reads it twice, for two different decisions:
            //
            //     switch e.Args[0].Op {
            //     case syntax.OpEscapeOctal, syntax.OpEscapeHex:
            //         continue
            //     }
            //     ch := c.charClassBoundRune(e.Args[0])
            //     if ch == 0 { return false }
            //
            // so a hex/octal low bound skips just this range, while anything
            // that is not a plain character — every escape, since
            // `charClassBoundRune` only answers for `OpChar` — returns 0 and
            // takes the **whole class** out of both this check and the
            // duplicate check. The high bound is never consulted at all.
            if matches!(
                r.start.kind,
                LiteralKind::Octal | LiteralKind::HexFixed(_) | LiteralKind::HexBrace(_)
            ) {
                continue;
            }
            if !matches!(r.start.kind, LiteralKind::Verbatim)
                || pat.escaped.contains(&r.start.span.start.offset)
            {
                return false;
            }
            let ch = r.start.c;
            if ch == '\0' {
                return false;
            }
            if !is_letter_or_digit(ch) {
                msgs.push(format!(
                    "suspicious char range `{}` in {}",
                    slice(pat, &r.span),
                    slice(pat, &cc.span)
                ));
            }
        }
    }
    true
}

fn class_items(kind: &ClassSet) -> Option<Vec<&ClassSetItem>> {
    match kind {
        ClassSet::Item(ClassSetItem::Union(u)) => Some(u.items.iter().collect()),
        ClassSet::Item(item) => Some(vec![item]),
        ClassSet::BinaryOp(_) => None,
    }
}

fn check_char_class_dups(
    pat: &Pat<'_>,
    cc: &regex_syntax::ast::ClassBracketed,
    msgs: &mut Vec<String>,
) {
    let Some(items) = class_items(&cc.kind) else {
        return;
    };
    if items.len() <= 1 {
        return;
    }

    let mut ranges: Vec<CharRange> = Vec::new();
    for item in items {
        if !collect_item_ranges(pat, item, &mut ranges) {
            return; // give up on unknown
        }
    }

    ranges.sort_by(|a, b| match a.low.cmp(&b.low) {
        Ordering::Equal => a.high.cmp(&b.high),
        o => o,
    });

    for i in 0..ranges.len().saturating_sub(1) {
        let x = &ranges[i];
        let y = &ranges[i + 1];
        if x.high >= y.low {
            if x.source == y.source {
                msgs.push(format!(
                    "`{}` is duplicated in {}",
                    x.source,
                    slice(pat, &cc.span)
                ));
            } else {
                msgs.push(format!(
                    "`{}` intersects with `{}` in {}",
                    x.source,
                    y.source,
                    slice(pat, &cc.span)
                ));
            }
            break;
        }
    }
}

fn collect_item_ranges(pat: &Pat<'_>, item: &ClassSetItem, ranges: &mut Vec<CharRange>) -> bool {
    match item {
        ClassSetItem::Literal(lit) => {
            ranges.push(CharRange {
                low: lit.c as u32,
                high: lit.c as u32,
                source: slice(pat, &lit.span).to_string(),
            });
            true
        }
        ClassSetItem::Range(r) => {
            ranges.push(CharRange {
                low: r.start.c as u32,
                high: r.end.c as u32,
                source: slice(pat, &r.span).to_string(),
            });
            true
        }
        ClassSetItem::Perl(p) => {
            add_perl_ranges(pat, p, ranges);
            true
        }
        ClassSetItem::Empty(_) => true,
        ClassSetItem::Union(u) => {
            for it in &u.items {
                if !collect_item_ranges(pat, it, ranges) {
                    return false;
                }
            }
            true
        }
        ClassSetItem::Ascii(_) | ClassSetItem::Unicode(_) | ClassSetItem::Bracketed(_) => false,
    }
}

fn add_perl_ranges(pat: &Pat<'_>, p: &ClassPerl, ranges: &mut Vec<CharRange>) {
    let src = slice(pat, &p.span).to_string();
    let add = |ranges: &mut Vec<CharRange>, low: u32, high: u32, source: &str| {
        ranges.push(CharRange {
            low,
            high,
            source: source.to_string(),
        });
    };
    match (&p.kind, p.negated) {
        (ClassPerlKind::Digit, false) => add(ranges, b'0' as u32, b'9' as u32, &src),
        (ClassPerlKind::Digit, true) => {
            add(ranges, 0, b'0' as u32 - 1, &src);
            add(ranges, b'9' as u32 + 1, MAX_RUNE, &src);
        }
        (ClassPerlKind::Space, false) => {
            add(ranges, b'\t' as u32, b'\n' as u32, &src);
            add(ranges, b'\x0c' as u32, b'\r' as u32, &src);
            add(ranges, b' ' as u32, b' ' as u32, &src);
        }
        (ClassPerlKind::Space, true) => {
            add(ranges, 0, b'\t' as u32 - 1, &src);
            add(ranges, b'\n' as u32 + 1, b'\x0c' as u32 - 1, &src);
            add(ranges, b'\r' as u32 + 1, b' ' as u32 - 1, &src);
            add(ranges, b' ' as u32 + 1, MAX_RUNE, &src);
        }
        (ClassPerlKind::Word, false) => {
            add(ranges, b'0' as u32, b'9' as u32, &src);
            add(ranges, b'A' as u32, b'Z' as u32, &src);
            add(ranges, b'_' as u32, b'_' as u32, &src);
            add(ranges, b'a' as u32, b'z' as u32, &src);
        }
        (ClassPerlKind::Word, true) => {
            add(ranges, 0, b'0' as u32 - 1, &src);
            add(ranges, b'9' as u32 + 1, b'A' as u32 - 1, &src);
            add(ranges, b'Z' as u32 + 1, b'_' as u32 - 1, &src);
            add(ranges, b'_' as u32 + 1, b'a' as u32 - 1, &src);
            add(ranges, b'z' as u32 + 1, MAX_RUNE, &src);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::check_pattern;

    #[test]
    fn detects_char_class_dup() {
        let msgs = check_pattern(r"x[aba]y");
        assert!(
            msgs.iter().any(|m| m.contains("`a` is duplicated")),
            "{msgs:?}"
        );
    }

    #[test]
    fn detects_word_intersect() {
        let msgs = check_pattern(r"[\w_]");
        assert!(
            msgs.iter().any(|m| m.contains(r"`\w` intersects with `_`")),
            "{msgs:?}"
        );
    }

    #[test]
    fn detects_alt_anchor() {
        let msgs = check_pattern(r"^foo|bar|baz");
        assert!(
            msgs.iter()
                .any(|m| m.contains("^ applied only to `foo`")),
            "{msgs:?}"
        );
    }

    #[test]
    fn detects_nested_quantifier() {
        let msgs = check_pattern(r"(a+)+");
        assert!(
            msgs.iter()
                .any(|m| m.contains("repeated greedy quantifier")),
            "{msgs:?}"
        );
    }

    /// The char-class range check, shape by shape, as exact sets.
    ///
    /// Every line here was measured against golangci-lint 2.12.2 before it was
    /// written down; the three that used to be wrong are marked.
    #[test]
    fn char_class_range_reads_only_the_low_bound() {
        let cases: &[(&str, &[&str])] = &[
            // A plain low bound that is neither letter nor digit.
            (r"[!-_]", &[r"suspicious char range `!-_` in [!-_]"]),
            // WAS WRONG (missing): upstream never looks at the high bound, so a
            // hex *high* bound does not skip the range.
            (r"[!-\x7A]", &[r"suspicious char range `!-\x7A` in [!-\x7A]"]),
            // A hex or octal *low* bound skips this range and only this range.
            (r"[\x41-\x5A]", &[]),
            (r"[\101-\132]", &[]),
            // WAS WRONG (false positives): an escaped low bound gives
            // `charClassBoundRune` 0, which returns false for the whole class.
            (r"[\|-~]", &[]),
            (r"[\--z]", &[]),
            (r"[\+-z]", &[]),
            (r"[\.-z]", &[]),
            (r"[\t-\r]", &[]),
            // A letter low bound is good whatever the high bound is.
            (r"[a-\|]", &[]),
            // An escaped element that is not a range bound changes nothing.
            (
                r#"[\|"-:]"#,
                &[r#"suspicious char range `"-:` in [\|"-:]"#],
            ),
        ];
        for (pat, want) in cases {
            assert_eq!(&check_pattern(pat), want, "pattern {pat}");
        }
    }

    /// `\<` and `\>`: go-critic's lexer makes them plain escaped characters,
    /// `regex-syntax` makes them word-boundary assertions — which inside a
    /// character class is a parse error, and a parse error here is silent.
    ///
    /// WAS WRONG (missing): one `\<` anywhere in the pattern hid every finding
    /// in it. telegraf's `plugins/serializers/graphite/graphite.go:21` is the
    /// fifth case.
    #[test]
    fn escaped_angle_brackets_are_literals() {
        let cases: &[(&str, &[&str])] = &[
            (r"[\<\<]", &[r"`\<` is duplicated in [\<\<]"]),
            (r"[\<<]", &[r"`\<` intersects with `<` in [\<<]"]),
            (r"[\>\>]", &[r"`\>` is duplicated in [\>\>]"]),
            (r"\<a|\<a", &[r"`\<a` is duplicated in \<a|\<a"]),
            (
                r#"[^ "-:\<>-\]_a-~\p{L}]"#,
                &[
                    r#"suspicious char range `"-:` in [^ "-:\<>-\]_a-~\p{L}]"#,
                    r#"suspicious char range `>-\]` in [^ "-:\<>-\]_a-~\p{L}]"#,
                ],
            ),
            (r"[\<>-\]]", &[r"suspicious char range `>-\]` in [\<>-\]]"]),
            (r#"[">-\]]"#, &[r#"suspicious char range `>-\]` in [">-\]]"#]),
            // Silent: `\<` as a range low bound still returns 0.
            (r"[\<-\~]", &[]),
            (r"[\<-\]]", &[]),
            (r"\<abc\>", &[]),
        ];
        for (pat, want) in cases {
            assert_eq!(&check_pattern(pat), want, "pattern {pat}");
        }
    }

    /// The rewrite keeps the source text: messages quote `\<`, not `<`, and the
    /// class they name is the one the programmer wrote.
    #[test]
    fn messages_quote_the_source_not_the_rewrite() {
        let msgs = check_pattern(r"[\<\<]");
        assert_eq!(msgs, vec![r"`\<` is duplicated in [\<\<]"]);
    }

    #[test]
    fn detects_overview_example() {
        let msgs = check_pattern(r"(?:^aa|bb|cc)foo[aba]");
        assert!(
            msgs.iter().any(|m| m.contains("^ applied only to")),
            "{msgs:?}"
        );
        assert!(
            msgs.iter().any(|m| m.contains("is duplicated")),
            "{msgs:?}"
        );
    }
}
