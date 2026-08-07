//! Port of [`github.com/denis-tingaikin/go-header`](https://github.com/denis-tingaikin/go-header)
//! **v0.5.0** — the exact version golangci-lint 2.12.2 pins — together with the
//! wrapper in `golangci-lint/pkg/golinters/goheader`, which supplies the
//! reported position.
//!
//! Checks that each `.go` file's leading copyright / license header matches
//! a configured template. Without `template` / `template-path`, the analyzer
//! is a no-op (the golangci wrapper returns before constructing the analyzer).
//!
//! # Why this is a character-by-character reader and not a regexp
//!
//! Upstream walks the template and the header **in parallel**, one byte at a
//! time, and reports the exact point where they diverge:
//!
//! | condition | message |
//! |---|---|
//! | bytes differ | `Actual: <rest of header line>\nExpected:<rest of template line>` |
//! | header outlives template | `Unexpected string: <remainder of header>` |
//! | template outlives header | `Missed string: <remainder of template>` |
//! | `{{ v }}` const mismatch | `Expected:<value>, Actual: <rest of header line>` |
//! | `{{ v }}` regexp mismatch | `Pattern <re> doesn't match.` |
//! | no header / blank header | `Missed header for check` |
//! | `{{ v }}` undefined | `Template has unknown value: <name>` |
//!
//! A single whole-header regexp — what this module used to do — can produce the
//! boolean but not the position or the text, so every mismatch collapsed into
//! one `template doesn't match` at the head of the comment.
//!
//! # Position
//!
//! The reported position is *not* the position of the divergence in the file.
//! The wrapper computes a raw byte offset
//!
//! ```text
//! LineStart(loc.Line + 1) + (loc.Position - offset)
//! ```
//!
//! where `loc` is the reader's location **within the header text** and
//! `LineStart` indexes lines of the **whole file**. `loc.Position` already
//! carries a fixed fudge (`+4` for `//` headers, `+1` for `/* */`) that stands
//! in for the comment marker, and the wrapper subtracts `1` again for `//`.
//! Because the two coordinate spaces are mixed, a header that does not begin on
//! line 1 reports a position that walks off its line — upstream behaviour that
//! is reproduced here deliberately. The golden case pins it:
//! `compat/golden/cases/goheader/` has `offset_header.go`, whose header starts
//! on line 3 and which both tools report at `3:17` rather than `3:19`.
//!
//! That same arithmetic doubles as upstream's build-directive filter: an issue
//! carrying no location (`Location{0,0}`) against a `//` header yields
//! `0 - 1 < 0` and is **dropped**. That is the only reason a file whose first
//! comment is `//go:build …` reports nothing.
//!
//! DEFERRED (see DEVELOPMENT.md R13/R14):
//! - SuggestedFix / `--fix` (upstream `generateFix`)
//! - `template-path` `${config-path}` / `${base-path}` placeholders
//! - custom `delims`, `vars` alias
//! - `mod-year` / `mod-year-range` resolve from the file's mtime; upstream
//!   prefers the file's **git commit** year and falls back to mtime. Templates
//!   using them are therefore environment-dependent on both sides, and differ
//!   from upstream inside a git checkout.

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use guff::ast::{Comment, CommentGroup, File};
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::FileSet;
use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};
use regex::Regex;

use crate::options::GoheaderOptions;

// ---------------------------------------------------------------------------
// Reader — upstream `reader.go`
// ---------------------------------------------------------------------------

/// Location within the text being read. Both fields are 0-based, matching
/// upstream's `Location`; `line` is rendered as `line + 1` by the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Loc {
    line: i64,
    col: i64,
}

/// Byte-oriented cursor over a string.
///
/// Upstream's `Peek` is `rune(r.source[r.position])` — it widens a single
/// **byte** to a rune rather than decoding UTF-8 — and `Next` advances by one
/// byte. Both sides of the main comparison are bytes, so multi-byte text
/// matches fine there; the asymmetry only shows up in [`Matcher::read_const`],
/// which is faithfully reproduced.
struct Reader<'a> {
    source: &'a [u8],
    position: usize,
    location: Loc,
    offset: Loc,
}

impl<'a> Reader<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            position: 0,
            location: Loc::default(),
            offset: Loc::default(),
        }
    }

    fn set_offset(&mut self, offset: Loc) {
        self.offset = offset;
    }

    fn position(&self) -> usize {
        self.position
    }

    /// Upstream `Location()` = raw location + offset.
    fn location(&self) -> Loc {
        Loc {
            line: self.location.line + self.offset.line,
            col: self.location.col + self.offset.col,
        }
    }

    fn done(&self) -> bool {
        self.position >= self.source.len()
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.peek()?;
        if b == b'\n' {
            self.location.line += 1;
            self.location.col = 0;
        } else {
            self.location.col += 1;
        }
        self.position += 1;
        Some(b)
    }

    /// Jump to `pos`, recomputing the location from the start of the source
    /// (upstream `SetPosition` → `calculateLocation`).
    fn set_position(&mut self, pos: usize) {
        let end = pos.min(self.source.len());
        let mut loc = Loc::default();
        for &b in &self.source[..end] {
            if b == b'\n' {
                loc.line += 1;
                loc.col = 0;
            } else {
                loc.col += 1;
            }
        }
        self.position = pos;
        self.location = loc;
    }

    fn read_while(&mut self, pred: impl Fn(u8) -> bool) -> &'a [u8] {
        let source = self.source;
        let start = self.position;
        while let Some(b) = self.peek() {
            if !pred(b) {
                break;
            }
            self.next();
        }
        &source[start..self.position]
    }

    /// Remainder of the source; leaves the reader at the end.
    fn finish(&mut self) -> &'a [u8] {
        let source = self.source;
        if self.position >= source.len() {
            return &[];
        }
        let start = self.position;
        self.set_position(source.len());
        &source[start..]
    }
}

/// Reader contents are always slices of `str`, so this never allocates a
/// replacement. Kept lossy so a malformed file can never panic the analyzer.
fn as_str(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    String::from_utf8_lossy(bytes)
}

// ---------------------------------------------------------------------------
// Values — upstream `value.go` / `config.go`
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Const,
    Regexp,
}

#[derive(Debug, Clone)]
struct Value {
    kind: ValueKind,
    raw: String,
    calculated: String,
}

impl Value {
    fn new(kind: ValueKind, raw: impl Into<String>) -> Self {
        Self {
            kind,
            raw: raw.into(),
            calculated: String::new(),
        }
    }

    /// Upstream `Get()`: the calculated form, or the raw one while it is empty.
    fn get(&self) -> &str {
        if self.calculated.is_empty() {
            &self.raw
        } else {
            &self.calculated
        }
    }
}

/// Resolve `{{ name }}` references inside a raw value.
///
/// Mirrors upstream `calculateValue`, including its habit of searching for the
/// closing `}}` from the start of the remaining string rather than from the
/// opening `{{`. Upstream panics on the resulting inverted slice; we report a
/// config error instead.
fn calculate_value(
    raw: &str,
    values: &HashMap<String, Value>,
    resolved: &mut HashMap<String, String>,
    visiting: &mut Vec<String>,
) -> Result<String, String> {
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let Some(end) = rest.find("}}") else {
            return Err("missed value ending".into());
        };
        if end < start + 2 {
            return Err("missed value ending".into());
        }
        let name = rest[start + 2..end].trim().to_ascii_lowercase();
        out.push_str(&resolve(&name, values, resolved, visiting)?);
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Calculate `name`, recursing into whatever it references.
///
/// Upstream has no cycle guard here — a self-referencing value overflows the
/// stack. We return an error, which surfaces as a config error rather than a
/// crash.
fn resolve(
    name: &str,
    values: &HashMap<String, Value>,
    resolved: &mut HashMap<String, String>,
    visiting: &mut Vec<String>,
) -> Result<String, String> {
    if let Some(v) = resolved.get(name) {
        return Ok(v.clone());
    }
    let Some(value) = values.get(name) else {
        return Err(format!("unknown value name {name}"));
    };
    if visiting.iter().any(|v| v == name) {
        return Err(format!("recursive value name {name}"));
    }
    visiting.push(name.to_string());
    let calculated = calculate_value(&value.raw, values, resolved, visiting)?;
    visiting.pop();

    // Upstream `Get()` falls back to the raw value while the calculated one is
    // empty, so an empty calculation is indistinguishable from "not yet done".
    let effective = if calculated.is_empty() {
        value.raw.clone()
    } else {
        calculated
    };
    resolved.insert(name.to_string(), effective.clone());
    Ok(effective)
}

fn calculate_all(values: &mut HashMap<String, Value>) -> Result<(), String> {
    let snapshot = values.clone();
    let mut resolved: HashMap<String, String> = HashMap::new();
    let mut names: Vec<String> = snapshot.keys().cloned().collect();
    names.sort();
    for name in names {
        let mut visiting = Vec::new();
        let calculated = resolve(&name, &snapshot, &mut resolved, &mut visiting)?;
        if let Some(v) = values.get_mut(&name) {
            v.calculated = calculated;
        }
    }
    Ok(())
}

fn year_of(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let mut y = 1970i64;
    let mut rem = days;
    loop {
        let diy = if is_leap(y) { 366 } else { 365 };
        if rem < diy {
            break;
        }
        rem -= diy;
        y += 1;
    }
    y.to_string()
}

fn current_year() -> String {
    year_of(SystemTime::now())
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

const YEAR_RANGE_RAW: &str = r"((20\d\d\-{{YEAR}})|({{YEAR}}))";
const MOD_YEAR_RANGE_RAW: &str = r"((20\d\d\-{{mod-year}})|({{mod-year}}))";

/// Upstream `Configuration.builtInValues` — note the names are `year` and
/// `year-range`. There is no `YEAR_RANGE`; a template naming it reports
/// `Template has unknown value: year_range`.
fn built_in_values() -> HashMap<String, Value> {
    let mut m = HashMap::new();
    m.insert(
        "year".to_string(),
        Value::new(ValueKind::Const, current_year()),
    );
    m.insert(
        "year-range".to_string(),
        Value::new(ValueKind::Regexp, YEAR_RANGE_RAW),
    );
    m
}

/// Base value set: built-ins overlaid with the user's `const` / `regexp` maps.
/// Upstream lower-cases every key exactly once, here and at lookup.
fn build_values(opts: &GoheaderOptions) -> HashMap<String, Value> {
    let mut values = built_in_values();
    for (k, v) in &opts.const_values {
        values.insert(k.to_ascii_lowercase(), Value::new(ValueKind::Const, v));
    }
    for (k, v) in &opts.regexp_values {
        values.insert(k.to_ascii_lowercase(), Value::new(ValueKind::Regexp, v));
    }
    values
}

/// Upstream `processPerTargetValues`: `mod-year` shadows `year` unless the
/// file's modification time is available, then it wins.
fn per_target_values(
    base: &HashMap<String, Value>,
    path: &std::path::Path,
) -> Result<HashMap<String, Value>, String> {
    let mut values = base.clone();
    if let Some(year) = values.get("year").cloned() {
        values.insert("mod-year".to_string(), year);
    }
    if let Some(range) = values.get("year-range").cloned() {
        values.insert("mod-year-range".to_string(), range);
    }
    // DEFERRED: upstream prefers the git commit date and only falls back to
    // mtime. Shelling out to git per file is too costly to do unconditionally.
    if let Ok(modified) = fs::metadata(path).and_then(|m| m.modified()) {
        values.insert(
            "mod-year".to_string(),
            Value::new(ValueKind::Const, year_of(modified)),
        );
        values.insert(
            "mod-year-range".to_string(),
            Value::new(ValueKind::Regexp, MOD_YEAR_RANGE_RAW),
        );
    }
    calculate_all(&mut values)?;
    Ok(values)
}

// ---------------------------------------------------------------------------
// Issue
// ---------------------------------------------------------------------------

/// Upstream distinguishes `NewIssueWithLocation` from `NewIssue`; the latter
/// carries `Location{0,0}`, which the wrapper's `Position - offset < 0` test
/// then silently drops for `//`-style headers.
struct Issue {
    message: String,
    loc: Loc,
}

impl Issue {
    fn located(message: String, loc: Loc) -> Self {
        Self { message, loc }
    }

    fn bare(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            loc: Loc::default(),
        }
    }
}

// ---------------------------------------------------------------------------
// Matcher — upstream `Analyzer.Analyze`
// ---------------------------------------------------------------------------

struct Matcher<'a> {
    values: &'a HashMap<String, Value>,
    /// Compiled lazily: upstream `regexp.MustCompile`s on each use, so a value
    /// whose pattern is invalid but never referenced never raises anything.
    re_cache: HashMap<String, Regex>,
}

impl<'a> Matcher<'a> {
    fn new(values: &'a HashMap<String, Value>) -> Self {
        Self {
            values,
            re_cache: HashMap::new(),
        }
    }

    /// Upstream `readField`: consumes `{{`, reads to the first `}`, consumes
    /// `}}`. The name is lower-cased and trimmed but **not** stripped of a
    /// leading dot, so `{{ .YEAR }}` looks up `.year` and is unknown.
    fn read_field(t: &mut Reader<'_>) -> String {
        t.next();
        t.next();
        let name = t.read_while(|b| b != b'}').to_vec();
        t.next();
        t.next();
        as_str(&name).trim().to_ascii_lowercase()
    }

    /// `reader_offset` is upstream's `offset.Position` — see [`run`].
    fn analyze(
        &mut self,
        header: &str,
        template: &str,
        reader_offset: i64,
    ) -> Result<Option<Issue>, String> {
        if header.is_empty() {
            return Ok(Some(Issue::bare("Missed header for check")));
        }
        let mut s = Reader::new(header.as_bytes());
        s.set_offset(Loc {
            line: 0,
            col: reader_offset,
        });
        let mut t = Reader::new(template.as_bytes());

        while !s.done() && !t.done() {
            let tc = t.peek().unwrap_or(0);
            if tc == b'{' {
                let name = Self::read_field(&mut t);
                let Some(value) = self.values.get(&name) else {
                    return Ok(Some(Issue::bare(format!(
                        "Template has unknown value: {name}"
                    ))));
                };
                let value = value.clone();
                if let Some(issue) = self.read_value(&value, &mut s)? {
                    return Ok(Some(issue));
                }
                continue;
            }
            let sc = s.peek().unwrap_or(0);
            if sc != tc {
                let loc = s.location();
                let actual = s.read_while(|b| b != b'\n').to_vec();
                let expected = t.read_while(|b| b != b'\n').to_vec();
                return Ok(Some(Issue::located(
                    format!(
                        "Actual: {}\nExpected:{}",
                        as_str(&actual),
                        as_str(&expected)
                    ),
                    loc,
                )));
            }
            s.next();
            t.next();
        }

        if !s.done() {
            let loc = s.location();
            let rest = s.finish().to_vec();
            return Ok(Some(Issue::located(
                format!("Unexpected string: {}", as_str(&rest)),
                loc,
            )));
        }
        if !t.done() {
            let loc = s.location();
            let rest = t.finish().to_vec();
            return Ok(Some(Issue::located(
                format!("Missed string: {}", as_str(&rest)),
                loc,
            )));
        }
        Ok(None)
    }

    fn read_value(&mut self, value: &Value, s: &mut Reader<'_>) -> Result<Option<Issue>, String> {
        match value.kind {
            ValueKind::Const => Ok(Self::read_const(value, s)),
            ValueKind::Regexp => self.read_regexp(value, s),
        }
    }

    /// Upstream `ConstValue.Read`.
    ///
    /// Note the deliberate rune/byte asymmetry: upstream ranges over the
    /// value's **runes** while `Peek` yields a single **byte**, so a const
    /// value containing non-ASCII can never match. Reproduced.
    fn read_const(value: &Value, s: &mut Reader<'_>) -> Option<Issue> {
        let loc = s.location();
        let start = s.position();
        let expected = value.get().to_string();
        for ch in expected.chars() {
            if s.peek().map(u32::from) != Some(ch as u32) {
                s.set_position(start);
                let actual = s.read_while(|b| b != b'\n').to_vec();
                return Some(Issue::located(
                    format!("Expected:{}, Actual: {}", expected, as_str(&actual)),
                    loc,
                ));
            }
            s.next();
        }
        None
    }

    /// Upstream `RegexpValue.Read`.
    ///
    /// The pattern is **not** anchored: it finds its first match anywhere in
    /// the remainder of the header and moves the cursor to the end of that
    /// match, so a regexp value can skip over arbitrary text.
    fn read_regexp(&mut self, value: &Value, s: &mut Reader<'_>) -> Result<Option<Issue>, String> {
        let loc = s.location();
        let pattern = value.get().to_string();
        if !self.re_cache.contains_key(&pattern) {
            let re = Regex::new(&pattern)
                .map_err(|e| format!("goheader value regexp {pattern}: {e}"))?;
            self.re_cache.insert(pattern.clone(), re);
        }
        let re = &self.re_cache[&pattern];

        let start = s.position();
        let rest = s.finish().to_vec();
        s.set_position(start);
        match re.find(&as_str(&rest)) {
            None => Ok(Some(Issue::located(
                format!("Pattern {pattern} doesn't match."),
                loc,
            ))),
            Some(m) => {
                s.set_position(start + m.end());
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Header extraction — upstream `Analyze` prologue
// ---------------------------------------------------------------------------

/// The comment group upstream treats as the header: the **first** one before
/// the `package` clause, whatever it contains.
///
/// No skipping. A file whose first comment is `//go:build` has no header as far
/// as go-header is concerned — not "look further down for a licence block".
/// `// +build` is not a directive (`ast.IsDirective` wants `word:word`), so the
/// old-style constraint *is* header text upstream, and reports as such.
fn header_group(file: &File) -> Option<&CommentGroup> {
    file.comments
        .first()
        .filter(|cg| cg.pos().0 < file.package.0)
}

fn starts_block(cg: &CommentGroup) -> bool {
    cg.list
        .first()
        .is_some_and(|c| c.text.starts_with("/*"))
}

/// Header text exactly as upstream derives it: the whole group for `//` style,
/// but only the **first** comment for `/* */` style.
fn extract_header(cg: Option<&CommentGroup>) -> String {
    let Some(cg) = cg else {
        return String::new();
    };
    if starts_block(cg) {
        if let Some(first) = cg.list.first() {
            let single = CommentGroup {
                list: vec![Comment {
                    slash: first.slash,
                    text: first.text.clone(),
                }],
            };
            return single.text();
        }
    }
    cg.text()
}

fn reparse(path: &std::path::Path) -> Option<(Arc<FileSet>, File)> {
    let src = fs::read(path).ok()?;
    let name = path.file_name()?.to_str()?;
    let fset = FileSet::new();
    let file = parse_file(&fset, name, &src, PARSE_COMMENTS).ok()?;
    Some((fset, file))
}

// ---------------------------------------------------------------------------
// Analyzer
// ---------------------------------------------------------------------------

/// Upstream `GetTemplate`: an inline template is used **verbatim** — leading
/// and trailing whitespace are significant, and a trailing newline shows up as
/// `Missed string:`. Only a template read from `template-path` is trimmed.
fn resolve_template(opts: &GoheaderOptions) -> Result<String, String> {
    if !opts.template.is_empty() {
        return Ok(opts.template.clone());
    }
    if opts.template_path.is_empty() {
        return Ok(String::new());
    }
    // DEFERRED: ${config-path} / ${base-path} substitution.
    let raw = fs::read_to_string(&opts.template_path)
        .map_err(|e| format!("goheader template-path: {e}"))?;
    Ok(raw.trim().to_string())
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "goheader requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<GoheaderOptions>("goheader")
        .cloned()
        .unwrap_or_default();

    let template = resolve_template(&opts)?;
    if template.is_empty() {
        return Ok(None);
    }
    let base_values = build_values(&opts);

    let mut pending: Vec<(u32, String)> = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let n = pass.files().len();

    for i in 0..n {
        let Some(path) = paths.get(i) else {
            continue;
        };
        if path.extension().and_then(|s| s.to_str()) != Some("go") {
            continue;
        }
        let Some((_re_fset, parsed)) = reparse(path) else {
            continue;
        };

        let cg = header_group(&parsed);
        let is_block = cg.is_some_and(starts_block);
        // `offset.Position` seeded into the header reader, and the amount the
        // golangci wrapper subtracts back off. They do not cancel: the residue
        // is what puts the caret two columns left of the real divergence.
        let (reader_offset, wrapper_offset) = if cg.is_none() || is_block {
            (1i64, 0i64)
        } else {
            (4i64, 1i64)
        };

        let header = extract_header(cg);
        let header = header.trim();

        // A value that references an undefined name is a config error, but
        // upstream surfaces it as a per-file finding carrying the error text
        // (`&issue{msg: err.Error()}`), not as an analyzer failure — so it is
        // subject to the same `< 0` drop as any other location-less issue.
        let issue = match per_target_values(&base_values, path) {
            Ok(values) => Matcher::new(&values).analyze(header, &template, reader_offset)?,
            Err(e) => Some(Issue::bare(e)),
        };
        let Some(issue) = issue else {
            continue;
        };

        // Upstream's build-directive filter, and the sole reason a `//go:build`
        // file reports nothing: a location-less issue lands at column 0 and
        // 0 - 1 < 0.
        let col = issue.loc.col - wrapper_offset;
        if col < 0 {
            continue;
        }

        let Some(ft) = pass.fset().file(pass.files()[i].pos()) else {
            continue;
        };
        let line = issue.loc.line + 1;
        if line < 1 || line as usize > ft.line_count() {
            // Upstream `token.File.LineStart` would panic here.
            continue;
        }
        let offset = ft.line_start(line as usize).0 + col;
        // The mixed coordinate spaces can walk the offset past the end of the
        // file; upstream produces a bogus position, we clamp to stay in-bounds.
        let offset = offset.clamp(ft.base(), ft.end().0);
        pending.push((offset as u32, issue.message));
    }

    for (pos, msg) in pending {
        pass.reportf(pos, &msg);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "goheader",
        doc: "Check if file header matches to pattern",
        url: "https://github.com/denis-tingaikin/go-header",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `//`-style header offset, the common case.
    const SLASH: i64 = 4;
    /// `/* */`-style header offset.
    const BLOCK: i64 = 1;

    fn values(pairs: &[(&str, ValueKind, &str)]) -> HashMap<String, Value> {
        let mut m = built_in_values();
        for (k, kind, raw) in pairs {
            m.insert(k.to_ascii_lowercase(), Value::new(*kind, *raw));
        }
        calculate_all(&mut m).expect("calculate");
        m
    }

    fn check(header: &str, template: &str, offset: i64, vals: &HashMap<String, Value>) -> Option<(String, i64, i64)> {
        Matcher::new(vals)
            .analyze(header, template, offset)
            .expect("analyze")
            .map(|i| (i.message, i.loc.line, i.loc.col))
    }

    const TMPL: &str = "Copyright {{ YEAR }} Example Corp\nAll rights reserved.";

    fn year2020() -> HashMap<String, Value> {
        values(&[("YEAR", ValueKind::Const, "2020")])
    }

    // -- the four divergence shapes, all verified against golangci-lint 2.12.2 --

    #[test]
    fn exact_match_reports_nothing() {
        let v = year2020();
        assert!(check("Copyright 2020 Example Corp\nAll rights reserved.", TMPL, SLASH, &v).is_none());
    }

    /// golangci-lint: `b.go:1:19: Actual: Nobody Inc.\nExpected:Example Corp`
    #[test]
    fn mismatch_reports_actual_and_expected_at_divergence() {
        let v = year2020();
        let (msg, line, col) =
            check("Copyright 2020 Nobody Inc.\nAll rights reserved.", TMPL, SLASH, &v).unwrap();
        assert_eq!(msg, "Actual: Nobody Inc.\nExpected:Example Corp");
        assert_eq!((line, col), (0, 19));
    }

    /// golangci-lint: `c.go:2:4:` — the offset applies to every line, not just
    /// the first.
    #[test]
    fn mismatch_on_second_line() {
        let v = year2020();
        let (msg, line, col) =
            check("Copyright 2020 Example Corp\nSome rights reserved.", TMPL, SLASH, &v).unwrap();
        assert_eq!(msg, "Actual: Some rights reserved.\nExpected:All rights reserved.");
        assert_eq!((line, col), (1, 4));
    }

    /// golangci-lint: `d.go:2:24: Unexpected string: \nExtra line here.`
    #[test]
    fn header_longer_than_template() {
        let v = year2020();
        let (msg, line, col) = check(
            "Copyright 2020 Example Corp\nAll rights reserved.\nExtra line here.",
            TMPL,
            SLASH,
            &v,
        )
        .unwrap();
        assert_eq!(msg, "Unexpected string: \nExtra line here.");
        assert_eq!((line, col), (1, 24));
    }

    /// golangci-lint: `e.go:1:26: Missed string:  Corp\nAll rights reserved.`
    #[test]
    fn template_longer_than_header() {
        let v = year2020();
        let (msg, line, col) = check("Copyright 2020 Example", TMPL, SLASH, &v).unwrap();
        assert_eq!(msg, "Missed string:  Corp\nAll rights reserved.");
        assert_eq!((line, col), (0, 26));
    }

    #[test]
    fn blank_header_is_missed_header_at_origin() {
        let v = year2020();
        let (msg, line, col) = check("", TMPL, SLASH, &v).unwrap();
        assert_eq!(msg, "Missed header for check");
        // Location{0,0}; the wrapper's `0 - 1 < 0` then drops it for `//`.
        assert_eq!((line, col), (0, 0));
    }

    // -- values --

    /// golangci-lint: `v1.go:1:14: Expected:2020, Actual: 2019 Example Corp`.
    /// Note the asymmetric spacing, and that `Actual` runs to end of line from
    /// the *start* of the value, not from the diverging byte.
    #[test]
    fn const_value_mismatch() {
        let v = year2020();
        let (msg, line, col) =
            check("Copyright 2019 Example Corp\nAll rights reserved.", TMPL, SLASH, &v).unwrap();
        assert_eq!(msg, "Expected:2020, Actual: 2019 Example Corp");
        assert_eq!((line, col), (0, 14));
    }

    /// A regexp value is not anchored: it finds its first match anywhere in the
    /// remainder and resumes at the end of that match.
    #[test]
    fn regexp_value_is_unanchored_and_skips_to_match_end() {
        let v = values(&[("RE", ValueKind::Regexp, "[0-9]+ Example")]);
        assert!(check("Copyright 2020 Example Corp", "Copyright {{ RE }} Corp", SLASH, &v).is_none());
    }

    /// golangci-lint: `Pattern ZZZ[0-9]+ doesn't match.` at the value's start.
    #[test]
    fn regexp_value_mismatch() {
        let v = values(&[("RE", ValueKind::Regexp, "ZZZ[0-9]+")]);
        let (msg, line, col) =
            check("Copyright 2020 Example Corp", "Copyright {{ RE }} Corp", SLASH, &v).unwrap();
        assert_eq!(msg, "Pattern ZZZ[0-9]+ doesn't match.");
        assert_eq!((line, col), (0, 14));
    }

    /// `{{ .YEAR }}` is *not* the same as `{{ YEAR }}`: upstream never strips
    /// the dot, so the dotted spelling names a value that does not exist.
    #[test]
    fn dotted_placeholder_is_an_unknown_value() {
        let v = year2020();
        let (msg, ..) = check(
            "Copyright 2020 Example Corp",
            "Copyright {{ .YEAR }} Example Corp",
            SLASH,
            &v,
        )
        .unwrap();
        assert_eq!(msg, "Template has unknown value: .year");
    }

    /// The built-in range value is `year-range`; `YEAR_RANGE` does not exist.
    #[test]
    fn year_range_builtin_is_hyphenated() {
        let v = year2020();
        let (msg, ..) = check("Copyright 2020", "Copyright {{ YEAR_RANGE }}", SLASH, &v).unwrap();
        assert_eq!(msg, "Template has unknown value: year_range");

        // `year-range` nests `{{YEAR}}`, so overriding `year` propagates: with
        // year=2020 it accepts both "2020" and a "20xx-2020" range.
        assert!(check("Copyright 2020", "Copyright {{ year-range }}", SLASH, &v).is_none());
        assert!(check("Copyright 2019-2020", "Copyright {{ year-range }}", SLASH, &v).is_none());
    }

    /// Value names keep interior spaces; they are only lower-cased and trimmed.
    #[test]
    fn value_name_with_space() {
        let v = values(&[("SOME VALUE", ValueKind::Const, "2020")]);
        assert!(check(
            "Copyright 2020 Example Corp",
            "Copyright {{ SOME VALUE }} Example Corp",
            SLASH,
            &v
        )
        .is_none());
    }

    #[test]
    fn nested_values_resolve() {
        let v = values(&[
            ("INNER", ValueKind::Const, "Example"),
            ("OUTER", ValueKind::Const, "{{ INNER }} Corp"),
        ]);
        assert!(check("Copyright 2020 Example Corp", "Copyright 2020 {{ OUTER }}", SLASH, &v).is_none());
    }

    #[test]
    fn recursive_value_is_an_error_not_a_stack_overflow() {
        let mut m = built_in_values();
        m.insert("a".into(), Value::new(ValueKind::Const, "{{ b }}"));
        m.insert("b".into(), Value::new(ValueKind::Const, "{{ a }}"));
        assert!(calculate_all(&mut m).is_err());
    }

    // -- block-comment offset --

    /// `/* */` headers get offset 1 rather than 4, which is why upstream's
    /// caret lands two columns left of the real divergence.
    #[test]
    fn block_comment_offset() {
        let v = year2020();
        let (_, line, col) =
            check("Copyright 2020 Nobody Corp\nAll rights reserved.", TMPL, BLOCK, &v).unwrap();
        assert_eq!((line, col), (0, 16));
    }

    // -- template handling --

    /// An inline template is used verbatim: a trailing newline is a real
    /// mismatch, reported as `Missed string:`.
    #[test]
    fn inline_template_is_not_trimmed() {
        let mut opts = GoheaderOptions::default();
        opts.template = "Copyright {{ YEAR }} Example Corp\n".into();
        let tmpl = resolve_template(&opts).unwrap();
        assert_eq!(tmpl, "Copyright {{ YEAR }} Example Corp\n");

        let v = year2020();
        let (msg, ..) = check("Copyright 2020 Example Corp", &tmpl, SLASH, &v).unwrap();
        assert_eq!(msg, "Missed string: \n");
    }

    #[test]
    fn empty_template_disables_the_analyzer() {
        let opts = GoheaderOptions::default();
        assert_eq!(resolve_template(&opts).unwrap(), "");
    }

    // -- reader --

    #[test]
    fn reader_tracks_line_and_column() {
        let mut r = Reader::new(b"ab\ncd");
        assert_eq!(r.location(), Loc { line: 0, col: 0 });
        r.next();
        r.next();
        assert_eq!(r.location(), Loc { line: 0, col: 2 });
        r.next(); // '\n'
        assert_eq!(r.location(), Loc { line: 1, col: 0 });
        r.next();
        assert_eq!(r.location(), Loc { line: 1, col: 1 });
    }

    #[test]
    fn set_position_recomputes_location() {
        let mut r = Reader::new(b"ab\ncdef");
        r.set_position(6);
        assert_eq!(r.location(), Loc { line: 1, col: 3 });
        r.set_position(1);
        assert_eq!(r.location(), Loc { line: 0, col: 1 });
    }

    #[test]
    fn finish_returns_remainder_and_exhausts() {
        let mut r = Reader::new(b"abc");
        r.next();
        assert_eq!(r.finish(), b"bc");
        assert!(r.done());
        assert_eq!(r.finish(), b"");
    }
}
