//! Minimal `go.mod` finder + parser for gomoddirectives / gomodguard.
//!
//! Enough to cover the default checks we port; not a full `golang.org/x/mod`
//! substitute. DEFERRED: full directive coverage (ignore, godebug blocks, …).

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct GoMod {
    #[allow(dead_code)]
    pub path: PathBuf,
    pub module: Option<String>,
    pub requires: Vec<String>,
    pub replaces: Vec<Replace>,
    pub retracts: Vec<Retract>,
    pub excludes: Vec<String>,
    pub tools: Vec<String>,
    pub toolchain: Option<String>,
    pub godebugs: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Replace {
    pub old_path: String,
    pub old_version: String,
    pub new_path: String,
    /// Empty version means a local filesystem replace.
    pub new_version: String,
    #[allow(dead_code)]
    pub line: u32,
}

#[derive(Debug, Clone)]
pub struct Retract {
    pub rationale: String,
    #[allow(dead_code)]
    pub line: u32,
}

/// Walk `start` and parents looking for `go.mod`.
pub fn find_gomod(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        let candidate = dir.join("go.mod");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

pub fn parse_gomod(path: &Path) -> Option<GoMod> {
    let text = fs::read_to_string(path).ok()?;
    Some(parse_gomod_str(path, &text))
}

pub fn parse_gomod_str(path: &Path, text: &str) -> GoMod {
    let mut out = GoMod {
        path: path.to_path_buf(),
        ..GoMod::default()
    };

    let mut lines = text.lines().enumerate().peekable();
    while let Some((idx, raw)) = lines.next() {
        let line_no = (idx + 1) as u32;
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("module ") {
            out.module = Some(rest.trim().to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("toolchain ") {
            out.toolchain = Some(rest.trim().to_string());
            continue;
        }
        if line.starts_with("go ") {
            continue;
        }

        if let Some(block) = parse_block_header(line, "require") {
            consume_block(&mut lines, block, line_no, |toks, ln| {
                if let Some(path) = toks.first() {
                    out.requires.push((*path).to_string());
                }
                let _ = ln;
            });
            continue;
        }
        if let Some(block) = parse_block_header(line, "replace") {
            consume_block(&mut lines, block, line_no, |toks, ln| {
                if let Some(r) = parse_replace_tokens(toks, ln) {
                    out.replaces.push(r);
                }
            });
            continue;
        }
        if let Some(block) = parse_block_header(line, "retract") {
            match block {
                BlockStart::Inline(_) => {
                    out.retracts.push(Retract {
                        rationale: extract_retract_rationale(raw).unwrap_or_default(),
                        line: line_no,
                    });
                }
                BlockStart::Open => {
                    while let Some((idx, raw_inner)) = lines.next() {
                        let ln = (idx + 1) as u32;
                        let inner = strip_comment(raw_inner).trim();
                        if inner.is_empty() {
                            continue;
                        }
                        if inner == ")" {
                            break;
                        }
                        out.retracts.push(Retract {
                            rationale: extract_retract_rationale(raw_inner).unwrap_or_default(),
                            line: ln,
                        });
                    }
                }
            }
            continue;
        }
        if let Some(block) = parse_block_header(line, "exclude") {
            consume_block(&mut lines, block, line_no, |toks, ln| {
                if let Some(p) = toks.first() {
                    out.excludes.push((*p).to_string());
                }
                let _ = ln;
            });
            continue;
        }
        if let Some(block) = parse_block_header(line, "tool") {
            consume_block(&mut lines, block, line_no, |toks, ln| {
                if let Some(p) = toks.first() {
                    out.tools.push((*p).to_string());
                }
                let _ = ln;
            });
            continue;
        }
        if let Some(block) = parse_block_header(line, "godebug") {
            consume_block(&mut lines, block, line_no, |toks, ln| {
                if let Some(p) = toks.first() {
                    out.godebugs.push((*p).to_string());
                }
                let _ = ln;
            });
            continue;
        }

        // Single-line forms already handled when block==Some(tokens).
        let _ = (line_no, line);
    }

    out
}

fn strip_comment(line: &str) -> &str {
    // Keep `//` inside module paths uncommon; treat first ` //` as comment.
    if let Some(idx) = line.find("//") {
        // But for retract rationale we need the comment — callers use raw.
        &line[..idx]
    } else {
        line
    }
}

fn extract_retract_rationale(raw: &str) -> Option<String> {
    let idx = raw.find("//")?;
    let r = raw[idx + 2..].trim();
    if r.is_empty() {
        None
    } else {
        Some(r.to_string())
    }
}

/// Returns `Some(inline_tokens)` for `directive ( ... )` or single-line body,
/// or `Some(empty)` for `directive (` opener.
enum BlockStart<'a> {
    Inline(Vec<&'a str>),
    Open,
}

fn parse_block_header<'a>(line: &'a str, directive: &str) -> Option<BlockStart<'a>> {
    let rest = line.strip_prefix(directive)?.trim_start();
    if rest.is_empty() {
        return None;
    }
    if rest == "(" {
        return Some(BlockStart::Open);
    }
    if let Some(inner) = rest.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        return Some(BlockStart::Inline(tokenize(inner.trim())));
    }
    // Single-line: `replace a => b`
    Some(BlockStart::Inline(tokenize(rest)))
}

fn consume_block<'a, I, F>(
    lines: &mut std::iter::Peekable<I>,
    start: BlockStart<'a>,
    inline_line: u32,
    mut f: F,
) where
    I: Iterator<Item = (usize, &'a str)>,
    F: FnMut(&[&str], u32),
{
    match start {
        BlockStart::Inline(toks) => {
            if !toks.is_empty() {
                f(&toks, inline_line);
            }
        }
        BlockStart::Open => {
            while let Some((idx, raw)) = lines.next() {
                let line_no = (idx + 1) as u32;
                let line = strip_comment(raw).trim();
                if line.is_empty() {
                    continue;
                }
                if line == ")" {
                    break;
                }
                let toks = tokenize(line);
                if !toks.is_empty() {
                    f(&toks, line_no);
                }
            }
        }
    }
}

fn tokenize(s: &str) -> Vec<&str> {
    s.split_whitespace().collect()
}

fn parse_replace_tokens(toks: &[&str], line: u32) -> Option<Replace> {
    // Forms:
    //   old [v] => new [v]
    let arrow = toks.iter().position(|t| *t == "=>")?;
    let old = &toks[..arrow];
    let new = &toks[arrow + 1..];
    if old.is_empty() || new.is_empty() {
        return None;
    }
    let (old_path, old_version) = split_path_ver(old);
    let (new_path, new_version) = split_path_ver(new);
    Some(Replace {
        old_path,
        old_version,
        new_path,
        new_version,
        line,
    })
}

fn split_path_ver(toks: &[&str]) -> (String, String) {
    if toks.len() >= 2 && toks[1].starts_with('v') {
        (toks[0].to_string(), toks[1].to_string())
    } else {
        (toks[0].to_string(), String::new())
    }
}

impl Replace {
    pub fn is_local(&self) -> bool {
        self.new_version.trim().is_empty()
    }
}

/// Whether `pkg` belongs to module `mod` (gomodguard `isPackageInModule`).
pub fn is_package_in_module(pkg: &str, module: &str) -> bool {
    let pkg_parts: Vec<&str> = pkg.split('/').collect();
    let mod_parts: Vec<&str> = module.split('/').collect();
    if pkg_parts.len() < mod_parts.len() {
        return false;
    }
    if pkg_parts[..mod_parts.len()] != mod_parts[..] {
        return false;
    }
    if pkg_parts.len() > mod_parts.len() {
        let next = pkg_parts[mod_parts.len()];
        if next.len() >= 2 && next.starts_with('v') && next[1..].chars().all(|c| c.is_ascii_digit())
        {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn parses_replace_and_retract() {
        let text = r#"
module example.com/m

go 1.22

require github.com/foo/bar v1.2.3

replace github.com/foo/bar => ../bar

retract v1.0.0 // bad release
"#;
        let g = parse_gomod_str(Path::new("go.mod"), text);
        assert_eq!(g.module.as_deref(), Some("example.com/m"));
        assert_eq!(g.requires, vec!["github.com/foo/bar"]);
        assert_eq!(g.replaces.len(), 1);
        assert!(g.replaces[0].is_local());
        assert_eq!(g.retracts.len(), 1);
        assert_eq!(g.retracts[0].rationale, "bad release");
    }

    #[test]
    fn package_in_module() {
        assert!(is_package_in_module(
            "github.com/foo/bar/baz",
            "github.com/foo/bar"
        ));
        assert!(!is_package_in_module(
            "github.com/foo/bar/v2/baz",
            "github.com/foo/bar"
        ));
    }
}
