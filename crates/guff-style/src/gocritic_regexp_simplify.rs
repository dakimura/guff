//! Port of go-critic `regexpSimplify` (detects regexp patterns that can be simplified).
//!
//! Uses `regex-syntax` AST instead of quasilyte/regex. Full edge-case parity with
//! upstream (e.g. Go-only `[][]` class spelling) is DEFERRED.

use std::collections::HashSet;

use regex_syntax::ast::parse::ParserBuilder;
use regex_syntax::ast::{
    AssertionKind, Ast, ClassAsciiKind, ClassBracketed, ClassPerlKind, ClassSet, ClassSetItem,
    ClassSetRange, Flag, Flags, FlagsItemKind, GroupKind, Literal, LiteralKind, RepetitionKind,
    RepetitionRange,
};

/// Suggest a simplified form of `pat`, or `None` if unchanged / unparseable / too long.
pub fn simplify(pat: &str) -> Option<String> {
    if pat.len() > 60 {
        return None;
    }
    let mut simplified = pat.to_string();
    for _ in 0..2 {
        let Some(candidate) = simplify_once(&simplified) else {
            break;
        };
        if candidate == simplified {
            break;
        }
        simplified = candidate;
    }
    if simplified != pat {
        Some(simplified)
    } else {
        None
    }
}

fn simplify_once(pat: &str) -> Option<String> {
    let mut parser = ParserBuilder::new().octal(true).build();
    let (ast, unangled, seed) = match parser.parse(pat) {
        Ok(ast) => (ast, HashSet::new(), 0),
        Err(_) => {
            // `\<` and `\>` are word-boundary assertions to `regex-syntax` and
            // plain escaped characters to the parser upstream uses. Outside a
            // character class the `Ast::Assertion` arm below already answers as
            // upstream does; **inside** one they are a parse error, and a parse
            // error drops the pattern in silence — taking every other
            // simplification in it with it. telegraf's
            // `plugins/serializers/graphite/graphite.go:21` is one.
            let (rewritten, marks) = unescape_angle_brackets(pat);
            if marks.is_empty() {
                return None;
            }
            let n = marks.len() as u32;
            // `regex-syntax`'s parser is single-use: the failed attempt left
            // state behind, so the retry needs a fresh one.
            let mut retry = ParserBuilder::new().octal(true).build();
            let ast = retry.parse(&rewritten).ok()?;
            (ast, marks, n)
        }
    };
    let mut ctx = Ctx {
        out: String::new(),
        score: seed,
        unangled,
    };
    ctx.walk(&ast);
    if ctx.score > 0 {
        Some(ctx.out)
    } else {
        None
    }
}

/// Drop the backslash of every `\<` / `\>`, returning the offsets the bare
/// characters landed on. Those offsets are how the walker remembers that a
/// plain-looking `<` is really an `OpEscapeChar` — which matters because
/// `\<` is **not** a valid char-range operand upstream.
fn unescape_angle_brackets(pat: &str) -> (String, HashSet<usize>) {
    let mut out = String::with_capacity(pat.len());
    let mut marks = HashSet::new();
    let mut it = pat.chars().peekable();
    while let Some(ch) = it.next() {
        if ch == '\\' {
            if let Some(&c2) = it.peek() {
                it.next();
                if c2 == '<' || c2 == '>' {
                    marks.insert(out.len());
                    out.push(c2);
                    continue;
                }
                out.push(ch);
                out.push(c2);
                continue;
            }
        }
        out.push(ch);
    }
    (out, marks)
}

struct Ctx {
    out: String,
    score: u32,
    /// Offsets in the parsed text where a `\<` / `\>` lost its backslash.
    unangled: HashSet<usize>,
}

impl Ctx {
    fn walk(&mut self, e: &Ast) {
        match e {
            Ast::Concat(c) => self.walk_concat(&c.asts),
            Ast::Alternation(a) => self.walk_alt(&a.asts),
            Ast::ClassBracketed(c) => self.walk_class(c),
            Ast::Repetition(r) => self.walk_repetition(r),
            Ast::Group(g) => self.walk_group(g),
            Ast::Literal(lit) => self.walk_literal(lit, /*in_class*/ false),
            Ast::Dot(_) => self.out.push('.'),
            Ast::ClassPerl(p) => self.out.push_str(&perl_str(&p.kind, p.negated)),
            Ast::ClassUnicode(u) => {
                self.out
                    .push_str(&format!("{}", Ast::class_unicode((**u).clone())));
            }
            // `\<` / `\>` are word-boundary assertions to `regex-syntax` and
            // plain escaped characters to the parser upstream uses, which then
            // unescapes both — they are on its thirteen-character list. Answer
            // as upstream does; the assertion reading is not available in Go's
            // regexp anyway, where `\<` is just `<`.
            Ast::Assertion(a)
                if matches!(
                    a.kind,
                    AssertionKind::WordBoundaryStartAngle
                        | AssertionKind::WordBoundaryEndAngle
                ) =>
            {
                self.score += 1;
                self.out.push(match a.kind {
                    AssertionKind::WordBoundaryStartAngle => '<',
                    _ => '>',
                });
            }
            Ast::Assertion(a) => self.out.push_str(assertion_str(&a.kind)),
            Ast::Flags(f) => {
                self.out.push_str("(?");
                write_flags(&mut self.out, &f.flags);
                self.out.push(')');
            }
            Ast::Empty(_) => {}
        }
    }

    fn walk_literal(&mut self, lit: &Literal, in_class: bool) {
        match lit.kind {
            LiteralKind::Superfluous => {
                // Same list as `Meta` below, and for the same reason: upstream
                // names the thirteen characters it will unescape and writes
                // every other `OpEscapeChar` back as it found it. `\ ` is the
                // one that showed — argo-cd's `^gpg: Signature made
                // ([a-zA-Z0-9\ :]+)$` got a rewrite suggestion from guff alone,
                // for an escape upstream does not touch.
                if can_unescape_meta(lit.c, in_class) {
                    self.score += 1;
                    if needs_escape_in_class(lit.c) && in_class {
                        self.out.push('\\');
                    }
                    self.out.push(lit.c);
                } else {
                    self.out.push('\\');
                    self.out.push(lit.c);
                }
            }
            LiteralKind::Meta => {
                // regex-syntax marks many unnecessary escapes as Meta (e.g. `\ #`).
                // Upstream unescapes `\#`, `\&`, `\/`, … (and `\.` only inside classes).
                if can_unescape_meta(lit.c, in_class) {
                    self.score += 1;
                    self.out.push(lit.c);
                } else {
                    self.out.push('\\');
                    self.out.push(lit.c);
                }
            }
            LiteralKind::Verbatim => {
                if in_class {
                    self.out.push(lit.c);
                } else if is_meta_outside(lit.c) {
                    self.out.push('\\');
                    self.out.push(lit.c);
                } else {
                    self.out.push(lit.c);
                }
            }
            _ => {
                // Preserve hex/octal/special escapes via Display.
                self.out.push_str(&format!("{}", Ast::literal(lit.clone())));
            }
        }
    }

    fn walk_repetition(&mut self, r: &regex_syntax::ast::Repetition) {
        match &r.op.kind {
            RepetitionKind::Range(RepetitionRange::Bounded(0, 1)) => {
                self.walk(&r.ast);
                self.out.push('?');
                self.score += 1;
            }
            RepetitionKind::Range(RepetitionRange::AtLeast(1)) => {
                self.walk(&r.ast);
                self.out.push('+');
                self.score += 1;
            }
            RepetitionKind::Range(RepetitionRange::AtLeast(0)) => {
                self.walk(&r.ast);
                self.out.push('*');
                self.score += 1;
            }
            RepetitionKind::Range(RepetitionRange::Exactly(0)) => {
                // Drop the atom entirely.
                self.score += 1;
            }
            RepetitionKind::Range(RepetitionRange::Exactly(1)) => {
                self.walk(&r.ast);
                self.score += 1;
            }
            RepetitionKind::ZeroOrOne => {
                self.walk(&r.ast);
                self.out.push('?');
                if !r.greedy {
                    self.out.push('?');
                }
            }
            RepetitionKind::ZeroOrMore => {
                self.walk(&r.ast);
                self.out.push('*');
                if !r.greedy {
                    self.out.push('?');
                }
            }
            RepetitionKind::OneOrMore => {
                self.walk(&r.ast);
                self.out.push('+');
                if !r.greedy {
                    self.out.push('?');
                }
            }
            RepetitionKind::Range(range) => {
                self.walk(&r.ast);
                match range {
                    RepetitionRange::Exactly(n) => {
                        self.out.push_str(&format!("{{{n}}}"));
                    }
                    RepetitionRange::AtLeast(n) => {
                        self.out.push_str(&format!("{{{n},}}"));
                    }
                    RepetitionRange::Bounded(lo, hi) => {
                        self.out.push_str(&format!("{{{lo},{hi}}}"));
                    }
                }
                if !r.greedy {
                    self.out.push('?');
                }
            }
        }
    }

    fn walk_group(&mut self, g: &regex_syntax::ast::Group) {
        match &g.kind {
            GroupKind::NonCapturing(flags) if flags.items.is_empty() => {
                // `(?:x)` → `x` when x is a single char / escape / class.
                if matches!(
                    g.ast.as_ref(),
                    Ast::Literal(_)
                        | Ast::ClassPerl(_)
                        | Ast::ClassBracketed(_)
                        | Ast::ClassUnicode(_)
                        | Ast::Dot(_)
                ) {
                    self.walk(&g.ast);
                    self.score += 1;
                    return;
                }
                self.out.push_str("(?:");
                self.walk(&g.ast);
                self.out.push(')');
            }
            GroupKind::NonCapturing(flags) => {
                self.out.push_str("(?");
                write_flags(&mut self.out, flags);
                self.out.push(':');
                self.walk(&g.ast);
                self.out.push(')');
            }
            GroupKind::CaptureIndex(_) => {
                self.out.push('(');
                self.walk(&g.ast);
                self.out.push(')');
            }
            GroupKind::CaptureName {
                name,
                starts_with_p,
            } => {
                if *starts_with_p {
                    self.out.push_str("(?P<");
                } else {
                    self.out.push_str("(?<");
                }
                self.out.push_str(&name.name);
                self.out.push('>');
                self.walk(&g.ast);
                self.out.push(')');
            }
        }
    }

    fn walk_class(&mut self, c: &ClassBracketed) {
        if let Some(s) = simplify_whole_class(c) {
            self.out.push_str(s);
            self.score += 1;
            return;
        }
        if !c.negated {
            if let Some(s) = unwrap_single_class_item(&c.kind) {
                self.out.push_str(&s);
                self.score += 1;
                return;
            }
        }
        self.out.push('[');
        if c.negated {
            self.out.push('^');
        }
        let before = self.score;
        self.walk_class_set(&c.kind);
        // If we only rewrote escapes/ranges inside, score already bumped.
        let _ = before;
        self.out.push(']');
    }

    fn walk_class_set(&mut self, set: &ClassSet) {
        match set {
            ClassSet::Item(item) => self.walk_class_item(item),
            ClassSet::BinaryOp(op) => {
                self.walk_class_set(&op.lhs);
                match op.kind {
                    regex_syntax::ast::ClassSetBinaryOpKind::Intersection => {
                        self.out.push_str("&&")
                    }
                    regex_syntax::ast::ClassSetBinaryOpKind::Difference => self.out.push_str("--"),
                    regex_syntax::ast::ClassSetBinaryOpKind::SymmetricDifference => {
                        self.out.push_str("~~")
                    }
                }
                self.walk_class_set(&op.rhs);
            }
        }
    }

    fn walk_class_item(&mut self, item: &ClassSetItem) {
        match item {
            ClassSetItem::Empty(_) => {}
            ClassSetItem::Literal(lit) => self.walk_literal(lit, true),
            ClassSetItem::Range(r) => self.walk_range(r),
            ClassSetItem::Ascii(a) => {
                    self.out.push_str("[:");
                if a.negated {
                    self.out.push('^');
                }
                self.out.push_str(ascii_name(&a.kind));
                self.out.push_str(":]");
            }
            ClassSetItem::Perl(p) => self.out.push_str(&perl_str(&p.kind, p.negated)),
            ClassSetItem::Unicode(u) => {
                self.out
                    .push_str(&format!("{}", Ast::class_unicode(u.clone())));
            }
            ClassSetItem::Bracketed(b) => self.walk_class(b),
            ClassSetItem::Union(u) => {
                for it in &u.items {
                    self.walk_class_item(it);
                }
            }
        }
    }

    /// A `-` inside a character class is a range only when upstream's parser
    /// says so, and it only *simplifies* a range when both ends are plain
    /// characters.
    ///
    /// ```go
    /// func (p *Parser) isValidCharRangeOperand(e *Expr) bool {
    ///     switch e.Op {
    ///     case OpEscapeHex, OpEscapeOctal, OpEscapeMeta, OpChar:
    ///         return true
    ///     case OpEscapeChar:
    ///         switch p.exprValue(e) {
    ///         case `\\`, `\|`, `\*`, `\+`, `\?`, `\.`, `\[`, `\^`, `\$`, `\(`, `\)`:
    ///     …
    /// func (c *regexpSimplifyChecker) simplifyCharRange(rng syntax.Expr) string {
    ///     if rng.Args[0].Op != syntax.OpChar || rng.Args[1].Op != syntax.OpChar {
    ///         return ""
    ///     }
    /// ```
    ///
    /// So there are three outcomes, and guff had only the first two blurred
    /// together: a real range with two plain ends may be expanded; a real range
    /// with any other end is written back **exactly as it was read**, escapes
    /// and all; and a `-` whose left operand is not a valid operand never
    /// became a range at all, so its three parts are walked separately and may
    /// each be unescaped.
    fn walk_range(&mut self, r: &ClassSetRange) {
        if !self.is_valid_range_operand(&r.start) {
            // Not a range upstream: three elements, each on its own.
            self.walk_literal(&r.start, true);
            self.out.push('-');
            self.walk_literal(&r.end, true);
            return;
        }
        if self.is_plain_char(&r.start) && self.is_plain_char(&r.end) {
            let (lo, hi) = (r.start.c, r.end.c);
            if lo.is_ascii() && hi.is_ascii() {
                match hi as u8 - lo as u8 {
                    0 => {
                        self.out.push(lo);
                        self.score += 1;
                        return;
                    }
                    1 => {
                        self.out.push(lo);
                        self.out.push(hi);
                        self.score += 1;
                        return;
                    }
                    2 => {
                        self.out.push(lo);
                        self.out.push(char::from(lo as u8 + 1));
                        self.out.push(hi);
                        self.score += 1;
                        return;
                    }
                    _ => {}
                }
            }
        }
        // `out.WriteString(e.Value)` — the range verbatim.
        self.write_literal_verbatim(&r.start);
        self.out.push('-');
        self.write_literal_verbatim(&r.end);
    }

    /// upstream `OpChar`: a character that was written without a backslash.
    fn is_plain_char(&self, lit: &Literal) -> bool {
        matches!(lit.kind, LiteralKind::Verbatim)
            && !self.unangled.contains(&lit.span.start.offset)
    }

    fn is_valid_range_operand(&self, lit: &Literal) -> bool {
        if self.is_plain_char(lit) {
            return true;
        }
        match lit.kind {
            // OpEscapeHex / OpEscapeOctal.
            LiteralKind::Octal | LiteralKind::HexFixed(_) | LiteralKind::HexBrace(_) => true,
            // Inside a character class quasilyte's metachars are only `-` and
            // `]`; every other escape is an `OpEscapeChar`, valid as an operand
            // only for the eleven regexp metacharacters. `\<` and `\>` — which
            // reach here as rewritten `Verbatim` characters — are not among
            // them, and neither are `\t` and friends.
            LiteralKind::Meta | LiteralKind::Superfluous => matches!(
                lit.c,
                '-' | ']' | '\\' | '|' | '*' | '+' | '?' | '.' | '[' | '^' | '$' | '(' | ')'
            ),
            _ => false,
        }
    }

    /// Write a literal back the way it was read — no unescaping, no score.
    fn write_literal_verbatim(&mut self, lit: &Literal) {
        if self.unangled.contains(&lit.span.start.offset) {
            self.out.push('\\');
            self.out.push(lit.c);
            return;
        }
        match lit.kind {
            LiteralKind::Meta | LiteralKind::Superfluous => {
                self.out.push('\\');
                self.out.push(lit.c);
            }
            LiteralKind::Verbatim => self.out.push(lit.c),
            _ => self.out.push_str(&format!("{}", Ast::literal(lit.clone()))),
        }
    }

    fn walk_alt(&mut self, alts: &[Ast]) {
        if !alts.is_empty() && alts.iter().all(is_single_char_literal) {
            self.score += 1;
            self.out.push('[');
            for a in alts {
                if let Ast::Literal(lit) = a {
                    self.out.push(lit.c);
                }
            }
            self.out.push(']');
            return;
        }
        if self.factor_prefix_suffix(alts) {
            return;
        }
        for (i, e) in alts.iter().enumerate() {
            self.walk(e);
            if i + 1 != alts.len() {
                self.out.push('|');
            }
        }
    }

    fn factor_prefix_suffix(&mut self, alts: &[Ast]) -> bool {
        if alts.len() != 2 {
            return false;
        }
        let Some(mut x) = concat_literal_str(&alts[0]) else {
            return false;
        };
        let Some(mut y) = concat_literal_str(&alts[1]) else {
            return false;
        };
        if x == y {
            return false;
        }
        if x.len() > y.len() {
            std::mem::swap(&mut x, &mut y);
        }
        if let Some(tail) = y.strip_prefix(&x) {
            if rune_count(tail) == 1 {
                self.out.push_str(&x);
                self.out.push_str(tail);
                self.out.push('?');
                self.score += 1;
                return true;
            }
        }
        if let Some(head) = y.strip_suffix(&x) {
            if rune_count(head) == 1 {
                self.out.push_str(head);
                self.out.push('?');
                self.out.push_str(&x);
                self.score += 1;
                return true;
            }
        }
        false
    }

    fn walk_concat(&mut self, parts: &[Ast]) {
        let mut i = 0;
        while i < parts.len() {
            let x = &parts[i];
            self.walk(x);
            i += 1;
            if i >= parts.len() {
                break;
            }
            // `xy*` → `x+` where x == y.
            if let Ast::Repetition(r) = &parts[i] {
                if matches!(r.op.kind, RepetitionKind::ZeroOrMore)
                    && r.greedy
                    && can_merge(x, &r.ast)
                {
                    self.out.push('+');
                    self.score += 1;
                    i += 1;
                    continue;
                }
            }
            let Some(threshold) = can_combine_threshold(x, &parts[i]) else {
                continue;
            };
            let mut n = 1usize;
            let mut j = i + 1;
            while j < parts.len() {
                if can_combine_threshold(x, &parts[j]).is_some() {
                    n += 1;
                    j += 1;
                } else {
                    break;
                }
            }
            if n >= threshold {
                self.out.push_str(&format!("{{{}}}", n + 1));
                self.score += 1;
                i += n;
            }
        }
    }
}

fn rune_count(s: &str) -> usize {
    s.chars().count()
}

fn is_single_char_literal(e: &Ast) -> bool {
    matches!(e, Ast::Literal(_))
}

fn concat_literal_str(e: &Ast) -> Option<String> {
    match e {
        Ast::Literal(lit) => Some(lit.c.to_string()),
        Ast::Concat(c) => {
            let mut s = String::new();
            for a in &c.asts {
                let Ast::Literal(lit) = a else {
                    return None;
                };
                s.push(lit.c);
            }
            Some(s)
        }
        _ => None,
    }
}

fn fingerprint(e: &Ast) -> String {
    // Structural identity for merge/combine (Display is stable enough).
    format!("{e}")
}

fn can_merge(x: &Ast, y: &Ast) -> bool {
    match (x, y) {
        (Ast::Literal(a), Ast::Literal(b)) => a.c == b.c && lit_kind_key(a) == lit_kind_key(b),
        (Ast::ClassBracketed(_), Ast::ClassBracketed(_))
        | (Ast::ClassPerl(_), Ast::ClassPerl(_))
        | (Ast::Group(_), Ast::Group(_))
        | (Ast::Dot(_), Ast::Dot(_)) => fingerprint(x) == fingerprint(y),
        _ => false,
    }
}

fn lit_kind_key(lit: &Literal) -> u8 {
    match lit.kind {
        LiteralKind::Verbatim => 0,
        LiteralKind::Meta => 1,
        LiteralKind::Superfluous => 2,
        _ => 3,
    }
}

fn can_combine_threshold(x: &Ast, y: &Ast) -> Option<usize> {
    match (x, y) {
        (Ast::Dot(_), Ast::Dot(_)) => Some(3),
        // Upstream's `canCombine` starts with `x.Op != y.Op`, so a plain space
        // (`OpChar`) never combines with an escaped one (`OpEscapeChar`):
        // `a\  b` is left alone, where guff used to answer `a\ {2}b`.
        (Ast::Literal(a), Ast::Literal(b))
            if a.c == b.c
                && matches!(a.kind, LiteralKind::Verbatim)
                && matches!(b.kind, LiteralKind::Verbatim) =>
        {
            if a.c == ' ' {
                Some(1)
            } else {
                Some(4)
            }
        }
        (Ast::Literal(a), Ast::Literal(b))
            if matches!(a.kind, LiteralKind::Meta | LiteralKind::Superfluous)
                && matches!(b.kind, LiteralKind::Meta | LiteralKind::Superfluous)
                && a.c == b.c =>
        {
            Some(2)
        }
        (Ast::ClassPerl(a), Ast::ClassPerl(b))
            if a.kind == b.kind && a.negated == b.negated =>
        {
            Some(2)
        }
        (Ast::ClassBracketed(_), Ast::ClassBracketed(_))
        | (Ast::Group(_), Ast::Group(_))
            if fingerprint(x) == fingerprint(y) =>
        {
            Some(1)
        }
        _ => None,
    }
}

fn simplify_whole_class(c: &ClassBracketed) -> Option<&'static str> {
    // Match upstream's Value-based table via structural recognition.
    if !c.negated {
        match &c.kind {
            ClassSet::Item(ClassSetItem::Range(r)) if r.start.c == '0' && r.end.c == '9' => {
                return Some(r"\d");
            }
            ClassSet::Item(ClassSetItem::Ascii(a)) => {
                return match (&a.kind, a.negated) {
                    (ClassAsciiKind::Word, false) => Some(r"\w"),
                    (ClassAsciiKind::Word, true) => Some(r"\W"),
                    (ClassAsciiKind::Digit, false) => Some(r"\d"),
                    (ClassAsciiKind::Digit, true) => Some(r"\D"),
                    (ClassAsciiKind::Space, false) => Some(r"\s"),
                    (ClassAsciiKind::Space, true) => Some(r"\S"),
                    _ => None,
                };
            }
            _ => {}
        }
    } else {
        match &c.kind {
            ClassSet::Item(ClassSetItem::Range(r)) if r.start.c == '0' && r.end.c == '9' => {
                return Some(r"\D");
            }
            ClassSet::Item(ClassSetItem::Perl(p)) => {
                return match (&p.kind, p.negated) {
                    (ClassPerlKind::Space, false) => Some(r"\S"),
                    (ClassPerlKind::Space, true) => Some(r"\s"),
                    (ClassPerlKind::Word, false) => Some(r"\W"),
                    (ClassPerlKind::Word, true) => Some(r"\w"),
                    (ClassPerlKind::Digit, false) => Some(r"\D"),
                    (ClassPerlKind::Digit, true) => Some(r"\d"),
                };
            }
            ClassSet::Item(ClassSetItem::Ascii(a)) => {
                // `[^[:space:]]` → `\S`, `[^[:^space:]]` → `\s`
                return match (&a.kind, a.negated) {
                    (ClassAsciiKind::Space, false) => Some(r"\S"),
                    (ClassAsciiKind::Space, true) => Some(r"\s"),
                    (ClassAsciiKind::Word, false) => Some(r"\W"),
                    (ClassAsciiKind::Word, true) => Some(r"\w"),
                    (ClassAsciiKind::Digit, false) => Some(r"\D"),
                    (ClassAsciiKind::Digit, true) => Some(r"\d"),
                    _ => None,
                };
            }
            _ => {}
        }
    }
    None
}

fn unwrap_single_class_item(kind: &ClassSet) -> Option<String> {
    let item = match kind {
        ClassSet::Item(ClassSetItem::Union(u)) if u.items.len() == 1 => &u.items[0],
        ClassSet::Item(item) => item,
        _ => return None,
    };
    match item {
        ClassSetItem::Literal(lit) => match lit.c {
            '|' | '*' | '+' | '?' | '.' | '[' | '^' | '$' | '(' | ')' => None,
            ']' => Some(r"\]".to_string()),
            c => Some(c.to_string()),
        },
        ClassSetItem::Perl(p) => Some(perl_str(&p.kind, p.negated)),
        _ => None,
    }
}

fn perl_str(kind: &ClassPerlKind, negated: bool) -> String {
    match (kind, negated) {
        (ClassPerlKind::Digit, false) => r"\d".to_string(),
        (ClassPerlKind::Digit, true) => r"\D".to_string(),
        (ClassPerlKind::Space, false) => r"\s".to_string(),
        (ClassPerlKind::Space, true) => r"\S".to_string(),
        (ClassPerlKind::Word, false) => r"\w".to_string(),
        (ClassPerlKind::Word, true) => r"\W".to_string(),
    }
}

fn ascii_name(kind: &ClassAsciiKind) -> &'static str {
    match kind {
        ClassAsciiKind::Alnum => "alnum",
        ClassAsciiKind::Alpha => "alpha",
        ClassAsciiKind::Ascii => "ascii",
        ClassAsciiKind::Blank => "blank",
        ClassAsciiKind::Cntrl => "cntrl",
        ClassAsciiKind::Digit => "digit",
        ClassAsciiKind::Graph => "graph",
        ClassAsciiKind::Lower => "lower",
        ClassAsciiKind::Print => "print",
        ClassAsciiKind::Punct => "punct",
        ClassAsciiKind::Space => "space",
        ClassAsciiKind::Upper => "upper",
        ClassAsciiKind::Word => "word",
        ClassAsciiKind::Xdigit => "xdigit",
    }
}

fn assertion_str(kind: &AssertionKind) -> &'static str {
    match kind {
        AssertionKind::StartLine => "^",
        AssertionKind::EndLine => "$",
        AssertionKind::StartText => r"\A",
        AssertionKind::EndText => r"\z",
        AssertionKind::WordBoundary => r"\b",
        AssertionKind::NotWordBoundary => r"\B",
        AssertionKind::WordBoundaryStart => r"\b{start}",
        AssertionKind::WordBoundaryEnd => r"\b{end}",
        AssertionKind::WordBoundaryStartAngle => r"\<",
        AssertionKind::WordBoundaryEndAngle => r"\>",
        AssertionKind::WordBoundaryStartHalf => r"\b{start-half}",
        AssertionKind::WordBoundaryEndHalf => r"\b{end-half}",
    }
}

fn write_flags(out: &mut String, flags: &Flags) {
    for item in &flags.items {
        match &item.kind {
            FlagsItemKind::Negation => out.push('-'),
            FlagsItemKind::Flag(f) => match f {
                Flag::CaseInsensitive => out.push('i'),
                Flag::MultiLine => out.push('m'),
                Flag::DotMatchesNewLine => out.push('s'),
                Flag::SwapGreed => out.push('U'),
                Flag::CRLF => out.push('R'),
                Flag::IgnoreWhitespace => out.push('x'),
                Flag::Unicode => out.push('u'),
            },
        }
    }
}

fn needs_escape_in_class(c: char) -> bool {
    matches!(c, ']' | '\\' | '^' | '-')
}

fn can_unescape_meta(c: char, in_class: bool) -> bool {
    // Upstream OpEscapeChar list (go-critic regexpSimplify).
    match c {
        '&' | '#' | '!' | '@' | '%' | '<' | '>' | ':' | ';' | '/' | ',' | '=' => true,
        '.' if in_class => true,
        _ => false,
    }
}

fn is_meta_outside(c: char) -> bool {
    matches!(
        c,
        '.' | '[' | ']' | '(' | ')' | '|' | '*' | '+' | '?' | '{' | '}' | '^' | '$' | '\\'
    )
}

#[cfg(test)]
mod tests {
    use super::simplify;

    /// A `-` inside a character class, as exact answers.
    ///
    /// Two questions decide the outcome, and guff used to ask neither: whether
    /// upstream's parser made a range at all (`isValidCharRangeOperand` on the
    /// **left** operand), and whether both ends are plain characters
    /// (`simplifyCharRange`). Every line was measured against
    /// golangci-lint 2.12.2.
    #[test]
    fn char_range_operands_decide_the_rewrite() {
        let cases: &[(&str, Option<&str>)] = &[
            // Two plain ends, three characters apart or fewer.
            (r"[x-z]", Some("[xyz]")),
            (r"[\<-\>]", Some("[<=>]")),
            (r"[\<-=]", Some("[<=]")),
            // WAS WRONG (false positives): a real range with an escaped end is
            // written back exactly as read — not expanded, not unescaped, even
            // when the escaped character is on the thirteen-character list.
            (r"[\|-~]", None),
            (r"[\.-z]", None),
            (r"[;-\>]", None),
            (r"[a-\|]", None),
            (r"[!-\x7A]", None),
            (r"[\x41-\x5A]", None),
            (r"[\101-\132]", None),
            (r"[\--z]", None),
            (r"[\+-z]", None),
            // Not a range upstream, so the parts are walked separately; `~` is
            // not on the list, so it keeps its backslash.
            (r"[\<-\~]", Some(r"[<-\~]")),
            (r"[\t-\r]", None),
            // Too far apart to shorten.
            (r"[!-_]", None),
        ];
        for (pat, want) in cases {
            assert_eq!(simplify(pat).as_deref(), *want, "pattern {pat}");
        }
    }

    /// `\<` and `\>` inside a character class used to be a parse error, and a
    /// parse error is silent — so one of them hid every simplification in its
    /// pattern. Outside a class the assertion arm already answered correctly.
    #[test]
    fn escaped_angle_brackets_inside_a_class() {
        let cases: &[(&str, Option<&str>)] = &[
            (r"[\<\<]", Some("[<<]")),
            (r"[\<<]", Some("[<<]")),
            (r"[\>\>]", Some("[>>]")),
            (r"[\<>-\]]", Some(r"[<>-\]]")),
            (
                r#"[^ "-:\<>-\]_a-~\p{L}]"#,
                Some(r#"[^ "-:<>-\]_a-~\p{L}]"#),
            ),
            (r"\<abc\>", Some("<abc>")),
            (r#"[">-\]]"#, None),
        ];
        for (pat, want) in cases {
            assert_eq!(simplify(pat).as_deref(), *want, "pattern {pat}");
        }
    }

    #[test]
    fn basic_rewrites() {
        assert_eq!(simplify(r"[0-9]").as_deref(), Some(r"\d"));
        assert_eq!(simplify(r"(?:a|b|c)").as_deref(), Some("[abc]"));
        assert_eq!(simplify("foo|fo").as_deref(), Some("foo?"));
        assert_eq!(simplify("axx*y").as_deref(), Some("ax+y"));
        assert_eq!(simplify("  ").as_deref(), Some(" {2}"));
        assert_eq!(simplify(r"[a-a]").as_deref(), Some("a"));
        assert_eq!(simplify(r"\#").as_deref(), Some("#"));
        assert_eq!(simplify("[x]").as_deref(), Some("x"));
        assert_eq!(simplify(r"[^\s]").as_deref(), Some(r"\S"));
        assert_eq!(simplify("x{0,1}").as_deref(), Some("x?"));
        assert_eq!(simplify("aaaaax").as_deref(), Some("a{5}x"));
        assert_eq!(simplify(r"(?:x)+").as_deref(), Some("x+"));
        assert_eq!(simplify(r"[0-9]{1,}").as_deref(), Some(r"\d+"));
    }
}
