//! Build-tag matching for source files.
//!
//! Port of `matchFile`, `shouldBuild`, `matchTag`, and `goodOSArchFile` from
//! `go/build/build.go`.

use std::collections::HashSet;

use guff::constraint::{self, Expr};

use crate::context::Context;

/// Error while evaluating build constraints in a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchError {
    /// More than one `//go:build` line in the file header.
    MultipleGoBuild,
    /// Failed to parse a build constraint expression.
    Parse(String),
}

impl std::fmt::Display for MatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MatchError::MultipleGoBuild => f.write_str("multiple //go:build comments"),
            MatchError::Parse(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for MatchError {}

impl Context {
    /// Reports whether the file `name` with contents `content` should be built.
    ///
    /// Equivalent to the build-tag portions of `build.Context.matchFile`.
    pub fn match_file(&self, name: &str, content: &[u8]) -> Result<bool, MatchError> {
        if name.starts_with('_') || name.starts_with('.') {
            return Ok(false);
        }

        let ext = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or("");
        if ext != "go" {
            return Ok(false);
        }

        if !self.good_os_arch_file(name, &mut None) && !self.use_all_files {
            return Ok(false);
        }

        let should_build = self.should_build(content, &mut None)?;
        Ok(should_build || self.use_all_files)
    }

    /// Reports whether build constraints in `content` are satisfied.
    ///
    /// Equivalent to `build.Context.shouldBuild`.
    pub fn should_build(
        &self,
        content: &[u8],
        all_tags: &mut Option<HashSet<String>>,
    ) -> Result<bool, MatchError> {
        let (header, go_build, _saw_binary_only) = parse_file_header(content)?;

        if let Some(line) = go_build {
            let text = std::str::from_utf8(line).map_err(|e| MatchError::Parse(e.to_string()))?;
            let expr = constraint::parse(text).map_err(|e| MatchError::Parse(e.to_string()))?;
            return Ok(self.eval(&expr, all_tags));
        }

        let header = std::str::from_utf8(header).map_err(|e| MatchError::Parse(e.to_string()))?;
        let mut should_build = true;
        for line in header.lines() {
            let line = line.trim_end();
            if !line.starts_with("//") || !line.contains("+build") {
                continue;
            }
            if !constraint::is_plus_build(line) {
                continue;
            }
            let expr = constraint::parse(line).map_err(|e| MatchError::Parse(e.to_string()))?;
            if !self.eval(&expr, all_tags) {
                should_build = false;
            }
        }
        Ok(should_build)
    }

    /// Reports whether `name` is one of the satisfied build tags.
    ///
    /// Equivalent to `build.Context.matchTag`.
    pub fn match_tag(&self, name: &str, all_tags: &mut Option<HashSet<String>>) -> bool {
        if let Some(tags) = all_tags {
            tags.insert(name.to_string());
        }

        if self.cgo_enabled && name == "cgo" {
            return true;
        }
        if name == self.goos || name == self.goarch || name == self.compiler {
            return true;
        }
        if self.goos == "android" && name == "linux" {
            return true;
        }
        if self.goos == "illumos" && name == "solaris" {
            return true;
        }
        if self.goos == "ios" && name == "darwin" {
            return true;
        }
        if name == "unix" && is_unix_os(&self.goos) {
            return true;
        }

        let tag = if name == "boringcrypto" {
            "goexperiment.boringcrypto"
        } else {
            name
        };

        self.build_tags.iter().any(|t| t == tag)
            || self.tool_tags.iter().any(|t| t == tag)
            || self.release_tags.iter().any(|t| t == tag)
    }

    /// Reports whether the file name's `_$GOOS` / `_$GOARCH` suffixes match.
    ///
    /// Equivalent to `build.Context.goodOSArchFile`.
    pub fn good_os_arch_file(&self, name: &str, all_tags: &mut Option<HashSet<String>>) -> bool {
        let stem = name.split('.').next().unwrap_or(name);

        // Ignore everything before the first `_` (see Go 1.4+ behavior).
        let Some(suffix) = stem.split_once('_').map(|(_, rest)| rest) else {
            return true;
        };

        let mut parts: Vec<&str> = suffix.split('_').collect();
        if parts.last() == Some(&"test") {
            parts.pop();
        }

        let n = parts.len();
        if n >= 2 && is_known_os(parts[n - 2]) && is_known_arch(parts[n - 1]) {
            if let Some(tags) = all_tags {
                tags.insert(parts[n - 2].to_string());
            }
            return self.match_tag(parts[n - 1], all_tags) && self.match_tag(parts[n - 2], all_tags);
        }
        if n >= 1 && (is_known_os(parts[n - 1]) || is_known_arch(parts[n - 1])) {
            return self.match_tag(parts[n - 1], all_tags);
        }
        true
    }

    fn eval(&self, expr: &Expr, all_tags: &mut Option<HashSet<String>>) -> bool {
        expr.eval(&mut |tag| self.match_tag(tag, all_tags))
    }
}

/// Parses the leading comment run of a Go source file.
///
/// Equivalent to `build.parseFileHeader`.
fn parse_file_header(content: &[u8]) -> Result<(&[u8], Option<&[u8]>, bool), MatchError> {
    let mut end = 0usize;
    let mut p = content;
    let mut ended = false;
    let mut in_slash_star = false;
    let mut go_build: Option<&[u8]> = None;
    let mut saw_binary_only = false;

    while !p.is_empty() {
        let (line, rest) = match p.iter().position(|&b| b == b'\n') {
            Some(i) => (&p[..i], &p[i + 1..]),
            None => (p, &[] as &[u8]),
        };
        p = rest;

        let trimmed = trim_bytes(line);
        if trimmed.is_empty() && !ended {
            end = content.len() - p.len();
            continue;
        }
        if !trimmed.starts_with(b"//") {
            ended = true;
        }

        if !in_slash_star && is_go_build_comment(trimmed) {
            if go_build.is_some() {
                return Err(MatchError::MultipleGoBuild);
            }
            go_build = Some(trimmed);
        }
        if !in_slash_star && trimmed == b"//go:binary-only-package" {
            saw_binary_only = true;
        }

        let mut line = trimmed;
        loop {
            if in_slash_star {
                if let Some(i) = find_subslice(line, b"*/") {
                    in_slash_star = false;
                    line = trim_bytes(&line[i + 2..]);
                    continue;
                }
                break;
            }
            if line.starts_with(b"//") {
                break;
            }
            if line.starts_with(b"/*") {
                in_slash_star = true;
                line = trim_bytes(&line[2..]);
                continue;
            }
            return Ok((&content[..end], go_build, saw_binary_only));
        }
    }

    Ok((&content[..end], go_build, saw_binary_only))
}

fn is_go_build_comment(line: &[u8]) -> bool {
    if !line.starts_with(b"//go:build") {
        return false;
    }
    let line = trim_bytes(line);
    let rest = &line[b"//go:build".len()..];
    rest.is_empty() || trim_bytes(rest).len() < rest.len()
}

fn trim_bytes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|b| !matches!(b, b' ' | b'\t' | b'\r')).unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t' | b'\r'))
        .map(|i| i + 1)
        .unwrap_or(start);
    &bytes[start..end]
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn is_known_os(name: &str) -> bool {
    matches!(
        name,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "js"
            | "linux"
            | "nacl"
            | "netbsd"
            | "openbsd"
            | "plan9"
            | "solaris"
            | "wasip1"
            | "windows"
            | "zos"
    )
}

fn is_known_arch(name: &str) -> bool {
    matches!(
        name,
        "386"
            | "amd64"
            | "amd64p32"
            | "arm"
            | "armbe"
            | "arm64"
            | "arm64be"
            | "loong64"
            | "mips"
            | "mipsle"
            | "mips64"
            | "mips64le"
            | "mips64p32"
            | "mips64p32le"
            | "ppc"
            | "ppc64"
            | "ppc64le"
            | "riscv"
            | "riscv64"
            | "s390"
            | "s390x"
            | "sparc"
            | "sparc64"
            | "wasm"
    )
}

fn is_unix_os(goos: &str) -> bool {
    matches!(
        goos,
        "aix"
            | "android"
            | "darwin"
            | "dragonfly"
            | "freebsd"
            | "hurd"
            | "illumos"
            | "ios"
            | "linux"
            | "netbsd"
            | "openbsd"
            | "solaris"
    )
}
