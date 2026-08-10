//! Port of Go's `regexp/syntax` parser, for [`SA1000`](../../sa1000/index.html).
//!
//! Upstream staticcheck's SA1000 is `regexp.Compile(s)` plus `err.Error()`, so
//! the check *is* this parser. Two things have to match, not one: the
//! `ErrorCode`, and the `Expr` that `syntax.Error` prints after it — a different
//! slice of the input at nearly every error site (the whole pattern for
//! `unexpected )`, the two bytes of the escape for `invalid escape sequence`,
//! the operator together with its operand for `invalid repeat count`, the empty
//! string for `trailing backslash at end of expression`). Getting the code right
//! and the slice wrong still fails the golden gate.
//!
//! Only `syntax.Perl` mode is ported, because that is the only mode
//! `regexp.Compile` uses. The tree is built for real rather than approximated:
//! `expression too large` and `expression nests too deeply` are measured against
//! the node graph's size and height, and `invalid repeat count` against a rewalk
//! of it, so a parser that only scanned tokens could not reach those three.
//!
//! Ground truth is `compat/oracles/goregexp`; `tests/gostd_regexp.rs` replays it.

use std::collections::HashMap;

use super::regexp_table::{
    CATEGORIES, CATEGORY_ALIASES, FOLD_CATEGORY, FOLD_SCRIPT, SCRIPTS, SIMPLE_FOLD,
};

const MAX_RUNE: i32 = 0x10_FFFF;
const RUNE_ERROR: i32 = 0xFFFD;

/// Mirrors `maxHeight`, `maxSize` and `maxRunes` in `parse.go`.
const MAX_HEIGHT: i32 = 1000;
const MAX_SIZE: i64 = (128 << 20) / 40; // instSize = 5 * 8
const MAX_RUNES: i64 = (128 << 20) / 4; // runeSize = 4

/// Depths at which the port gives up rather than exhaust the thread stack.
///
/// Go's parser is iterative over the input but recursive over the tree it
/// builds, and a goroutine stack grows to fit where a Rust thread's does not.
/// Past either limit the answer is [`CompileResult::Undecided`] and SA1000
/// reports nothing — never a guess in either direction. See
/// `docs/COMPAT-HARDENING.md` §7.
///
/// The two recursions get different limits because they cost and reach
/// different things:
///
/// * `factor` → `collapse` → `factor` descends once per rune of shared literal
///   prefix, carrying several `Vec`s per frame; measured at over 3 KiB a frame
///   unoptimized, so 600 of them overflow a 2 MiB stack. Nothing bounds the
///   descent either — `checkHeight` only fires on the way back up. Reaching it
///   takes branches sharing an ever-longer prefix (`a|aa|aaa|…`); hand-written
///   alternations share a handful of runes, not hundreds.
/// * The tree walkers (`calcSize`, `calcHeight`, `Equal`, `repeatIsValid`)
///   recurse once per level with a few locals each. Their limit has to clear
///   Go's own `maxHeight` of 1000, or patterns Go rejects for nesting too
///   deeply would go unreported.
const MAX_FACTOR_DEPTH: u32 = 250;
const MAX_WALK_DEPTH: u32 = 2000;

// Parse flags. Only the ones `syntax.Perl` involves are given names.
const FOLD_CASE: u16 = 1 << 0;
const CLASS_NL: u16 = 1 << 2;
const DOT_NL: u16 = 1 << 3;
const ONE_LINE: u16 = 1 << 4;
const NON_GREEDY: u16 = 1 << 5;
const PERL_X: u16 = 1 << 6;
const UNICODE_GROUPS: u16 = 1 << 7;
const WAS_DOLLAR: u16 = 1 << 8;
const PERL: u16 = CLASS_NL | ONE_LINE | PERL_X | UNICODE_GROUPS;

/// `syntax.ErrorCode`. The strings are what `Error.Error()` prints.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Code {
    InvalidCharRange,
    InvalidEscape,
    InvalidNamedCapture,
    InvalidPerlOp,
    InvalidRepeatOp,
    InvalidRepeatSize,
    InvalidUtf8,
    MissingBracket,
    MissingParen,
    MissingRepeatArgument,
    TrailingBackslash,
    UnexpectedParen,
    NestingDepth,
    Large,
    /// Not a Go code: the recursion cutoffs, which never reach a
    /// message because the caller turns it into `Undecided`.
    GuffRecursionLimit,
}

impl Code {
    fn as_str(self) -> &'static str {
        match self {
            Code::InvalidCharRange => "invalid character class range",
            Code::InvalidEscape => "invalid escape sequence",
            Code::InvalidNamedCapture => "invalid named capture",
            Code::InvalidPerlOp => "invalid or unsupported Perl syntax",
            Code::InvalidRepeatOp => "invalid nested repetition operator",
            Code::InvalidRepeatSize => "invalid repeat count",
            Code::InvalidUtf8 => "invalid UTF-8",
            Code::MissingBracket => "missing closing ]",
            Code::MissingParen => "missing closing )",
            Code::MissingRepeatArgument => "missing argument to repetition operator",
            Code::TrailingBackslash => "trailing backslash at end of expression",
            Code::UnexpectedParen => "unexpected )",
            Code::NestingDepth => "expression nests too deeply",
            Code::Large => "expression too large",
            Code::GuffRecursionLimit => "guff: recursion limit",
        }
    }
}

#[derive(Clone, Debug)]
struct Error {
    code: Code,
    expr: Vec<u8>,
}

impl Error {
    fn new(code: Code, expr: &[u8]) -> Self {
        Error {
            code,
            expr: expr.to_vec(),
        }
    }

    /// `syntax.Error.Error()`. Bytes rather than a `String` because `Expr` is a
    /// raw slice of the pattern, and `invalid UTF-8` reports the ill-formed tail.
    fn message(&self) -> Vec<u8> {
        let mut out = b"error parsing regexp: ".to_vec();
        out.extend_from_slice(self.code.as_str().as_bytes());
        out.extend_from_slice(b": `");
        out.extend_from_slice(&self.expr);
        out.push(b'`');
        out
    }
}

/// What `regexp.Compile` decided, or that guff declined to decide.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CompileResult {
    /// The pattern compiles.
    Valid,
    /// The pattern does not compile; the payload is `err.Error()` verbatim.
    Invalid(Vec<u8>),
    /// Past [`MAX_FACTOR_DEPTH`] or [`MAX_WALK_DEPTH`]. Callers report nothing.
    Undecided,
}

/// `regexp.Compile(pattern)`, reduced to whether it succeeds and what it says.
pub fn compile_bytes(pattern: &[u8]) -> CompileResult {
    let mut p = Parser::new(pattern);
    match p.parse() {
        Ok(_) => CompileResult::Valid,
        Err(e) if e.code == Code::GuffRecursionLimit => CompileResult::Undecided,
        Err(mut e) => {
            // parse() recovers ErrLarge and ErrNestingDepth from a panic and
            // reports the whole regexp; every other code carries its own slice.
            if matches!(e.code, Code::Large | Code::NestingDepth) {
                e.expr = pattern.to_vec();
            }
            CompileResult::Invalid(e.message())
        }
    }
}

/// [`compile_bytes`] for a pattern guff holds as a `String`.
///
/// Every `Expr` is cut at a rune boundary of the input, so for valid UTF-8 in
/// the message is valid UTF-8 out; the lossy conversion is unreachable.
pub fn compile(pattern: &str) -> Result<(), Option<String>> {
    match compile_bytes(pattern.as_bytes()) {
        CompileResult::Valid => Ok(()),
        CompileResult::Undecided => Err(None),
        CompileResult::Invalid(msg) => Err(Some(String::from_utf8_lossy(&msg).into_owned())),
    }
}

// ---------------------------------------------------------------------- tree

type NodeId = usize;

/// `syntax.Op`, in Go's declaration order — the parser compares ops by
/// magnitude (`sub.Op >= opPseudo`, `re1.Op > re3.Op`), so the order is load
/// bearing, not cosmetic.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
enum Op {
    #[default]
    Zero = 0,
    NoMatch = 1,
    EmptyMatch,
    Literal,
    CharClass,
    AnyCharNotNl,
    AnyChar,
    BeginLine,
    EndLine,
    BeginText,
    EndText,
    WordBoundary,
    NoWordBoundary,
    Capture,
    Star,
    Plus,
    Quest,
    Repeat,
    Concat,
    Alternate,
    LeftParen = 128,
    VerticalBar = 129,
}

const OP_PSEUDO: u8 = 128;

#[derive(Clone, Default)]
struct Node {
    op: Op,
    flags: u16,
    sub: Vec<NodeId>,
    rune: Vec<i32>,
    min: i32,
    max: i32,
    cap: i32,
    name: Vec<u8>,
}

type PResult<T> = Result<T, Error>;

struct Parser<'a> {
    s: &'a [u8],
    flags: u16,
    stack: Vec<NodeId>,
    /// `p.free`, whose LIFO order decides which node id a fresh `newRegexp`
    /// hands out — and node identity is the key of the height and size maps.
    free: Vec<NodeId>,
    nodes: Vec<Node>,
    num_cap: i32,
    num_regexp: i64,
    num_runes: i64,
    repeats: i64,
    height: Option<HashMap<NodeId, i32>>,
    size: Option<HashMap<NodeId, i64>>,
    walk_depth: u32,
    factor_depth: u32,
}

impl<'a> Parser<'a> {
    fn new(s: &'a [u8]) -> Self {
        Parser {
            s,
            flags: PERL,
            stack: Vec::new(),
            free: Vec::new(),
            nodes: Vec::new(),
            num_cap: 0,
            num_regexp: 0,
            num_runes: 0,
            repeats: 0,
            height: None,
            size: None,
            walk_depth: 0,
            factor_depth: 0,
        }
    }

    fn enter_walk(&mut self) -> PResult<()> {
        self.walk_depth += 1;
        if self.walk_depth > MAX_WALK_DEPTH {
            return Err(Error::new(Code::GuffRecursionLimit, b""));
        }
        Ok(())
    }

    fn leave_walk(&mut self) {
        self.walk_depth -= 1;
    }

    fn enter_factor(&mut self) -> PResult<()> {
        self.factor_depth += 1;
        if self.factor_depth > MAX_FACTOR_DEPTH {
            return Err(Error::new(Code::GuffRecursionLimit, b""));
        }
        Ok(())
    }

    fn leave_factor(&mut self) {
        self.factor_depth -= 1;
    }

    // ------------------------------------------------------------ node arena

    fn new_regexp(&mut self, op: Op) -> NodeId {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = Node {
                op,
                ..Node::default()
            };
            id
        } else {
            self.nodes.push(Node {
                op,
                ..Node::default()
            });
            self.num_regexp += 1;
            self.nodes.len() - 1
        }
    }

    fn reuse(&mut self, re: NodeId) {
        if let Some(h) = self.height.as_mut() {
            h.remove(&re);
        }
        self.free.push(re);
    }

    // ------------------------------------------------------------- limits

    fn check_limits(&mut self, re: NodeId) -> PResult<()> {
        if self.num_runes > MAX_RUNES {
            return Err(Error::new(Code::Large, b""));
        }
        self.check_size(re)?;
        self.check_height(re)
    }

    fn check_size(&mut self, re: NodeId) -> PResult<()> {
        if self.size.is_none() {
            // Cheap pre-check: track the product of the repeats seen so far and
            // only start measuring when the node count times that product could
            // exceed the budget.
            if self.repeats == 0 {
                self.repeats = 1;
            }
            if self.nodes[re].op == Op::Repeat {
                let mut n = self.nodes[re].max;
                if n == -1 {
                    n = self.nodes[re].min;
                }
                if n <= 0 {
                    n = 1;
                }
                if i64::from(n) > MAX_SIZE / self.repeats {
                    self.repeats = MAX_SIZE;
                } else {
                    self.repeats *= i64::from(n);
                }
            }
            if self.num_regexp < MAX_SIZE / self.repeats {
                return Ok(());
            }
            self.size = Some(HashMap::new());
            for r in self.stack.clone() {
                self.check_size(r)?;
            }
        }
        if self.calc_size(re, true)? > MAX_SIZE {
            return Err(Error::new(Code::Large, b""));
        }
        Ok(())
    }

    fn calc_size(&mut self, re: NodeId, force: bool) -> PResult<i64> {
        if !force {
            if let Some(sz) = self.size.as_ref().and_then(|m| m.get(&re)).copied() {
                return Ok(sz);
            }
        }
        self.enter_walk()?;
        let mut size: i64 = 0;
        match self.nodes[re].op {
            Op::Literal => size = self.nodes[re].rune.len() as i64,
            Op::Capture | Op::Star => {
                // Star can be 1+ or 2+; Go assumes 2 pessimistically.
                size = 2 + self.calc_size(self.nodes[re].sub[0], false)?;
            }
            Op::Plus | Op::Quest => size = 1 + self.calc_size(self.nodes[re].sub[0], false)?,
            Op::Concat => {
                for sub in self.nodes[re].sub.clone() {
                    size += self.calc_size(sub, false)?;
                }
            }
            Op::Alternate => {
                let subs = self.nodes[re].sub.clone();
                for sub in &subs {
                    size += self.calc_size(*sub, false)?;
                }
                if subs.len() > 1 {
                    size += subs.len() as i64 - 1;
                }
            }
            Op::Repeat => {
                let sub = self.calc_size(self.nodes[re].sub[0], false)?;
                let (min, max) = (i64::from(self.nodes[re].min), i64::from(self.nodes[re].max));
                if max == -1 {
                    size = if min == 0 { 2 + sub } else { 1 + min * sub };
                } else {
                    // x{2,5} = xx(x(x(x)?)?)?
                    size = max * sub + (max - min);
                }
            }
            _ => {}
        }
        self.leave_walk();
        size = size.max(1);
        self.size.as_mut().expect("size map").insert(re, size);
        Ok(size)
    }

    fn check_height(&mut self, re: NodeId) -> PResult<()> {
        if self.num_regexp < i64::from(MAX_HEIGHT) {
            return Ok(());
        }
        if self.height.is_none() {
            self.height = Some(HashMap::new());
            for r in self.stack.clone() {
                self.check_height(r)?;
            }
        }
        if self.calc_height(re, true)? > MAX_HEIGHT {
            return Err(Error::new(Code::NestingDepth, b""));
        }
        Ok(())
    }

    fn calc_height(&mut self, re: NodeId, force: bool) -> PResult<i32> {
        if !force {
            if let Some(h) = self.height.as_ref().and_then(|m| m.get(&re)).copied() {
                return Ok(h);
            }
        }
        self.enter_walk()?;
        let mut h = 1;
        for sub in self.nodes[re].sub.clone() {
            let hsub = self.calc_height(sub, false)?;
            if h < 1 + hsub {
                h = 1 + hsub;
            }
        }
        self.leave_walk();
        self.height.as_mut().expect("height map").insert(re, h);
        Ok(h)
    }

    // -------------------------------------------------------- stack handling

    fn push(&mut self, re: NodeId) -> PResult<Option<NodeId>> {
        self.num_runes += self.nodes[re].rune.len() as i64;
        let n = &self.nodes[re];
        let single = n.op == Op::CharClass && n.rune.len() == 2 && n.rune[0] == n.rune[1];
        let folded = (n.op == Op::CharClass
            && n.rune.len() == 4
            && n.rune[0] == n.rune[1]
            && n.rune[2] == n.rune[3]
            && simple_fold(n.rune[0]) == n.rune[2]
            && simple_fold(n.rune[2]) == n.rune[0])
            || (n.op == Op::CharClass
                && n.rune.len() == 2
                && n.rune[0] + 1 == n.rune[1]
                && simple_fold(n.rune[0]) == n.rune[1]
                && simple_fold(n.rune[1]) == n.rune[0]);

        if single {
            let (r0, flags) = (self.nodes[re].rune[0], self.flags & !FOLD_CASE);
            if self.maybe_concat(r0, flags) {
                return Ok(None);
            }
            let n = &mut self.nodes[re];
            n.op = Op::Literal;
            n.rune.truncate(1);
            n.flags = flags;
        } else if folded {
            // Case-insensitive rune like [Aa] or [Δδ].
            let (r0, flags) = (self.nodes[re].rune[0], self.flags | FOLD_CASE);
            if self.maybe_concat(r0, flags) {
                return Ok(None);
            }
            let n = &mut self.nodes[re];
            n.op = Op::Literal;
            n.rune.truncate(1);
            n.flags = flags;
        } else {
            self.maybe_concat(-1, 0);
        }

        self.stack.push(re);
        self.check_limits(re)?;
        Ok(Some(re))
    }

    /// Incremental concatenation of adjacent literals. Reports whether `r` was
    /// pushed into the leftover node.
    fn maybe_concat(&mut self, r: i32, flags: u16) -> bool {
        let n = self.stack.len();
        if n < 2 {
            return false;
        }
        let (re1, re2) = (self.stack[n - 1], self.stack[n - 2]);
        if self.nodes[re1].op != Op::Literal
            || self.nodes[re2].op != Op::Literal
            || self.nodes[re1].flags & FOLD_CASE != self.nodes[re2].flags & FOLD_CASE
        {
            return false;
        }
        let tail = self.nodes[re1].rune.clone();
        self.nodes[re2].rune.extend_from_slice(&tail);
        if r >= 0 {
            let n1 = &mut self.nodes[re1];
            n1.rune.clear();
            n1.rune.push(r);
            n1.flags = flags;
            return true;
        }
        self.stack.pop();
        self.reuse(re1);
        false
    }

    fn literal(&mut self, r: i32) -> PResult<()> {
        let id = self.new_regexp(Op::Literal);
        self.nodes[id].flags = self.flags;
        let r = if self.flags & FOLD_CASE != 0 {
            min_fold_rune(r)
        } else {
            r
        };
        self.nodes[id].rune = vec![r];
        self.push(id)?;
        Ok(())
    }

    fn op(&mut self, op: Op) -> PResult<NodeId> {
        let id = self.new_regexp(op);
        self.nodes[id].flags = self.flags;
        // Only char classes can be swallowed by maybe_concat, and this is never
        // called with one.
        Ok(self.push(id)?.expect("op node is never merged away"))
    }

    /// Replaces the top of the stack with itself repeated. `before` and `after`
    /// are offsets: the repetition operator, and what follows it.
    fn repeat(
        &mut self,
        op: Op,
        min: i32,
        max: i32,
        before: usize,
        mut after: usize,
        last_repeat: Option<usize>,
    ) -> PResult<usize> {
        let mut flags = self.flags;
        if self.flags & PERL_X != 0 {
            if after < self.s.len() && self.s[after] == b'?' {
                after += 1;
                flags ^= NON_GREEDY;
            }
            if let Some(lr) = last_repeat {
                // In Perl a** is a syntax error, not a doubled star.
                return Err(Error::new(Code::InvalidRepeatOp, &self.s[lr..after]));
            }
        }
        let n = self.stack.len();
        if n == 0 {
            return Err(Error::new(
                Code::MissingRepeatArgument,
                &self.s[before..after],
            ));
        }
        let sub = self.stack[n - 1];
        if self.nodes[sub].op as u8 >= OP_PSEUDO {
            return Err(Error::new(
                Code::MissingRepeatArgument,
                &self.s[before..after],
            ));
        }

        let re = self.new_regexp(op);
        {
            let nd = &mut self.nodes[re];
            nd.min = min;
            nd.max = max;
            nd.flags = flags;
            nd.sub = vec![sub];
        }
        self.stack[n - 1] = re;
        self.check_limits(re)?;

        if op == Op::Repeat && (min >= 2 || max >= 2) && !self.repeat_is_valid(re, 1000) {
            return Err(Error::new(Code::InvalidRepeatSize, &self.s[before..after]));
        }
        Ok(after)
    }

    /// Whether the top-level repetition together with any inner ones stays
    /// within `n` copies of the innermost thing.
    fn repeat_is_valid(&self, re: NodeId, n: i32) -> bool {
        let mut n = n;
        if self.nodes[re].op == Op::Repeat {
            let mut m = self.nodes[re].max;
            if m == 0 {
                return true;
            }
            if m < 0 {
                m = self.nodes[re].min;
            }
            if m > n {
                return false;
            }
            if m > 0 {
                n /= m;
            }
        }
        self.nodes[re]
            .sub
            .iter()
            .all(|&sub| self.repeat_is_valid(sub, n))
    }

    fn concat(&mut self) -> PResult<Option<NodeId>> {
        self.maybe_concat(-1, 0);

        let mut i = self.stack.len();
        while i > 0 && (self.nodes[self.stack[i - 1]].op as u8) < OP_PSEUDO {
            i -= 1;
        }
        let subs: Vec<NodeId> = self.stack[i..].to_vec();
        self.stack.truncate(i);

        if subs.is_empty() {
            let id = self.new_regexp(Op::EmptyMatch);
            return self.push(id);
        }
        let c = self.collapse(subs, Op::Concat)?;
        self.push(c)
    }

    fn alternate(&mut self) -> PResult<Option<NodeId>> {
        let mut i = self.stack.len();
        while i > 0 && (self.nodes[self.stack[i - 1]].op as u8) < OP_PSEUDO {
            i -= 1;
        }
        let subs: Vec<NodeId> = self.stack[i..].to_vec();
        self.stack.truncate(i);

        // All the others are already clean (see swap_vertical_bar).
        if let Some(&last) = subs.last() {
            self.clean_alt(last);
        }
        if subs.is_empty() {
            let id = self.new_regexp(Op::NoMatch);
            return self.push(id);
        }
        let c = self.collapse(subs, Op::Alternate)?;
        self.push(c)
    }

    fn clean_alt(&mut self, re: NodeId) {
        if self.nodes[re].op != Op::CharClass {
            return;
        }
        let mut rune = std::mem::take(&mut self.nodes[re].rune);
        clean_class(&mut rune);
        if rune.len() == 2 && rune[0] == 0 && rune[1] == MAX_RUNE {
            self.nodes[re].op = Op::AnyChar;
            return;
        }
        if rune.len() == 4
            && rune[0] == 0
            && rune[1] == i32::from(b'\n') - 1
            && rune[2] == i32::from(b'\n') + 1
            && rune[3] == MAX_RUNE
        {
            self.nodes[re].op = Op::AnyCharNotNl;
            return;
        }
        self.nodes[re].rune = rune;
    }

    /// Applies `op` to `subs`, hoisting any nested `op` nodes so there is never
    /// a concat of a concat or an alternate of an alternate.
    fn collapse(&mut self, subs: Vec<NodeId>, op: Op) -> PResult<NodeId> {
        if subs.len() == 1 {
            return Ok(subs[0]);
        }
        let re = self.new_regexp(op);
        let mut out: Vec<NodeId> = Vec::new();
        for sub in subs {
            if self.nodes[sub].op == op {
                let inner = std::mem::take(&mut self.nodes[sub].sub);
                out.extend(inner);
                self.reuse(sub);
            } else {
                out.push(sub);
            }
        }
        self.nodes[re].sub = out;
        if op == Op::Alternate {
            let sub = std::mem::take(&mut self.nodes[re].sub);
            self.enter_factor()?;
            let sub = self.factor(sub);
            self.leave_factor();
            self.nodes[re].sub = sub?;
            if self.nodes[re].sub.len() == 1 {
                let new = self.nodes[re].sub[0];
                self.reuse(re);
                return Ok(new);
            }
        }
        Ok(re)
    }

    /// Factors common prefixes out of an alternation list: `ABC|ABD|AEF` becomes
    /// `A(B[CD]|EF)` over four rounds.
    fn factor(&mut self, sub: Vec<NodeId>) -> PResult<Vec<NodeId>> {
        if sub.len() < 2 {
            return Ok(sub);
        }
        let mut sub = sub;

        // Round 1: factor out common literal prefixes.
        let mut str_: Vec<i32> = Vec::new();
        let mut strflags: u16 = 0;
        let mut start = 0usize;
        let mut out: Vec<NodeId> = Vec::new();
        let mut i = 0usize;
        while i <= sub.len() {
            let mut istr: Vec<i32> = Vec::new();
            let mut iflags: u16 = 0;
            if i < sub.len() {
                let (a, b) = self.leading_string(sub[i]);
                istr = a;
                iflags = b;
                if iflags == strflags {
                    let mut same = 0;
                    while same < str_.len() && same < istr.len() && str_[same] == istr[same] {
                        same += 1;
                    }
                    if same > 0 {
                        str_.truncate(same);
                        i += 1;
                        continue;
                    }
                }
            }
            if i == start + 1 {
                out.push(sub[start]);
            } else if i > start {
                let prefix = self.new_regexp(Op::Literal);
                self.nodes[prefix].flags = strflags;
                self.nodes[prefix].rune = str_.clone();
                let n = str_.len();
                for j in start..i {
                    sub[j] = self.remove_leading_string(sub[j], n);
                    self.check_limits(sub[j])?;
                }
                let suffix = self.collapse(sub[start..i].to_vec(), Op::Alternate)?;
                let re = self.new_regexp(Op::Concat);
                self.nodes[re].sub = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            str_ = istr;
            strflags = iflags;
            i += 1;
        }
        sub = out;

        // Round 2: factor out a common first piece, when it is a character class
        // or a fixed repeat of one. Anything else is unsafe to merge.
        start = 0;
        out = Vec::new();
        let mut first: Option<NodeId> = None;
        i = 0;
        while i <= sub.len() {
            let mut ifirst: Option<NodeId> = None;
            if i < sub.len() {
                ifirst = self.leading_regexp(sub[i]);
                if let (Some(f), Some(g)) = (first, ifirst) {
                    let repeat_of_class = self.nodes[f].op == Op::Repeat
                        && self.nodes[f].min == self.nodes[f].max
                        && self.is_char_class(self.nodes[f].sub[0]);
                    if self.node_equal(f, g) && (self.is_char_class(f) || repeat_of_class) {
                        i += 1;
                        continue;
                    }
                }
            }
            if i == start + 1 {
                out.push(sub[start]);
            } else if i > start {
                let prefix = first.expect("a run of >1 has a leading regexp");
                for j in start..i {
                    let reuse = j != start; // prefix came from sub[start]
                    sub[j] = self.remove_leading_regexp(sub[j], reuse);
                    self.check_limits(sub[j])?;
                }
                let suffix = self.collapse(sub[start..i].to_vec(), Op::Alternate)?;
                let re = self.new_regexp(Op::Concat);
                self.nodes[re].sub = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            first = ifirst;
            i += 1;
        }
        sub = out;

        // Round 3: collapse runs of single literals into character classes.
        start = 0;
        out = Vec::new();
        i = 0;
        while i <= sub.len() {
            if i < sub.len() && self.is_char_class(sub[i]) {
                i += 1;
                continue;
            }
            if i == start + 1 {
                out.push(sub[start]);
            } else if i > start {
                // Start with the most complex regexp in the run.
                let mut max = start;
                for j in start + 1..i {
                    let (a, b) = (sub[max], sub[j]);
                    if self.nodes[a].op < self.nodes[b].op
                        || (self.nodes[a].op == self.nodes[b].op
                            && self.nodes[a].rune.len() < self.nodes[b].rune.len())
                    {
                        max = j;
                    }
                }
                sub.swap(start, max);
                for j in start + 1..i {
                    self.merge_char_class(sub[start], sub[j]);
                    self.reuse(sub[j]);
                }
                self.clean_alt(sub[start]);
                out.push(sub[start]);
            }
            if i < sub.len() {
                out.push(sub[i]);
            }
            start = i + 1;
            i += 1;
        }
        sub = out;

        // Round 4: collapse runs of empty matches into a single empty match.
        out = Vec::new();
        for i in 0..sub.len() {
            if i + 1 < sub.len()
                && self.nodes[sub[i]].op == Op::EmptyMatch
                && self.nodes[sub[i + 1]].op == Op::EmptyMatch
            {
                continue;
            }
            out.push(sub[i]);
        }
        Ok(out)
    }

    fn leading_string(&self, re: NodeId) -> (Vec<i32>, u16) {
        let mut re = re;
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            re = self.nodes[re].sub[0];
        }
        if self.nodes[re].op != Op::Literal {
            return (Vec::new(), 0);
        }
        (
            self.nodes[re].rune.clone(),
            self.nodes[re].flags & FOLD_CASE,
        )
    }

    fn remove_leading_string(&mut self, re: NodeId, n: usize) -> NodeId {
        let mut re = re;
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            // Removing a leading string may simplify the concatenation.
            let sub = self.nodes[re].sub[0];
            let sub = self.remove_leading_string(sub, n);
            self.nodes[re].sub[0] = sub;
            if self.nodes[sub].op == Op::EmptyMatch {
                self.reuse(sub);
                match self.nodes[re].sub.len() {
                    0 | 1 => {
                        // Impossible but handled, as upstream does.
                        self.nodes[re].op = Op::EmptyMatch;
                        self.nodes[re].sub.clear();
                    }
                    2 => {
                        let old = re;
                        re = self.nodes[re].sub[1];
                        self.reuse(old);
                    }
                    _ => {
                        self.nodes[re].sub.remove(0);
                    }
                }
            }
            return re;
        }
        if self.nodes[re].op == Op::Literal {
            self.nodes[re].rune.drain(..n);
            if self.nodes[re].rune.is_empty() {
                self.nodes[re].op = Op::EmptyMatch;
            }
        }
        re
    }

    fn leading_regexp(&self, re: NodeId) -> Option<NodeId> {
        if self.nodes[re].op == Op::EmptyMatch {
            return None;
        }
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            let sub = self.nodes[re].sub[0];
            if self.nodes[sub].op == Op::EmptyMatch {
                return None;
            }
            return Some(sub);
        }
        Some(re)
    }

    fn remove_leading_regexp(&mut self, re: NodeId, reuse: bool) -> NodeId {
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            if reuse {
                let first = self.nodes[re].sub[0];
                self.reuse(first);
            }
            self.nodes[re].sub.remove(0);
            match self.nodes[re].sub.len() {
                0 => {
                    self.nodes[re].op = Op::EmptyMatch;
                    self.nodes[re].sub.clear();
                }
                1 => {
                    let new = self.nodes[re].sub[0];
                    self.reuse(re);
                    return new;
                }
                _ => {}
            }
            return re;
        }
        if reuse {
            self.reuse(re);
        }
        self.new_regexp(Op::EmptyMatch)
    }

    fn is_char_class(&self, re: NodeId) -> bool {
        let n = &self.nodes[re];
        (n.op == Op::Literal && n.rune.len() == 1)
            || n.op == Op::CharClass
            || n.op == Op::AnyCharNotNl
            || n.op == Op::AnyChar
    }

    fn match_rune(&self, re: NodeId, r: i32) -> bool {
        let n = &self.nodes[re];
        match n.op {
            Op::Literal => n.rune.len() == 1 && n.rune[0] == r,
            Op::CharClass => n.rune.chunks(2).any(|c| c[0] <= r && r <= c[1]),
            Op::AnyCharNotNl => r != i32::from(b'\n'),
            Op::AnyChar => true,
            _ => false,
        }
    }

    fn node_equal(&self, x: NodeId, y: NodeId) -> bool {
        if self.nodes[x].op != self.nodes[y].op {
            return false;
        }
        match self.nodes[x].op {
            // The parse flags remember whether this was \z or $.
            Op::EndText => self.nodes[x].flags & WAS_DOLLAR == self.nodes[y].flags & WAS_DOLLAR,
            Op::Literal | Op::CharClass => {
                self.nodes[x].flags & FOLD_CASE == self.nodes[y].flags & FOLD_CASE
                    && self.nodes[x].rune == self.nodes[y].rune
            }
            Op::Alternate | Op::Concat => {
                self.nodes[x].sub.len() == self.nodes[y].sub.len()
                    && self.nodes[x]
                        .sub
                        .iter()
                        .zip(&self.nodes[y].sub)
                        .all(|(&a, &b)| self.node_equal(a, b))
            }
            Op::Star | Op::Plus | Op::Quest => {
                self.nodes[x].flags & NON_GREEDY == self.nodes[y].flags & NON_GREEDY
                    && self.node_equal(self.nodes[x].sub[0], self.nodes[y].sub[0])
            }
            Op::Repeat => {
                self.nodes[x].flags & NON_GREEDY == self.nodes[y].flags & NON_GREEDY
                    && self.nodes[x].min == self.nodes[y].min
                    && self.nodes[x].max == self.nodes[y].max
                    && self.node_equal(self.nodes[x].sub[0], self.nodes[y].sub[0])
            }
            Op::Capture => {
                self.nodes[x].cap == self.nodes[y].cap
                    && self.nodes[x].name == self.nodes[y].name
                    && self.node_equal(self.nodes[x].sub[0], self.nodes[y].sub[0])
            }
            _ => true,
        }
    }

    /// `dst = dst|src`. The caller ensures `dst.Op >= src.Op`.
    fn merge_char_class(&mut self, dst: NodeId, src: NodeId) {
        match self.nodes[dst].op {
            Op::AnyChar => {} // src adds nothing
            Op::AnyCharNotNl => {
                if self.match_rune(src, i32::from(b'\n')) {
                    self.nodes[dst].op = Op::AnyChar;
                }
            }
            Op::CharClass => {
                // src is simpler, so either a literal or a char class.
                let mut rune = std::mem::take(&mut self.nodes[dst].rune);
                if self.nodes[src].op == Op::Literal {
                    append_literal(&mut rune, self.nodes[src].rune[0], self.nodes[src].flags);
                } else {
                    let x = self.nodes[src].rune.clone();
                    append_class(&mut rune, &x);
                }
                self.nodes[dst].rune = rune;
            }
            Op::Literal => {
                if self.nodes[src].rune[0] == self.nodes[dst].rune[0]
                    && self.nodes[src].flags == self.nodes[dst].flags
                {
                    return;
                }
                let (dr, df) = (self.nodes[dst].rune[0], self.nodes[dst].flags);
                let (sr, sf) = (self.nodes[src].rune[0], self.nodes[src].flags);
                self.nodes[dst].op = Op::CharClass;
                let mut rune = Vec::new();
                append_literal(&mut rune, dr, df);
                append_literal(&mut rune, sr, sf);
                self.nodes[dst].rune = rune;
            }
            _ => {}
        }
    }

    fn parse_vertical_bar(&mut self) -> PResult<()> {
        self.concat()?;
        // If the concatenation sits above an opVerticalBar, swap it below;
        // otherwise start a new alternation.
        if !self.swap_vertical_bar() {
            self.op(Op::VerticalBar)?;
        }
        Ok(())
    }

    fn swap_vertical_bar(&mut self) -> bool {
        let n = self.stack.len();
        // If both sides of the bar are char classes, merge them into one.
        if n >= 3
            && self.nodes[self.stack[n - 2]].op == Op::VerticalBar
            && self.is_char_class(self.stack[n - 1])
            && self.is_char_class(self.stack[n - 3])
        {
            let mut re1 = self.stack[n - 1];
            let mut re3 = self.stack[n - 3];
            // Make re3 the more complex of the two.
            if self.nodes[re1].op > self.nodes[re3].op {
                std::mem::swap(&mut re1, &mut re3);
                self.stack[n - 3] = re3;
            }
            self.merge_char_class(re3, re1);
            self.reuse(re1);
            self.stack.truncate(n - 1);
            return true;
        }
        if n >= 2 {
            let re1 = self.stack[n - 1];
            let re2 = self.stack[n - 2];
            if self.nodes[re2].op == Op::VerticalBar {
                if n >= 3 {
                    // Now out of reach; clean opportunistically.
                    let re3 = self.stack[n - 3];
                    self.clean_alt(re3);
                }
                self.stack[n - 2] = re1;
                self.stack[n - 1] = re2;
                return true;
            }
        }
        false
    }

    fn parse_right_paren(&mut self) -> PResult<()> {
        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop();
        }
        self.alternate()?;

        let n = self.stack.len();
        if n < 2 {
            return Err(Error::new(Code::UnexpectedParen, self.s));
        }
        let re1 = self.stack[n - 1];
        let re2 = self.stack[n - 2];
        self.stack.truncate(n - 2);
        if self.nodes[re2].op != Op::LeftParen {
            return Err(Error::new(Code::UnexpectedParen, self.s));
        }
        // Restore the flags in effect when the paren opened.
        self.flags = self.nodes[re2].flags;
        if self.nodes[re2].cap == 0 {
            self.push(re1)?; // just for grouping
        } else {
            self.nodes[re2].op = Op::Capture;
            self.nodes[re2].sub = vec![re1];
            self.push(re2)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------- scanning

    /// `utf8.DecodeRuneInString` plus Go's "invalid UTF-8 is an error" rule.
    fn next_rune(&self, lo: usize, hi: usize) -> PResult<(i32, usize)> {
        let (c, size) = decode_rune(&self.s[lo..hi]);
        if c == RUNE_ERROR && size == 1 {
            return Err(Error::new(Code::InvalidUtf8, &self.s[lo..hi]));
        }
        Ok((c, lo + size))
    }

    fn check_utf8(&self, lo: usize, hi: usize) -> PResult<()> {
        let mut i = lo;
        while i < hi {
            let (c, size) = decode_rune(&self.s[i..hi]);
            if c == RUNE_ERROR && size == 1 {
                return Err(Error::new(Code::InvalidUtf8, &self.s[i..hi]));
            }
            i += size;
        }
        Ok(())
    }

    /// `{min}`, `{min,}` or `{min,max}` at `i`. `None` means "not that shape",
    /// which makes the caller treat `{` as a literal. `min == -1` means the
    /// numbers parsed but were too big.
    fn parse_repeat(&self, i: usize) -> Option<(i32, i32, usize)> {
        if i >= self.s.len() || self.s[i] != b'{' {
            return None;
        }
        let mut j = i + 1;
        let (mut min, nj) = parse_int(self.s, j)?;
        j = nj;
        if j >= self.s.len() {
            return None;
        }
        let max;
        if self.s[j] != b',' {
            max = min;
        } else {
            j += 1;
            if j >= self.s.len() {
                return None;
            }
            if self.s[j] == b'}' {
                max = -1;
            } else {
                let (m, nj) = parse_int(self.s, j)?;
                max = m;
                j = nj;
                if max < 0 {
                    min = -1; // parse_int found too big a number
                }
            }
        }
        if j >= self.s.len() || self.s[j] != b'}' {
            return None;
        }
        Some((min, max, j + 1))
    }

    /// A Perl flag setting, a non-capturing group, or both — `(?i)`, `(?:`,
    /// `(?i:`. The caller has ensured `s[i..]` starts with `(?`.
    fn parse_perl_flags(&mut self, i: usize) -> PResult<usize> {
        let s = i;
        let rest = &self.s[i..];

        // Named captures: (?P<name>expr), (?<name>expr).
        let starts_with_p = rest.len() > 4 && rest[2] == b'P' && rest[3] == b'<';
        let starts_with_name = rest.len() > 3 && rest[2] == b'<';
        if starts_with_p || starts_with_name {
            let expr_start = if starts_with_name { 3 } else { 4 };
            let Some(end) = rest.iter().position(|&b| b == b'>') else {
                self.check_utf8(i, self.s.len())?;
                return Err(Error::new(Code::InvalidNamedCapture, &self.s[s..]));
            };
            let capture = &self.s[i..i + end + 1]; // "(?P<name>" or "(?<name>"
            let (name_lo, name_hi) = (i + expr_start, i + end);
            self.check_utf8(name_lo, name_hi)?;
            if !is_valid_capture_name(&self.s[name_lo..name_hi]) {
                return Err(Error::new(Code::InvalidNamedCapture, capture));
            }
            self.num_cap += 1;
            let re = self.op(Op::LeftParen)?;
            self.nodes[re].cap = self.num_cap;
            self.nodes[re].name = self.s[name_lo..name_hi].to_vec();
            return Ok(i + end + 1);
        }

        // Non-capturing group, possibly twiddling flags.
        let mut t = i + 2;
        let mut flags = self.flags;
        let mut sign: i32 = 1;
        let mut saw_flag = false;
        while t < self.s.len() {
            let (c, nt) = self.next_rune(t, self.s.len())?;
            t = nt;
            match u32::try_from(c).ok().and_then(char::from_u32) {
                Some('i') => {
                    flags |= FOLD_CASE;
                    saw_flag = true;
                }
                Some('m') => {
                    flags &= !ONE_LINE;
                    saw_flag = true;
                }
                Some('s') => {
                    flags |= DOT_NL;
                    saw_flag = true;
                }
                Some('U') => {
                    flags |= NON_GREEDY;
                    saw_flag = true;
                }
                Some('-') => {
                    if sign < 0 {
                        break;
                    }
                    sign = -1;
                    // Invert so the |= above turn into &^=; inverted back below.
                    flags = !flags;
                    saw_flag = false;
                }
                Some(':') | Some(')') => {
                    if sign < 0 {
                        if !saw_flag {
                            break;
                        }
                        flags = !flags;
                    }
                    if c == i32::from(b':') {
                        self.op(Op::LeftParen)?; // open new group
                    }
                    self.flags = flags;
                    return Ok(t);
                }
                _ => break,
            }
        }
        Err(Error::new(Code::InvalidPerlOp, &self.s[s..t]))
    }

    /// An escape sequence at `i` (which points at the backslash).
    fn parse_escape(&self, i: usize) -> PResult<(i32, usize)> {
        let s = i;
        let mut t = i + 1;
        if t >= self.s.len() {
            return Err(Error::new(Code::TrailingBackslash, b""));
        }
        let (c, nt) = self.next_rune(t, self.s.len())?;
        t = nt;

        match c {
            // Octal escapes. A single non-zero digit is a backreference, which
            // Go does not support, so it falls through to the error.
            c @ (0x31..=0x37) if t < self.s.len() && (b'0'..=b'7').contains(&self.s[t]) => {
                let mut r = c - i32::from(b'0');
                for _ in 1..3 {
                    if t >= self.s.len() || !(b'0'..=b'7').contains(&self.s[t]) {
                        break;
                    }
                    r = r * 8 + i32::from(self.s[t]) - i32::from(b'0');
                    t += 1;
                }
                return Ok((r, t));
            }
            0x30 => {
                // '0': consume up to three octal digits, one already read.
                let mut r = 0;
                for _ in 1..3 {
                    if t >= self.s.len() || !(b'0'..=b'7').contains(&self.s[t]) {
                        break;
                    }
                    r = r * 8 + i32::from(self.s[t]) - i32::from(b'0');
                    t += 1;
                }
                return Ok((r, t));
            }
            // Hexadecimal escapes.
            0x78 => {
                // 'x'
                if t < self.s.len() {
                    let (c, nt) = self.next_rune(t, self.s.len())?;
                    t = nt;
                    if c == i32::from(b'{') {
                        // Any number of hex digits in braces, at least one.
                        let mut nhex = 0;
                        let mut r: i32 = 0;
                        let mut ok = false;
                        loop {
                            if t >= self.s.len() {
                                break;
                            }
                            let (c, nt) = self.next_rune(t, self.s.len())?;
                            t = nt;
                            if c == i32::from(b'}') {
                                ok = true;
                                break;
                            }
                            let v = unhex(c);
                            if v < 0 {
                                break;
                            }
                            r = r.saturating_mul(16).saturating_add(v);
                            if r > MAX_RUNE {
                                break;
                            }
                            nhex += 1;
                        }
                        if ok && nhex > 0 {
                            return Ok((r, t));
                        }
                    } else {
                        // Easy case: two hex digits.
                        let x = unhex(c);
                        let (c, nt) = self.next_rune(t, self.s.len())?;
                        t = nt;
                        let y = unhex(c);
                        if x >= 0 && y >= 0 {
                            return Ok((x * 16 + y, t));
                        }
                    }
                }
            }
            // C escapes. There is deliberately no 'b': \b is a word boundary.
            0x61 => return Ok((0x07, t)), // \a
            0x66 => return Ok((0x0C, t)), // \f
            0x6E => return Ok((0x0A, t)), // \n
            0x72 => return Ok((0x0D, t)), // \r
            0x74 => return Ok((0x09, t)), // \t
            0x76 => return Ok((0x0B, t)), // \v
            _ => {
                if c < 0x80 && !is_alnum(c) {
                    // Escaped non-word characters are always themselves. PCRE
                    // also accepts things like \q; Go does not.
                    return Ok((c, t));
                }
            }
        }
        Err(Error::new(Code::InvalidEscape, &self.s[s..t]))
    }

    fn parse_class_char(&self, i: usize, whole_class: usize) -> PResult<(i32, usize)> {
        if i >= self.s.len() {
            return Err(Error::new(Code::MissingBracket, &self.s[whole_class..]));
        }
        if self.s[i] == b'\\' {
            return self.parse_escape(i);
        }
        self.next_rune(i, self.s.len())
    }

    /// A Perl class escape like `\d` at `i`, appended to `class`.
    fn parse_perl_class_escape(&self, i: usize, class: &mut Vec<i32>) -> Option<usize> {
        if self.flags & PERL_X == 0 || self.s.len() - i < 2 || self.s[i] != b'\\' {
            return None;
        }
        let (sign, ranges) = perl_group(&self.s[i..i + 2])?;
        self.append_group(class, sign, ranges);
        Some(i + 2)
    }

    /// A POSIX named class like `[:alnum:]` at `i`, appended to `class`.
    fn parse_named_class(&self, i: usize, class: &mut Vec<i32>) -> PResult<Option<usize>> {
        if self.s.len() - i < 2 || self.s[i] != b'[' || self.s[i + 1] != b':' {
            return Ok(None);
        }
        let Some(k) = find(&self.s[i + 2..], b":]") else {
            return Ok(None);
        };
        let end = i + 2 + k + 2;
        let name = &self.s[i..end];
        let Some((sign, ranges)) = posix_group(name) else {
            return Err(Error::new(Code::InvalidCharRange, name));
        };
        self.append_group(class, sign, ranges);
        Ok(Some(end))
    }

    fn append_group(&self, r: &mut Vec<i32>, sign: i32, class: &[i32]) {
        if self.flags & FOLD_CASE == 0 {
            if sign < 0 {
                append_negated_class(r, class);
            } else {
                append_class(r, class);
            }
        } else {
            let mut tmp = Vec::new();
            append_folded_class(&mut tmp, class);
            clean_class(&mut tmp);
            if sign < 0 {
                append_negated_class(r, &tmp);
            } else {
                append_class(r, &tmp);
            }
        }
    }

    /// A Unicode class like `\p{Han}` at `i`, appended to `class`.
    fn parse_unicode_class(&self, i: usize, class: &mut Vec<i32>) -> PResult<Option<usize>> {
        if self.flags & UNICODE_GROUPS == 0
            || self.s.len() - i < 2
            || self.s[i] != b'\\'
            || (self.s[i + 1] != b'p' && self.s[i + 1] != b'P')
        {
            return Ok(None);
        }
        // Committed to parse or return an error.
        let mut sign: i32 = if self.s[i + 1] == b'P' { -1 } else { 1 };
        let mut t = i + 2;
        let (c, nt) = self.next_rune(t, self.s.len())?;
        t = nt;

        let seq;
        let (name_lo, name_hi);
        if c != i32::from(b'{') {
            // Single-letter name.
            seq = i..t;
            (name_lo, name_hi) = (i + 2, t);
        } else {
            let Some(k) = self.s[i..].iter().position(|&b| b == b'}') else {
                self.check_utf8(i, self.s.len())?;
                return Err(Error::new(Code::InvalidCharRange, &self.s[i..]));
            };
            let end = i + k;
            seq = i..end + 1;
            t = end + 1;
            (name_lo, name_hi) = (i + 3, end);
            self.check_utf8(name_lo, name_hi)?;
        }
        // \p{^Han} == \P{Han}.
        let mut name = &self.s[name_lo..name_hi];
        if !name.is_empty() && name[0] == b'^' {
            sign = -sign;
            name = &name[1..];
        }

        let Some((tab, fold, tsign)) = unicode_table(name) else {
            return Err(Error::new(Code::InvalidCharRange, &self.s[seq]));
        };
        if tsign < 0 {
            sign = -sign;
        }

        match fold {
            Some(fold) if self.flags & FOLD_CASE != 0 => {
                // Merge and clean tab and fold in a temporary buffer: needed for
                // the negative case, tidy for the positive one.
                let mut tmp = Vec::new();
                append_table(&mut tmp, tab);
                append_table(&mut tmp, fold);
                clean_class(&mut tmp);
                if sign > 0 {
                    append_class(class, &tmp);
                } else {
                    append_negated_class(class, &tmp);
                }
            }
            _ => {
                if sign > 0 {
                    append_table(class, tab);
                } else {
                    append_negated_table(class, tab);
                }
            }
        }
        Ok(Some(t))
    }

    /// A character class at `i` (which points at the `[`), pushed on the stack.
    fn parse_class(&mut self, i: usize) -> PResult<usize> {
        let whole_class = i;
        let mut t = i + 1; // chop [
        let re = self.new_regexp(Op::CharClass);
        self.nodes[re].flags = self.flags;
        let mut class: Vec<i32> = Vec::new();

        let mut sign: i32 = 1;
        if t < self.s.len() && self.s[t] == b'^' {
            sign = -1;
            t += 1;
            // If the class does not match \n, add it now so the negation below
            // does the right thing.
            if self.flags & CLASS_NL == 0 {
                class.push(i32::from(b'\n'));
                class.push(i32::from(b'\n'));
            }
        }

        let mut first = true; // ] and - are okay as the first char
        while t >= self.s.len() || self.s[t] != b']' || first {
            // POSIX: - is only okay unescaped first or last. Perl: anywhere.
            if t < self.s.len()
                && self.s[t] == b'-'
                && self.flags & PERL_X == 0
                && !first
                && (self.s.len() - t == 1 || self.s[t + 1] != b']')
            {
                let (_, size) = decode_rune(&self.s[t + 1..]);
                return Err(Error::new(Code::InvalidCharRange, &self.s[t..t + 1 + size]));
            }
            first = false;

            // Look for POSIX [:alnum:] etc.
            if self.s.len() - t > 2 && self.s[t] == b'[' && self.s[t + 1] == b':' {
                if let Some(nt) = self.parse_named_class(t, &mut class)? {
                    t = nt;
                    continue;
                }
            }
            // Look for a Unicode character group like \p{Han}.
            if let Some(nt) = self.parse_unicode_class(t, &mut class)? {
                t = nt;
                continue;
            }
            // Look for a Perl character class symbol.
            if let Some(nt) = self.parse_perl_class_escape(t, &mut class) {
                t = nt;
                continue;
            }

            // Single character or simple range.
            let rng = t;
            let (lo, nt) = self.parse_class_char(t, whole_class)?;
            t = nt;
            let mut hi = lo;
            // [a-] means (a|-), so check for the final ].
            if self.s.len() - t >= 2 && self.s[t] == b'-' && self.s[t + 1] != b']' {
                t += 1;
                let (h, nt) = self.parse_class_char(t, whole_class)?;
                hi = h;
                t = nt;
                if hi < lo {
                    return Err(Error::new(Code::InvalidCharRange, &self.s[rng..t]));
                }
            }
            if self.flags & FOLD_CASE == 0 {
                append_range(&mut class, lo, hi);
            } else {
                append_folded_range(&mut class, lo, hi);
            }
        }
        t += 1; // chop ]

        clean_class(&mut class);
        if sign < 0 {
            negate_class(&mut class);
        }
        self.nodes[re].rune = class;
        self.push(re)?;
        Ok(t)
    }

    // ------------------------------------------------------------- the loop

    fn parse(&mut self) -> PResult<NodeId> {
        let mut t = 0usize;
        let mut last_repeat: Option<usize> = None;

        while t < self.s.len() {
            let mut repeat: Option<usize> = None;
            match self.s[t] {
                b'(' => {
                    if self.flags & PERL_X != 0 && self.s.len() - t >= 2 && self.s[t + 1] == b'?' {
                        // Flag changes and non-capturing groups.
                        t = self.parse_perl_flags(t)?;
                    } else {
                        self.num_cap += 1;
                        let re = self.op(Op::LeftParen)?;
                        self.nodes[re].cap = self.num_cap;
                        t += 1;
                    }
                }
                b'|' => {
                    self.parse_vertical_bar()?;
                    t += 1;
                }
                b')' => {
                    self.parse_right_paren()?;
                    t += 1;
                }
                b'^' => {
                    let op = if self.flags & ONE_LINE != 0 {
                        Op::BeginText
                    } else {
                        Op::BeginLine
                    };
                    self.op(op)?;
                    t += 1;
                }
                b'$' => {
                    if self.flags & ONE_LINE != 0 {
                        let re = self.op(Op::EndText)?;
                        self.nodes[re].flags |= WAS_DOLLAR;
                    } else {
                        self.op(Op::EndLine)?;
                    }
                    t += 1;
                }
                b'.' => {
                    let op = if self.flags & DOT_NL != 0 {
                        Op::AnyChar
                    } else {
                        Op::AnyCharNotNl
                    };
                    self.op(op)?;
                    t += 1;
                }
                b'[' => t = self.parse_class(t)?,
                c @ (b'*' | b'+' | b'?') => {
                    let before = t;
                    let op = match c {
                        b'*' => Op::Star,
                        b'+' => Op::Plus,
                        _ => Op::Quest,
                    };
                    let after = t + 1;
                    let after = self.repeat(op, 0, 0, before, after, last_repeat)?;
                    repeat = Some(before);
                    t = after;
                }
                b'{' => {
                    let before = t;
                    let Some((min, max, after)) = self.parse_repeat(t) else {
                        // If the repeat cannot be parsed, { is a literal.
                        self.literal(i32::from(b'{'))?;
                        t += 1;
                        last_repeat = repeat;
                        continue;
                    };
                    if min < 0 || min > 1000 || max > 1000 || (max >= 0 && min > max) {
                        // Numbers too big, or max present and min > max.
                        return Err(Error::new(
                            Code::InvalidRepeatSize,
                            &self.s[before..after],
                        ));
                    }
                    let after = self.repeat(Op::Repeat, min, max, before, after, last_repeat)?;
                    repeat = Some(before);
                    t = after;
                }
                b'\\' => {
                    let mut handled = true;
                    if self.flags & PERL_X != 0 && self.s.len() - t >= 2 {
                        match self.s[t + 1] {
                            b'A' => {
                                self.op(Op::BeginText)?;
                                t += 2;
                            }
                            b'b' => {
                                self.op(Op::WordBoundary)?;
                                t += 2;
                            }
                            b'B' => {
                                self.op(Op::NoWordBoundary)?;
                                t += 2;
                            }
                            b'C' => {
                                // any byte; not supported
                                return Err(Error::new(Code::InvalidEscape, &self.s[t..t + 2]));
                            }
                            b'Q' => {
                                // \Q ... \E: the ... is always literals.
                                let (lit_lo, lit_hi, next) = match find(&self.s[t + 2..], b"\\E") {
                                    Some(k) => (t + 2, t + 2 + k, t + 2 + k + 2),
                                    None => (t + 2, self.s.len(), self.s.len()),
                                };
                                let mut li = lit_lo;
                                while li < lit_hi {
                                    let (c, ni) = self.next_rune(li, lit_hi)?;
                                    self.literal(c)?;
                                    li = ni;
                                }
                                t = next;
                            }
                            b'z' => {
                                self.op(Op::EndText)?;
                                t += 2;
                            }
                            _ => handled = false,
                        }
                    } else {
                        handled = false;
                    }
                    if handled {
                        last_repeat = repeat;
                        continue;
                    }

                    let re = self.new_regexp(Op::CharClass);
                    self.nodes[re].flags = self.flags;

                    // Look for a Unicode character group like \p{Han}.
                    if self.s.len() - t >= 2 && (self.s[t + 1] == b'p' || self.s[t + 1] == b'P') {
                        let mut class = Vec::new();
                        if let Some(rest) = self.parse_unicode_class(t, &mut class)? {
                            self.nodes[re].rune = class;
                            t = rest;
                            self.push(re)?;
                            last_repeat = repeat;
                            continue;
                        }
                    }
                    // Perl character class escape.
                    let mut class = Vec::new();
                    if let Some(rest) = self.parse_perl_class_escape(t, &mut class) {
                        self.nodes[re].rune = class;
                        t = rest;
                        self.push(re)?;
                        last_repeat = repeat;
                        continue;
                    }
                    self.reuse(re);

                    // Ordinary single-character escape.
                    let (c, nt) = self.parse_escape(t)?;
                    t = nt;
                    self.literal(c)?;
                }
                _ => {
                    let (c, nt) = self.next_rune(t, self.s.len())?;
                    t = nt;
                    self.literal(c)?;
                }
            }
            last_repeat = repeat;
        }

        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop(); // pop vertical bar
        }
        self.alternate()?;

        if self.stack.len() != 1 {
            return Err(Error::new(Code::MissingParen, self.s));
        }
        Ok(self.stack[0])
    }
}

// -------------------------------------------------------------- byte helpers

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `utf8.DecodeRuneInString`: `(RuneError, 1)` for any ill-formed sequence,
/// `(RuneError, 0)` for empty input — the two are distinguished by size, which
/// is how Go tells "invalid" from "nothing there".
fn decode_rune(b: &[u8]) -> (i32, usize) {
    if b.is_empty() {
        return (RUNE_ERROR, 0);
    }
    if b[0] < 0x80 {
        return (i32::from(b[0]), 1);
    }
    for n in 2..=4.min(b.len()) {
        if let Ok(s) = std::str::from_utf8(&b[..n]) {
            if let Some(c) = s.chars().next() {
                return (c as i32, n);
            }
        }
    }
    (RUNE_ERROR, 1)
}

fn is_alnum(c: i32) -> bool {
    (0x30..=0x39).contains(&c) || (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

fn unhex(c: i32) -> i32 {
    match c {
        0x30..=0x39 => c - 0x30,
        0x61..=0x66 => c - 0x61 + 10,
        0x41..=0x46 => c - 0x41 + 10,
        _ => -1,
    }
}

/// `[A-Za-z0-9_]+`. PCRE caps the length and Python rejects a leading digit;
/// Go enforces neither.
fn is_valid_capture_name(name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < name.len() {
        let (c, size) = decode_rune(&name[i..]);
        if c != i32::from(b'_') && !is_alnum(c) {
            return false;
        }
        i += size.max(1);
    }
    true
}

/// Decimal integer, no leading zeros, `-1` once it would overflow `1e8`.
fn parse_int(s: &[u8], i: usize) -> Option<(i32, usize)> {
    if i >= s.len() || !s[i].is_ascii_digit() {
        return None;
    }
    if s.len() - i >= 2 && s[i] == b'0' && s[i + 1].is_ascii_digit() {
        return None; // disallow leading zeros
    }
    let mut j = i;
    while j < s.len() && s[j].is_ascii_digit() {
        j += 1;
    }
    let mut n: i32 = 0;
    for &b in &s[i..j] {
        if n >= 100_000_000 {
            n = -1;
            break;
        }
        n = n * 10 + i32::from(b - b'0');
    }
    Some((n, j))
}

// ------------------------------------------------------------- char classes

fn append_range(r: &mut Vec<i32>, lo: i32, hi: i32) {
    // Check the last two ranges, not just the last: appending a case-folded
    // alphabet grows an A-Z range and an a-z range alternately.
    let n = r.len();
    let mut i = 2;
    while i <= 4 {
        if n >= i {
            let (rlo, rhi) = (r[n - i], r[n - i + 1]);
            if lo <= rhi + 1 && rlo <= hi + 1 {
                if lo < rlo {
                    r[n - i] = lo;
                }
                if hi > rhi {
                    r[n - i + 1] = hi;
                }
                return;
            }
        }
        i += 2;
    }
    r.push(lo);
    r.push(hi);
}

/// Minimum and maximum runes involved in folding, as `parse.go` pins them.
const MIN_FOLD: i32 = 0x0041;
const MAX_FOLD: i32 = 0x1e943;

fn append_folded_range(r: &mut Vec<i32>, lo: i32, hi: i32) {
    if lo <= MIN_FOLD && hi >= MAX_FOLD {
        append_range(r, lo, hi); // range is full; folding adds nothing
        return;
    }
    if hi < MIN_FOLD || lo > MAX_FOLD {
        append_range(r, lo, hi); // outside folding possibilities
        return;
    }
    let (mut lo, mut hi) = (lo, hi);
    if lo < MIN_FOLD {
        append_range(r, lo, MIN_FOLD - 1);
        lo = MIN_FOLD;
    }
    if hi > MAX_FOLD {
        append_range(r, MAX_FOLD + 1, hi);
        hi = MAX_FOLD;
    }
    // Brute force; append_range coalesces on the fly.
    let mut c = lo;
    while c <= hi {
        append_range(r, c, c);
        let mut f = simple_fold(c);
        while f != c {
            append_range(r, f, f);
            f = simple_fold(f);
        }
        c += 1;
    }
}

fn append_literal(r: &mut Vec<i32>, x: i32, flags: u16) {
    if flags & FOLD_CASE != 0 {
        append_folded_range(r, x, x);
    } else {
        append_range(r, x, x);
    }
}

fn append_class(r: &mut Vec<i32>, x: &[i32]) {
    for c in x.chunks(2) {
        append_range(r, c[0], c[1]);
    }
}

fn append_folded_class(r: &mut Vec<i32>, x: &[i32]) {
    for c in x.chunks(2) {
        append_folded_range(r, c[0], c[1]);
    }
}

fn append_negated_class(r: &mut Vec<i32>, x: &[i32]) {
    let mut next_lo = 0;
    for c in x.chunks(2) {
        let (lo, hi) = (c[0], c[1]);
        if next_lo <= lo - 1 {
            append_range(r, next_lo, lo - 1);
        }
        next_lo = hi + 1;
    }
    if next_lo <= MAX_RUNE {
        append_range(r, next_lo, MAX_RUNE);
    }
}

fn append_table(r: &mut Vec<i32>, x: &[(u32, u32, u32)]) {
    for &(lo, hi, stride) in x {
        let (lo, hi, stride) = (lo as i32, hi as i32, stride as i32);
        if stride == 1 {
            append_range(r, lo, hi);
            continue;
        }
        let mut c = lo;
        while c <= hi {
            append_range(r, c, c);
            c += stride;
        }
    }
}

fn append_negated_table(r: &mut Vec<i32>, x: &[(u32, u32, u32)]) {
    let mut next_lo = 0;
    for &(lo, hi, stride) in x {
        let (lo, hi, stride) = (lo as i32, hi as i32, stride as i32);
        if stride == 1 {
            if next_lo <= lo - 1 {
                append_range(r, next_lo, lo - 1);
            }
            next_lo = hi + 1;
            continue;
        }
        let mut c = lo;
        while c <= hi {
            if next_lo <= c - 1 {
                append_range(r, next_lo, c - 1);
            }
            next_lo = c + 1;
            c += stride;
        }
    }
    if next_lo <= MAX_RUNE {
        append_range(r, next_lo, MAX_RUNE);
    }
}

/// Sorts the pairs, merges them, and drops duplicates, in place.
fn clean_class(r: &mut Vec<i32>) {
    let mut pairs: Vec<(i32, i32)> = r.chunks(2).map(|c| (c[0], c[1])).collect();
    // Sort by lo increasing, hi decreasing to break ties.
    pairs.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    for (i, p) in pairs.iter().enumerate() {
        r[2 * i] = p.0;
        r[2 * i + 1] = p.1;
    }
    if r.len() < 2 {
        return;
    }
    let mut w = 2; // write index
    let mut i = 2;
    while i < r.len() {
        let (lo, hi) = (r[i], r[i + 1]);
        if lo <= r[w - 1] + 1 {
            if hi > r[w - 1] {
                r[w - 1] = hi; // merge with previous range
            }
            i += 2;
            continue;
        }
        r[w] = lo;
        r[w + 1] = hi;
        w += 2;
        i += 2;
    }
    r.truncate(w);
}

/// Overwrites `r` with its negation. Assumes `r` is clean.
fn negate_class(r: &mut Vec<i32>) {
    let mut next_lo = 0;
    let mut w = 0;
    let mut i = 0;
    while i < r.len() {
        let (lo, hi) = (r[i], r[i + 1]);
        if next_lo <= lo - 1 {
            r[w] = next_lo;
            r[w + 1] = lo - 1;
            w += 2;
        }
        next_lo = hi + 1;
        i += 2;
    }
    r.truncate(w);
    if next_lo <= MAX_RUNE {
        r.push(next_lo);
        r.push(MAX_RUNE);
    }
}

// ----------------------------------------------------------------- unicode

/// `unicode.SimpleFold`. The table holds only the runes that do not map to
/// themselves; everything else is the identity.
fn simple_fold(r: i32) -> i32 {
    if r < 0 || r > MAX_RUNE {
        return r;
    }
    let key = r as u32;
    match SIMPLE_FOLD.binary_search_by_key(&key, |&(k, _)| k) {
        Ok(i) => SIMPLE_FOLD[i].1 as i32,
        Err(_) => r,
    }
}

/// The minimum rune fold-equivalent to `r`.
fn min_fold_rune(r: i32) -> i32 {
    if r < MIN_FOLD || r > MAX_FOLD {
        return r;
    }
    let mut m = r;
    let r0 = r;
    let mut c = simple_fold(r);
    while c != r0 {
        m = m.min(c);
        c = simple_fold(c);
    }
    m
}

/// The canonical lookup string for `name`: a leading uppercase letter, then
/// lowercase, with underscores, spaces and hyphens dropped.
fn canonical_name(name: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(name.len());
    let mut first = true;
    for &b in name {
        let c = match b {
            b'_' | b'-' | b' ' => continue,
            b'a'..=b'z' if first => b - (b'a' - b'A'),
            b'A'..=b'Z' if !first => b + (b'a' - b'A'),
            other => other,
        };
        first = false;
        out.push(c);
    }
    out
}

const ANY_TABLE: &[(u32, u32, u32)] = &[(0, (1 << 16) - 1, 1), (1 << 16, 0x10_FFFF, 1)];
const ASCII_TABLE: &[(u32, u32, u32)] = &[(0, 0x7F, 1)];
const ASCII_FOLD_TABLE: &[(u32, u32, u32)] = &[
    (0, 0x7F, 1),
    (0x017F, 0x017F, 1), // Old English long s (ſ), folds to S/s
    (0x212A, 0x212A, 1), // Kelvin K, folds to K/k
];

fn lookup<'a, T: Copy>(table: &'a [(&'static str, T)], name: &[u8]) -> Option<T> {
    table
        .binary_search_by_key(&name, |&(k, _)| k.as_bytes())
        .ok()
        .map(|i| table[i].1)
}

type UnicodeTable = &'static [(u32, u32, u32)];

/// The range table `name` identifies, plus its fold-equivalent table and the
/// sign to apply. `None` means there is no such name.
fn unicode_table(name: &[u8]) -> Option<(UnicodeTable, Option<UnicodeTable>, i32)> {
    let name = canonical_name(name);
    // Special cases: Any, Assigned and ASCII. LC is the only non-canonical
    // Categories key, so it is handled here too.
    match name.as_slice() {
        b"Any" => return Some((ANY_TABLE, Some(ANY_TABLE), 1)),
        b"Assigned" => {
            // Invert Cn (unassigned).
            let cn = lookup(CATEGORIES, b"Cn").expect("unicode.Cn");
            return Some((cn, Some(cn), -1));
        }
        b"Ascii" => return Some((ASCII_TABLE, Some(ASCII_FOLD_TABLE), 1)),
        b"Lc" => {
            let t = lookup(CATEGORIES, b"LC")?;
            return Some((t, lookup(FOLD_CATEGORY, b"LC"), 1));
        }
        _ => {}
    }
    if let Some(t) = lookup(CATEGORIES, &name) {
        return Some((t, lookup(FOLD_CATEGORY, &name), 1));
    }
    if let Some(t) = lookup(SCRIPTS, &name) {
        return Some((t, lookup(FOLD_SCRIPT, &name), 1));
    }
    // unicode.CategoryAliases uses underscores that the canonical form drops,
    // so its keys are canonicalised here rather than compared raw.
    for &(alias, actual) in CATEGORY_ALIASES {
        if canonical_name(alias.as_bytes()) == name {
            let t = lookup(CATEGORIES, actual.as_bytes())?;
            return Some((t, lookup(FOLD_CATEGORY, actual.as_bytes()), 1));
        }
    }
    None
}

// ------------------------------------------------------- perl / posix groups

const CODE_D: &[i32] = &[0x30, 0x39];
const CODE_S: &[i32] = &[0x9, 0xa, 0xc, 0xd, 0x20, 0x20];
const CODE_W: &[i32] = &[0x30, 0x39, 0x41, 0x5a, 0x5f, 0x5f, 0x61, 0x7a];

fn perl_group(s: &[u8]) -> Option<(i32, &'static [i32])> {
    if s.len() != 2 || s[0] != b'\\' {
        return None;
    }
    match s[1] {
        b'd' => Some((1, CODE_D)),
        b'D' => Some((-1, CODE_D)),
        b's' => Some((1, CODE_S)),
        b'S' => Some((-1, CODE_S)),
        b'w' => Some((1, CODE_W)),
        b'W' => Some((-1, CODE_W)),
        _ => None,
    }
}

/// `posixGroup` from `perl_groups.go`, keyed on the bracketed name.
fn posix_group(name: &[u8]) -> Option<(i32, &'static [i32])> {
    const GROUPS: &[(&[u8], &[i32])] = &[
        (b"alnum", &[0x30, 0x39, 0x41, 0x5a, 0x61, 0x7a]),
        (b"alpha", &[0x41, 0x5a, 0x61, 0x7a]),
        (b"ascii", &[0x0, 0x7f]),
        (b"blank", &[0x9, 0x9, 0x20, 0x20]),
        (b"cntrl", &[0x0, 0x1f, 0x7f, 0x7f]),
        (b"digit", &[0x30, 0x39]),
        (b"graph", &[0x21, 0x7e]),
        (b"lower", &[0x61, 0x7a]),
        (b"print", &[0x20, 0x7e]),
        (b"punct", &[0x21, 0x2f, 0x3a, 0x40, 0x5b, 0x60, 0x7b, 0x7e]),
        (b"space", &[0x9, 0xd, 0x20, 0x20]),
        (b"upper", &[0x41, 0x5a]),
        (b"word", &[0x30, 0x39, 0x41, 0x5a, 0x5f, 0x5f, 0x61, 0x7a]),
        (b"xdigit", &[0x30, 0x39, 0x41, 0x46, 0x61, 0x66]),
    ];
    let inner = name.strip_prefix(b"[:")?.strip_suffix(b":]")?;
    let (sign, inner) = match inner.strip_prefix(b"^") {
        Some(rest) => (-1, rest),
        None => (1, inner),
    };
    GROUPS
        .iter()
        .find(|(n, _)| *n == inner)
        .map(|&(_, class)| (sign, class))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn err(pattern: &str) -> String {
        match compile(pattern) {
            Ok(()) => panic!("{pattern:?} unexpectedly compiled"),
            Err(Some(msg)) => msg,
            Err(None) => panic!("{pattern:?} was undecided"),
        }
    }

    #[test]
    fn error_text_matches_go() {
        assert_eq!(err("foo("), "error parsing regexp: missing closing ): `foo(`");
        assert_eq!(err("["), "error parsing regexp: missing closing ]: `[`");
        assert_eq!(err("a)"), "error parsing regexp: unexpected ): `a)`");
        assert_eq!(
            err("*"),
            "error parsing regexp: missing argument to repetition operator: `*`"
        );
        assert_eq!(
            err(r"\"),
            "error parsing regexp: trailing backslash at end of expression: ``"
        );
        assert_eq!(
            err(r"\p{Foo}"),
            "error parsing regexp: invalid character class range: `\\p{Foo}`"
        );
    }

    /// The port answers on bytes, which is what Go's `string` is. SA1000 cannot
    /// currently reach this: `guff-constant` models a Go string constant as a
    /// Rust `String`, so `"\xff"` in source arrives here as U+00FF and compiles
    /// fine, where upstream reports `invalid UTF-8`. The gap is in the constant
    /// layer, not here — see docs/COMPAT-HARDENING.md §7.
    #[test]
    fn ill_formed_utf8_is_an_error_on_the_byte_api() {
        assert_eq!(
            compile_bytes(b"a\xff"),
            CompileResult::Invalid(b"error parsing regexp: invalid UTF-8: `\xff`".to_vec())
        );
        // What the constant layer actually delivers today, and why SA1000 is
        // silent on the source above: U+00FF is an ordinary rune.
        assert_eq!(compile_bytes("a\u{00FF}".as_bytes()), CompileResult::Valid);
    }

    #[test]
    fn go_only_syntax_is_accepted() {
        // Go treats non-quantifier braces as literals and allows a perl class
        // as a range endpoint; the Rust regex crate rejects both, which is what
        // the approximation this port replaced carried exceptions for.
        assert!(compile(r"{header\.([\w-]*)}").is_ok());
        assert!(compile(r"{re\.([\w-\.]*)}").is_ok());
        assert!(compile(r"[-\/^$+?.()|[\]{}]").is_ok());
    }
}
