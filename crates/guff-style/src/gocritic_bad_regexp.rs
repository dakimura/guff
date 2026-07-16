//! Port of go-critic `badRegexp` (suspicious regexp patterns).
//!
//! Uses `regex-syntax` AST (Go RE2-like) instead of quasilyte/regex/syntax.
//! Full dangling-anchor / flag edge-case parity with upstream is DEFERRED.

use std::cmp::Ordering;

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

/// Analyze a regexp pattern string and return diagnostic messages
/// (without the `badRegexp: ` prefix — caller adds context).
pub fn check_pattern(pat: &str) -> Vec<String> {
    let mut parser = ParserBuilder::new().octal(true).build();
    let Ok(ast) = parser.parse(pat) else {
        return Vec::new();
    };
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

fn slice<'a>(pat: &'a str, span: &Span) -> &'a str {
    let start = span.start.offset.min(pat.len());
    let end = span.end.offset.min(pat.len()).max(start);
    &pat[start..end]
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

fn mark_good_carets(pat: &str, e: &Ast, out: &mut Vec<Span>) {
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
    pat: &str,
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
    pat: &str,
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
    pat: &str,
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

fn check_alt_dups(pat: &str, alt: &regex_syntax::ast::Alternation, msgs: &mut Vec<String>) {
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
fn caret_prefix_lit<'a>(pat: &'a str, e: &'a Ast) -> Option<&'a str> {
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
                return Some(&pat[start.min(pat.len())..end.min(pat.len())]);
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

fn dollar_suffix_lit<'a>(pat: &'a str, e: &'a Ast) -> Option<&'a str> {
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
            Some(&pat[start.min(pat.len())..end.min(pat.len())])
        }
        _ => None,
    }
}

fn check_alt_anchor(pat: &str, alt: &regex_syntax::ast::Alternation, msgs: &mut Vec<String>) {
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
    pat: &str,
    cc: &regex_syntax::ast::ClassBracketed,
    msgs: &mut Vec<String>,
) -> bool {
    let Some(items) = class_items(&cc.kind) else {
        return false;
    };
    for item in items {
        if let ClassSetItem::Range(r) = item {
            // Permit hex/octal bounds (go-critic skips OpEscapeHex/Octal).
            if matches!(
                r.start.kind,
                LiteralKind::Octal
                    | LiteralKind::HexFixed(_)
                    | LiteralKind::HexBrace(_)
            ) || matches!(
                r.end.kind,
                LiteralKind::Octal
                    | LiteralKind::HexFixed(_)
                    | LiteralKind::HexBrace(_)
            ) {
                continue;
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
    pat: &str,
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

fn collect_item_ranges(pat: &str, item: &ClassSetItem, ranges: &mut Vec<CharRange>) -> bool {
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

fn add_perl_ranges(pat: &str, p: &ClassPerl, ranges: &mut Vec<CharRange>) {
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
