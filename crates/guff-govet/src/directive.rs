//! `directive` — check Go toolchain directives such as `//go:debug`.
//!
//! Port of `golang.org/x/tools/go/analysis/passes/directive` (v0.44.0).
//!
//! Like `buildtag`, every diagnostic this pass can produce is about a comment,
//! and "only valid before package declaration" is by definition about one that
//! follows the `package` clause — which the analysis AST does not carry (the
//! parser drops comments past the file header unless `PARSE_COMMENTS` is set).
//! So the file is re-parsed with comments and positions are mapped back.

use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use guff::ast::File as AstFile;
use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff_analysis::code;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

struct Checker {
    filename: String,
    /// The package name, or `None` for a non-Go file (upstream's `file == nil`).
    package_name: Option<String>,
    /// In the file header (before or adjoining the package declaration).
    in_header: bool,
    pending: Vec<(u32, String)>,
}

impl Checker {
    fn new(filename: String, package_name: Option<String>) -> Self {
        Self {
            filename,
            package_name,
            in_header: true,
            pending: Vec::new(),
        }
    }

    fn check_go_file(&mut self, f: &AstFile) {
        for group in &f.comments {
            // A //go:build or //go:debug comment is ignored after the package
            // declaration (but adjoining it is OK, unlike +build comments).
            if group.pos().0 >= f.package.0 {
                self.in_header = false;
            }
            for c in &group.list {
                self.comment(c.slash.0 as u32, &c.text);
            }
        }
    }

    /// Upstream `nonGoFile`: walk the raw text of an assembly or other non-Go
    /// source, skipping `/* */` so a commented-out `//` line is not mistaken
    /// for a directive.
    fn check_non_go_file(&mut self, base: u32, full_text: &str) {
        let mut text = full_text;
        let mut in_star = false;
        while !text.is_empty() {
            let offset = full_text.len() - text.len();
            let (raw, rest) = match text.find('\n') {
                Some(i) => (&text[..i], &text[i + 1..]),
                None => (text, ""),
            };
            text = rest;

            if !in_star && raw.starts_with("//") {
                self.comment(base + offset as u32, raw);
                continue;
            }

            let mut line = raw;
            loop {
                line = line.trim();
                if in_star {
                    match line.find("*/") {
                        Some(i) => {
                            line = &line[i + 2..];
                            in_star = false;
                            continue;
                        }
                        None => break,
                    }
                }
                match line.strip_prefix("/*") {
                    Some(after) => {
                        line = after;
                        in_star = true;
                    }
                    None => break,
                }
            }
            if !in_star && !line.is_empty() {
                // A non-comment non-blank line ends the part of the file we can
                // reliably read: this might not even be a Go program.
                break;
            }
        }
    }

    fn comment(&mut self, pos: u32, line: &str) {
        if !line.starts_with("//go:") {
            return;
        }
        // testing hack: stop at // ERROR
        let line = match line.find(" // ERROR ") {
            Some(i) => &line[..i],
            None => line,
        };

        let mut verb = line;
        if let Some((i, r)) = line.char_indices().find(|(_, r)| is_go_space(*r)) {
            verb = &line[..i];
            if r != ' ' && r != '\t' && r != '\n' {
                self.pending.push((
                    pos,
                    format!("invalid space {} in {verb} directive", quote_rune(r)),
                ));
            }
        }

        match verb {
            // Ignored: the buildtag analyzer reports misplaced comments.
            "//go:build" => {}
            "//go:debug" => {
                let Some(package_name) = self.package_name.clone() else {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid in Go source files".into(),
                    ));
                    return;
                };
                if package_name != "main" && !self.filename.ends_with("_test.go") {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid in package main or test".into(),
                    ));
                } else if !self.in_header {
                    self.pending.push((
                        pos,
                        "//go:debug directive only valid before package declaration".into(),
                    ));
                }
            }
            _ => {}
        }
    }
}

/// `unicode.IsSpace`.
fn is_go_space(r: char) -> bool {
    matches!(
        r,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{85}' | '\u{a0}'
    ) || matches!(
        r,
        '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// Go's `%#q` on a rune. Only non-printable whitespace ever reaches here, and
/// none of it can be backquoted, so `%#q` always falls back to the `%q` form
/// (verified against golangci-lint: `'\v'`, `' '`).
fn quote_rune(r: char) -> String {
    let escaped = match r {
        '\u{7}' => Some("\\a"),
        '\u{8}' => Some("\\b"),
        '\u{c}' => Some("\\f"),
        '\n' => Some("\\n"),
        '\r' => Some("\\r"),
        '\t' => Some("\\t"),
        '\u{b}' => Some("\\v"),
        '\\' => Some("\\\\"),
        '\'' => Some("\\'"),
        _ => None,
    };
    if let Some(e) = escaped {
        return format!("'{e}'");
    }
    let c = r as u32;
    if c < 0x80 {
        format!("'\\x{c:02x}'")
    } else if c < 0x10000 {
        format!("'\\u{c:04x}'")
    } else {
        format!("'\\U{c:08x}'")
    }
}

fn check_go_file(pass: &mut Pass<'_>, index: usize) {
    let path = pass
        .pkg()
        .compiled_go_files
        .get(index)
        .cloned()
        .or_else(|| pass.pkg().go_files.get(index).cloned());
    let Some(path) = path else { return };
    let filename = path.to_string_lossy().to_string();
    let Ok(src) = fs::read(&path) else { return };
    // Every report path is gated on a `//go:` prefix.
    if !src.windows(5).any(|w| w == b"//go:") {
        return;
    }
    let Some(name) = Path::new(&path).file_name().and_then(|s| s.to_str()) else {
        return;
    };

    let re_fset = FileSet::new();
    let Ok(parsed) = parse_file(&re_fset, name, &src, PARSE_COMMENTS) else {
        return;
    };
    let mut check = Checker::new(filename, Some(parsed.name.name.clone()));
    check.check_go_file(&parsed);
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

fn check_other_file(pass: &mut Pass<'_>, path: &str) {
    let Ok(content) = fs::read_to_string(path) else {
        return;
    };
    if !content.contains("//go:") {
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

    let mut check = Checker::new(path.to_string(), None);
    check.check_non_go_file(file.base() as u32, &content);
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
        name: "directive",
        doc: "check Go toolchain directives such as //go:debug",
        url: "https://pkg.go.dev/golang.org/x/tools/go/analysis/passes/directive",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![],
        fact_types: vec![],
    })
}
