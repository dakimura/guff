//! `buildtag` — check `//go:build` and `// +build` directives.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/buildtag` (v0.44.0, the
//! version golangci-lint 2.12.2 pins).
//!
//! Two structural notes:
//!
//! * The analysis AST does not carry comments that follow the `package`
//!   clause: without `PARSE_COMMENTS` the parser drops every comment once it
//!   has left the file header (`parser.rs`, `next0`). Every "misplaced"
//!   diagnostic is by definition about such a comment, so this pass re-parses
//!   each file with comments and maps positions back with
//!   [`code::remap_reparsed_pos`] — the same treatment gocritic's comment
//!   checkers get.
//! * Constraint parsing goes through `guff::constraint`, the port of
//!   `go/build/constraint`. The previous hand-rolled approximation accepted
//!   `// +buildlinux` as a `+build` line (so it never reported the malformed
//!   comment) and treated `// go:build` as a real `//go:build` line.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::File as AstFile;
use guff::constraint::{self, Expr};
use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::{FileSet, Pos};
use guff_analysis::code;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// Upstream `checker`. Positions stay in whatever `FileSet` the caller fed the
/// text from; `run` maps them into the pass's `FileSet` afterwards.
struct Checker {
    /// "+build" lines still OK.
    plus_build_ok: bool,
    /// "go:build" lines still OK.
    go_build_ok: bool,
    /// Cross-check go:build and +build lines when done reading the file.
    cross_check: bool,
    /// Currently inside a `/* */` comment.
    in_star: bool,
    go_build_pos: Option<u32>,
    plus_build_pos: Option<u32>,
    go_build: Option<Expr>,
    /// AND of the +build constraints found.
    plus_build: Option<Expr>,
    pending: Vec<(u32, String)>,
}

impl Checker {
    fn new() -> Self {
        Self {
            plus_build_ok: true,
            go_build_ok: true,
            cross_check: true,
            in_star: false,
            go_build_pos: None,
            plus_build_pos: None,
            go_build: None,
            plus_build: None,
            pending: Vec::new(),
        }
    }

    fn report(&mut self, pos: u32, message: impl Into<String>) {
        self.pending.push((pos, message.into()));
    }

    fn check_go_file(&mut self, f: &AstFile) {
        for group in &f.comments {
            // A +build comment is ignored after or adjoining the package
            // declaration.
            if group.end().0 + 1 >= f.package.0 {
                self.plus_build_ok = false;
            }
            // A //go:build comment is ignored after the package declaration
            // (but adjoining it is OK, in contrast to +build comments).
            if group.pos().0 >= f.package.0 {
                self.go_build_ok = false;
            }

            for c in &group.list {
                // "+build" is ignored within or after a /*...*/ comment.
                if !c.text.starts_with("//") {
                    self.plus_build_ok = false;
                }
                self.comment(c.slash.0 as u32, &c.text);
            }
        }
    }

    /// Upstream `checker.file`: scan a non-Go file's raw text. Cannot use the
    /// Go parser, so it walks lines and tracks `/* */` nesting itself.
    fn check_text(&mut self, base: u32, full_text: &str) {
        // Determine the cutpoint where +build comments are no longer valid:
        // they are valid in leading // comments followed by a blank line.
        let mut plus_build_cutoff = 0usize;
        let mut text = full_text;
        while !text.is_empty() {
            let i = match text.find('\n') {
                Some(i) => i + 1,
                None => text.len(),
            };
            let offset = full_text.len() - text.len();
            let line = &text[..i];
            text = &text[i..];
            let line = line.trim();
            if !line.starts_with("//") && !line.is_empty() {
                break;
            }
            if line.is_empty() {
                plus_build_cutoff = offset;
            }
        }

        // Process each line. Must stop once go_build_ok is false.
        let mut text = full_text;
        self.in_star = false;
        while !text.is_empty() {
            let i = match text.find('\n') {
                Some(i) => i + 1,
                None => text.len(),
            };
            let offset = full_text.len() - text.len();
            let raw = &text[..i];
            text = &text[i..];
            self.plus_build_ok = offset < plus_build_cutoff;

            if raw.starts_with("//") {
                self.comment(base + offset as u32, raw);
                continue;
            }

            // Keep looking for the point at which //go:build comments stop
            // being allowed. Skip over / cut out any /* */ comments.
            let mut line = raw;
            loop {
                line = line.trim();
                if self.in_star {
                    match line.find("*/") {
                        Some(i) => {
                            line = &line[i + 2..];
                            self.in_star = false;
                            continue;
                        }
                        None => {
                            line = "";
                            break;
                        }
                    }
                }
                if let Some(rest) = line.strip_prefix("/*") {
                    self.in_star = true;
                    line = rest;
                    continue;
                }
                break;
            }
            if !line.is_empty() {
                // Found a non-comment non-blank line: this ends the space for
                // valid //go:build comments, and also ends the fraction of the
                // file we can reliably parse. Stop.
                break;
            }
        }
    }

    fn comment(&mut self, pos: u32, text: &str) {
        if text.starts_with("//") {
            if text.contains("+build") {
                self.plus_build_line(pos, text);
            }
            // Note the exact substring: `// go:build` (with a space) does not
            // reach `go_build_line` at all, which is why upstream's "space
            // between // and go:build" message is unreachable from Go source.
            if text.contains("//go:build") {
                self.go_build_line(pos, text);
            }
        }
        if text.starts_with("/*") {
            if let Some(i) = text.find('\n') {
                // Multiline /* */ comment: process the interior lines.
                self.in_star = true;
                let mut pos = pos + i as u32 + 1;
                let mut rest = &text[i + 1..];
                while !rest.is_empty() {
                    let j = match rest.find('\n') {
                        Some(j) => j + 1,
                        None => rest.len(),
                    };
                    let line = &rest[..j];
                    if line.starts_with("//") {
                        self.comment(pos, line);
                    }
                    pos += j as u32;
                    rest = &rest[j..];
                }
                self.in_star = false;
            }
        }
    }

    fn go_build_line(&mut self, pos: u32, line: &str) {
        if !constraint::is_go_build(line) {
            if !line.starts_with("//go:build") {
                if let Some(rest) = line.get(2..) {
                    if constraint::is_go_build(&format!("//{}", rest.trim())) {
                        self.report(
                            pos,
                            "malformed //go:build line (space between // and go:build)",
                        );
                    }
                }
            }
            return;
        }
        if !self.go_build_ok || self.in_star {
            self.report(pos, "misplaced //go:build comment");
            self.cross_check = false;
            return;
        }

        if self.go_build_pos.is_none() {
            self.go_build_pos = Some(pos);
        } else {
            self.report(pos, "unexpected extra //go:build line");
            self.cross_check = false;
        }

        // testing hack: stop at // ERROR
        let line = match line.find(" // ERROR ") {
            Some(i) => &line[..i],
            None => line,
        };

        match constraint::parse(line) {
            Err(e) => {
                self.report(pos, e.to_string());
                self.cross_check = false;
            }
            Ok(x) => {
                self.tags(pos, &x);
                if self.go_build.is_none() {
                    self.go_build = Some(x);
                }
            }
        }
    }

    fn plus_build_line(&mut self, pos: u32, line: &str) {
        let line = line.trim();
        if !constraint::is_plus_build(line) {
            // Comment with +build but not at the beginning. Only report early
            // in the file.
            if self.plus_build_ok && !line.starts_with("// want") {
                self.report(pos, "possible malformed +build comment");
            }
            return;
        }
        if !self.plus_build_ok {
            // in_star implies !plus_build_ok
            self.report(pos, "misplaced +build comment");
            self.cross_check = false;
        }

        if self.plus_build_pos.is_none() {
            self.plus_build_pos = Some(pos);
        }

        // testing hack: stop at // ERROR
        let line = match line.find(" // ERROR ") {
            Some(i) => &line[..i],
            None => line,
        };

        let fields: Vec<&str> = line[2..].split_whitespace().collect();
        // is_plus_build above implies fields[0] == "+build"
        for arg in fields.iter().skip(1) {
            for elem in arg.split(',') {
                if elem.starts_with("!!") {
                    self.report(
                        pos,
                        format!("invalid double negative in build constraint: {arg}"),
                    );
                    self.cross_check = false;
                    continue;
                }
                let elem = elem.strip_prefix('!').unwrap_or(elem);
                for c in elem.chars() {
                    if !c.is_alphabetic() && !c.is_numeric() && c != '_' && c != '.' {
                        self.report(
                            pos,
                            format!("invalid non-alphanumeric build constraint: {arg}"),
                        );
                        self.cross_check = false;
                        break;
                    }
                }
            }
        }

        if self.cross_check {
            match constraint::parse(line) {
                Err(e) => {
                    // Should never happen: constraint::parse never rejects a
                    // +build line, and the syntax was just checked above.
                    self.report(pos, e.to_string());
                    self.cross_check = false;
                }
                Ok(y) => {
                    self.tags(pos, &y);
                    self.plus_build = Some(match self.plus_build.take() {
                        None => y,
                        Some(prev) => Expr::and(prev, y),
                    });
                }
            }
        }
    }

    /// Report issues in go versions in tags within the expression.
    fn tags(&mut self, pos: u32, e: &Expr) {
        let mut bad = Vec::new();
        // `eval` does not short-circuit, so every tag is visited.
        let _ = e.eval(&mut |tag: &str| {
            if malformed_go_tag(tag) {
                bad.push(tag.to_string());
            }
            false
        });
        for tag in bad {
            self.report(pos, format!("invalid go version {tag:?} in build constraint"));
        }
    }

    fn finish(&mut self) {
        if !self.cross_check {
            return;
        }
        let (Some(go_pos), Some(plus_pos)) = (self.go_build_pos, self.plus_build_pos) else {
            return;
        };
        let (Some(go_build), Some(plus_build)) = (self.go_build.clone(), self.plus_build.clone())
        else {
            return;
        };

        // Have both //go:build and // +build with no errors found: check that
        // they mean the same thing.
        let lines = match constraint::plus_build_lines(&go_build) {
            Ok(lines) => lines,
            Err(e) => {
                self.report(go_pos, e.to_string());
                return;
            }
        };
        let mut want: Option<Expr> = None;
        for line in lines {
            let Ok(y) = constraint::parse(&line) else {
                // Definitely should not happen, and not the user's fault.
                return;
            };
            want = Some(match want {
                None => y,
                Some(prev) => Expr::and(prev, y),
            });
        }
        let Some(want) = want else { return };
        if want.to_string() != plus_build.to_string() {
            self.report(plus_pos, "+build lines do not match //go:build condition");
        }
    }
}

/// Upstream `malformedGoTag`: a tag likely meant to be a go version but isn't.
fn malformed_go_tag(tag: &str) -> bool {
    if !tag.starts_with("go1") {
        // Check for close misspellings of the "go1." prefix.
        for pre in ["go.", "g1.", "go"] {
            if let Some(suffix) = tag.strip_prefix(pre) {
                if valid_go_version(&format!("go1.{suffix}")) {
                    return true;
                }
            }
        }
        return false;
    }
    // The tag starts with "go1", so it is almost certainly a GoVersion.
    !valid_go_version(tag)
}

fn valid_go_version(tag: &str) -> bool {
    constraint::go_version(&Expr::tag(tag)).is_some()
}

/// Only files that mention a constraint can produce a diagnostic (every report
/// path is gated on `+build` or `//go:build` appearing in the text), so the
/// re-parse is skipped for the overwhelming majority of files.
fn may_have_constraints(src: &str) -> bool {
    src.contains("+build") || src.contains("go:build")
}

fn check_go_file(pass: &mut Pass<'_>, index: usize) {
    let path = pass
        .pkg()
        .compiled_go_files
        .get(index)
        .cloned()
        .or_else(|| pass.pkg().go_files.get(index).cloned());
    let Some(path) = path else { return };
    // Type-checking already read this file and kept the bytes (`source_files`,
    // parallel to `syntax`); opening it a second time here cost one `open` per
    // file of every package — 0.04s of analyze CPU on prometheus `./...` for a
    // gate that rejects almost every file. Fall back to a read only when the
    // bytes were not retained (`-j 1` trimming, export-data packages).
    let owned;
    let src: &[u8] = match pass.pkg().source_bytes(index) {
        Some(bytes) => bytes,
        None => {
            let Ok(read) = fs::read(&path) else { return };
            owned = read;
            &owned
        }
    };
    let Ok(text) = std::str::from_utf8(src) else {
        return;
    };
    if !may_have_constraints(text) {
        return;
    }
    let Some(name) = Path::new(&path).file_name().and_then(|s| s.to_str()) else {
        return;
    };

    let re_fset = FileSet::new();
    let Ok(parsed) = parse_file(&re_fset, name, src, COMMENTS_ONLY) else {
        return;
    };
    let mut check = Checker::new();
    check.check_go_file(&parsed);
    check.finish();
    if check.pending.is_empty() {
        return;
    }

    let file_pos = pass.files()[index].package;
    let fset = pass.fset();
    let mapped: Vec<(u32, String)> = check
        .pending
        .drain(..)
        .filter_map(|(pos, msg)| {
            code::remap_reparsed_pos(&fset, file_pos, &re_fset, Pos(pos as i64))
                .map(|p| (p.0 as u32, msg))
        })
        .collect();
    for (pos, msg) in mapped {
        pass.reportf(pos, msg);
    }
}

/// Upstream `checkOtherFile`: assembly and other non-Go sources carry build
/// constraints too, and are the only place `misplaced //go:build` can be
/// reported without the Go compiler rejecting the file first.
fn check_other_file(pass: &mut Pass<'_>, path: &str) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    if !may_have_constraints(&content) {
        return;
    }
    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path);

    let fset = pass.fset();
    let file = fset
        .files()
        .into_iter()
        .find(|f| f.name() == name)
        .unwrap_or_else(|| {
            let f = fset.add_file(name, -1, content.len() as i64);
            f.set_lines_for_content(content.as_bytes());
            f
        });

    let mut check = Checker::new();
    check.check_text(file.base() as u32, &content);
    check.finish();
    for (pos, msg) in std::mem::take(&mut check.pending) {
        pass.reportf(pos, msg);
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    for index in 0..pass.files().len() {
        check_go_file(pass, index);
    }
    let others: Vec<String> = pass.other_files().to_vec();
    for path in others {
        check_other_file(pass, &path);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "buildtag",
        doc: "check //go:build and // +build directives",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/buildtag",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
