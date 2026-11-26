//! `cmd/go/internal/load.resolveEmbed` — turning `//go:embed` patterns into a
//! file list, or into the error `go list -e` reports on the package.
//!
//! golangci-lint surfaces that error as its `typecheck` pseudo linter, so this
//! is the native lister's half of a finding both tools must agree on. The
//! measured shapes (`go list -e` and golangci-lint 2.12.2 on the same tree):
//!
//! ```text
//! //go:embed app/dist            → pattern app/dist: no matching files found
//! //go:embed have.txt nope.txt   → pattern nope.txt: … (column of the 2nd)
//! //go:embed "no such.txt"       → pattern no such.txt: …
//! //go:embed all:hidden          → pattern all:hidden: …
//! //go:embed ../ok               → pattern ../ok: invalid pattern syntax
//! //go:embed only (only/.hidden) → pattern only: cannot embed directory only:
//!                                  contains no embeddable files
//! ```
//!
//! Getting this wrong is not worth one wrong finding: a `typecheck` issue
//! deletes every other issue in the run, so an invented error empties the
//! report for the whole target. Where a shape is not certain, the code returns
//! *no* error rather than a guessed one.
//!
//! The tree those shapes live in is `crates/guff-golist/tests/testdata/embed`,
//! read by `tests/embed_shapes.rs`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// The failing pattern and the message `go list` puts after `pattern <p>: `.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedError {
    pub pattern: String,
    pub msg: String,
}

impl EmbedError {
    /// The full `Error.Err` string, which is `EmbedError.Error()`.
    pub fn text(&self) -> String {
        format!("pattern {}: {}", self.pattern, self.msg)
    }
}

fn err(pattern: &str, msg: impl Into<String>) -> EmbedError {
    EmbedError {
        pattern: pattern.to_string(),
        msg: msg.into(),
    }
}

/// Resolves `patterns` against `pkgdir`, returning the sorted unique relative
/// paths (`EmbedFiles`) or the first pattern that fails.
///
/// `patterns` must already be sorted — `go/build`'s `cleanDecls` sorts them and
/// the blamed pattern is the first *sorted* failure.
pub fn resolve_embed(pkgdir: &Path, patterns: &[String]) -> Result<Vec<String>, EmbedError> {
    let mut have: HashMap<String, usize> = HashMap::new();
    let mut dir_ok: HashSet<PathBuf> = HashSet::new();
    let mut pid = 0usize;

    for pattern in patterns {
        pid += 1;
        let (glob, all) = match pattern.strip_prefix("all:") {
            Some(rest) => (rest, true),
            None => (pattern.as_str(), false),
        };
        if !valid_embed_pattern(glob) || path_match(glob, "").is_err() {
            return Err(err(pattern, "invalid pattern syntax"));
        }

        let matches = glob_files(pkgdir, glob).map_err(|_| err(pattern, "invalid pattern syntax"))?;

        let mut list_len = 0usize;
        for file in matches {
            let rel = rel_slash(&file, pkgdir);
            let Ok(info) = std::fs::symlink_metadata(&file) else {
                // `fsys.Lstat` failing here is an I/O error whose text is the
                // OS message; not a shape we can match, so stay silent.
                return Ok(Vec::new());
            };
            let what = if info.is_dir() { "directory" } else { "file" };

            // Walk up to pkgdir: no nested module, no file acting as a
            // directory, no name that would not survive `go mod vendor`.
            let mut dir = file.clone();
            while dir.as_os_str().len() > pkgdir.as_os_str().len() + 1 && !dir_ok.contains(&dir) {
                if dir.join("go.mod").exists() {
                    return Err(err(
                        pattern,
                        format!("cannot embed {what} {rel}: in different module"),
                    ));
                }
                if dir != file {
                    if let Ok(i) = std::fs::symlink_metadata(&dir) {
                        if !i.is_dir() {
                            let suffix = rel_slash(&dir, pkgdir);
                            return Err(err(
                                pattern,
                                format!("cannot embed {what} {rel}: in non-directory {suffix}"),
                            ));
                        }
                    }
                }
                dir_ok.insert(dir.clone());
                let elem = base_name(&dir);
                if is_bad_embed_name(&elem) {
                    return Err(err(
                        pattern,
                        if dir == file {
                            format!("cannot embed {what} {rel}: invalid name {elem}")
                        } else {
                            format!("cannot embed {what} {rel}: in invalid directory {elem}")
                        },
                    ));
                }
                match dir.parent() {
                    Some(p) => dir = p.to_path_buf(),
                    None => break,
                }
            }

            if info.is_dir() {
                let mut added = 0usize;
                let count = walk_embed_dir(&file, pkgdir, all, true, &mut |rel_file| {
                    if have.get(&rel_file) != Some(&pid) {
                        have.insert(rel_file, pid);
                        added += 1;
                    }
                })
                .map_err(|m| err(pattern, m))?;
                list_len += added;
                if count == 0 {
                    return Err(err(
                        pattern,
                        format!("cannot embed directory {rel}: contains no embeddable files"),
                    ));
                }
            } else if info.is_file() {
                if have.get(&rel) != Some(&pid) {
                    have.insert(rel, pid);
                    list_len += 1;
                }
            } else {
                // Symlinks (the default `embedfollowsymlinks=0`) and devices.
                return Err(err(pattern, format!("cannot embed irregular file {rel}")));
            }
        }

        if list_len == 0 {
            return Err(err(pattern, "no matching files found"));
        }
    }

    let mut files: Vec<String> = have.into_keys().collect();
    files.sort();
    Ok(files)
}

/// `validEmbedPattern`: not `.`, and a valid slash path (`fs.ValidPath`).
fn valid_embed_pattern(pattern: &str) -> bool {
    pattern != "." && valid_fs_path(pattern)
}

/// `fs.ValidPath`: unrooted, slash-separated, no empty / `.` / `..` element.
fn valid_fs_path(name: &str) -> bool {
    if name == "." {
        return true;
    }
    if name.is_empty() {
        return false;
    }
    name.split('/').all(|e| !e.is_empty() && e != "." && e != "..")
}

/// The directory walk `resolveEmbed` runs for a matched directory.
///
/// Returns the number of regular files seen — which is *not* the number added,
/// because "contains no embeddable files" is decided on the count while the
/// file list is deduplicated per pattern.
fn walk_embed_dir(
    dir: &Path,
    pkgdir: &Path,
    all: bool,
    is_root: bool,
    out: &mut impl FnMut(String),
) -> Result<usize, String> {
    // A nested module ends the walk (`SkipDir`), and so does a bad or hidden
    // directory name — `.git` is both.
    if !is_root && dir.join("go.mod").exists() {
        return Ok(0);
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(0);
    };
    let mut names: Vec<(String, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), e.path()))
        .collect();
    names.sort();

    let mut count = 0usize;
    for (name, path) in names {
        let md = match std::fs::symlink_metadata(&path) {
            Ok(md) => md,
            Err(_) => continue,
        };
        let is_dir = md.is_dir();
        let hidden = name.starts_with('.') || name.starts_with('_');
        if is_bad_embed_name(&name) || (hidden && !all) {
            // Order matters: a directory is skipped whole even when its name is
            // the kind that would be an error on a file.
            if is_dir {
                continue;
            }
            if hidden {
                continue;
            }
            let rel = rel_slash(&path, pkgdir);
            return Err(format!("cannot embed file {rel}: invalid name {name}"));
        }
        if is_dir {
            count += walk_embed_dir(&path, pkgdir, all, false, out)?;
            continue;
        }
        if !md.is_file() {
            continue;
        }
        count += 1;
        out(rel_slash(&path, pkgdir));
    }
    Ok(count)
}

/// `isBadEmbedName`: rejected by `module.CheckFilePath`, or a VCS directory.
fn is_bad_embed_name(name: &str) -> bool {
    if matches!(name, "" | ".bzr" | ".hg" | ".git" | ".svn") {
        return true;
    }
    !check_file_elem(name)
}

/// `module.checkElem` for `filePath`, applied to one path element.
fn check_file_elem(elem: &str) -> bool {
    if elem.is_empty() || elem.contains('/') {
        return false;
    }
    if elem.chars().all(|c| c == '.') {
        return false;
    }
    if elem.ends_with('.') {
        return false;
    }
    if !elem.chars().all(file_name_ok) {
        return false;
    }
    // Windows device names are rejected on every platform.
    let short = elem.split('.').next().unwrap_or(elem);
    const BAD_WINDOWS: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    !BAD_WINDOWS.iter().any(|b| b.eq_ignore_ascii_case(short))
}

/// `module.fileNameOK`.
fn file_name_ok(r: char) -> bool {
    if r.is_ascii() {
        if r.is_ascii_alphanumeric() {
            return true;
        }
        return "!#$%&()+,-.=@[]^_{}~ ".contains(r);
    }
    r.is_alphabetic()
}

fn base_name(p: &Path) -> String {
    p.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `str.TrimFilePathPrefix` + `filepath.ToSlash`.
fn rel_slash(path: &Path, prefix: &Path) -> String {
    match path.strip_prefix(prefix) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Bad glob syntax, the only error `path.Match` and `filepath.Glob` return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadPattern;

/// `filepath.Glob` of `QuoteGlob(pkgdir) + "/" + glob`, returning absolute paths.
///
/// The directory prefix is glob-quoted upstream so it matches literally; we
/// keep that by walking out from `pkgdir` instead of re-globbing it, and only
/// take the `hasMeta` fast path when the *user's* pattern has no metacharacter.
fn glob_files(pkgdir: &Path, glob: &str) -> Result<Vec<PathBuf>, BadPattern> {
    // Well-formedness first: `Glob` checks `Match(pattern, "")` before anything.
    path_match(glob, "")?;

    if !has_meta(glob) {
        let full = pkgdir.join(glob);
        return Ok(if std::fs::symlink_metadata(&full).is_ok() {
            vec![full]
        } else {
            Vec::new()
        });
    }

    // Element-wise expansion, which is what the dir/file recursion in
    // `globWithLimit` amounts to once the prefix is literal.
    let mut dirs = vec![pkgdir.to_path_buf()];
    let elems: Vec<&str> = glob.split('/').collect();
    for (i, elem) in elems.iter().enumerate() {
        let last = i + 1 == elems.len();
        let mut next = Vec::new();
        if !has_meta(elem) {
            for d in &dirs {
                let cand = d.join(elem);
                // A literal element still has to exist; upstream reaches it
                // through `glob(dir, pattern)`, which reads the directory.
                if std::fs::symlink_metadata(&cand).is_ok() {
                    next.push(cand);
                }
            }
        } else {
            for d in &dirs {
                let Ok(entries) = std::fs::read_dir(d) else {
                    continue;
                };
                let mut names: Vec<String> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                names.sort();
                for name in names {
                    if path_match(elem, &name)? {
                        next.push(d.join(name));
                    }
                }
            }
        }
        if !last {
            next.retain(|p| p.is_dir());
        }
        dirs = next;
        if dirs.is_empty() {
            break;
        }
    }
    Ok(dirs)
}

/// `filepath.hasMeta` on a non-Windows host.
fn has_meta(s: &str) -> bool {
    s.contains(['*', '?', '[', '\\'])
}

/// `path.Match`, the shell-style matcher `//go:embed` patterns are checked and
/// expanded with. Separator is `/`, and `*` never crosses one.
pub fn path_match(pattern: &str, name: &str) -> Result<bool, BadPattern> {
    let mut pattern = pattern;
    let mut name = name;
    while !pattern.is_empty() {
        let (star, chunk, rest) = scan_chunk(pattern);
        pattern = rest;
        if star && chunk.is_empty() {
            // Trailing `*` takes the rest of the element.
            return Ok(!name.contains('/'));
        }
        match match_chunk(chunk, name) {
            Ok(Some(t)) if t.is_empty() || !pattern.is_empty() => {
                name = t;
                continue;
            }
            Err(e) => return Err(e),
            _ => {}
        }
        if star {
            let mut matched = false;
            for (i, c) in name.char_indices() {
                if c == '/' {
                    break;
                }
                let skip = &name[i + c.len_utf8()..];
                if let Some(t) = match_chunk(chunk, skip)? {
                    if pattern.is_empty() && !t.is_empty() {
                        continue;
                    }
                    name = t;
                    matched = true;
                    break;
                }
            }
            if matched {
                continue;
            }
        }
        // Even on failure the rest of the pattern must be well-formed.
        while !pattern.is_empty() {
            let (_, chunk, rest) = scan_chunk(pattern);
            pattern = rest;
            match_chunk(chunk, "")?;
        }
        return Ok(false);
    }
    Ok(name.is_empty())
}

/// `scanChunk`: leading stars, then the literal/class run up to the next star.
fn scan_chunk(pattern: &str) -> (bool, &str, &str) {
    let mut star = false;
    let mut p = pattern;
    while let Some(rest) = p.strip_prefix('*') {
        p = rest;
        star = true;
    }
    let b = p.as_bytes();
    let mut in_range = false;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'\\' => {
                if i + 1 < b.len() {
                    i += 1;
                }
            }
            b'[' => in_range = true,
            b']' => in_range = false,
            b'*' if !in_range => break,
            _ => {}
        }
        i += 1;
    }
    (star, &p[..i], &p[i..])
}

/// `matchChunk`: `Ok(Some(rest))` on a match, `Ok(None)` on a clean mismatch.
fn match_chunk<'a>(mut chunk: &str, s: &'a str) -> Result<Option<&'a str>, BadPattern> {
    let mut s = s;
    let mut failed = false;
    while !chunk.is_empty() {
        if !failed && s.is_empty() {
            failed = true;
        }
        match chunk.as_bytes()[0] {
            b'[' => {
                let mut r = '\0';
                if !failed {
                    let c = s.chars().next().unwrap();
                    r = c;
                    s = &s[c.len_utf8()..];
                }
                chunk = &chunk[1..];
                let negated = chunk.starts_with('^');
                if negated {
                    chunk = &chunk[1..];
                }
                let mut matched = false;
                let mut nrange = 0usize;
                loop {
                    if chunk.starts_with(']') && nrange > 0 {
                        chunk = &chunk[1..];
                        break;
                    }
                    let (lo, rest) = get_esc(chunk)?;
                    chunk = rest;
                    let mut hi = lo;
                    if chunk.starts_with('-') {
                        let (h, rest) = get_esc(&chunk[1..])?;
                        hi = h;
                        chunk = rest;
                    }
                    if lo <= r && r <= hi {
                        matched = true;
                    }
                    nrange += 1;
                }
                if matched == negated {
                    failed = true;
                }
            }
            b'?' => {
                if !failed {
                    let c = s.chars().next().unwrap();
                    if c == '/' {
                        failed = true;
                    }
                    s = &s[c.len_utf8()..];
                }
                chunk = &chunk[1..];
            }
            b'\\' => {
                chunk = &chunk[1..];
                if chunk.is_empty() {
                    return Err(BadPattern);
                }
                if !failed {
                    let pc = chunk.chars().next().unwrap();
                    let sc = s.chars().next().unwrap();
                    if pc != sc {
                        failed = true;
                    } else {
                        s = &s[sc.len_utf8()..];
                    }
                }
                let pc = chunk.chars().next().unwrap();
                chunk = &chunk[pc.len_utf8()..];
            }
            _ => {
                let pc = chunk.chars().next().unwrap();
                if !failed {
                    let sc = s.chars().next().unwrap();
                    if pc != sc {
                        failed = true;
                    } else {
                        s = &s[sc.len_utf8()..];
                    }
                }
                chunk = &chunk[pc.len_utf8()..];
            }
        }
    }
    if failed {
        return Ok(None);
    }
    Ok(Some(s))
}

/// `getEsc`: one (possibly escaped) rune inside a `[...]` class.
fn get_esc(chunk: &str) -> Result<(char, &str), BadPattern> {
    if chunk.is_empty() || chunk.starts_with('-') || chunk.starts_with(']') {
        return Err(BadPattern);
    }
    let mut chunk = chunk;
    if let Some(rest) = chunk.strip_prefix('\\') {
        chunk = rest;
        if chunk.is_empty() {
            return Err(BadPattern);
        }
    }
    let c = chunk.chars().next().unwrap();
    let rest = &chunk[c.len_utf8()..];
    if rest.is_empty() {
        return Err(BadPattern);
    }
    Ok((c, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_match_follows_the_go_rules() {
        assert_eq!(path_match("*.tmpl", "a.tmpl"), Ok(true));
        assert_eq!(path_match("*.tmpl", "a.txt"), Ok(false));
        // `*` never crosses a separator.
        assert_eq!(path_match("*", "a/b"), Ok(false));
        assert_eq!(path_match("a/*", "a/b"), Ok(true));
        assert_eq!(path_match("?.go", "a.go"), Ok(true));
        assert_eq!(path_match("?.go", "/.go"), Ok(false));
        assert_eq!(path_match("[a-c]x", "bx"), Ok(true));
        assert_eq!(path_match("[^a-c]x", "bx"), Ok(false));
        assert_eq!(path_match("[^a-c]x", "dx"), Ok(true));
        assert_eq!(path_match("\\*", "*"), Ok(true));
        // Unterminated class is the syntax error `resolveEmbed` reports.
        assert_eq!(path_match("[a-", ""), Err(BadPattern));
        assert_eq!(path_match("a\\", ""), Err(BadPattern));
    }

    #[test]
    fn valid_embed_pattern_rejects_dot_and_parent_escapes() {
        assert!(valid_embed_pattern("app/dist"));
        assert!(valid_embed_pattern("*.tmpl"));
        assert!(!valid_embed_pattern("."));
        assert!(!valid_embed_pattern("../a"));
        assert!(!valid_embed_pattern("/abs"));
        assert!(!valid_embed_pattern("a//b"));
        assert!(!valid_embed_pattern(""));
    }

    #[test]
    fn bad_embed_names_are_the_ones_vendoring_would_drop() {
        assert!(is_bad_embed_name(".git"));
        assert!(is_bad_embed_name(".svn"));
        assert!(is_bad_embed_name(""));
        assert!(is_bad_embed_name(".."));
        assert!(is_bad_embed_name("x."));
        assert!(is_bad_embed_name("con.txt"));
        assert!(is_bad_embed_name("a*b"));
        assert!(!is_bad_embed_name("index.html"));
        assert!(!is_bad_embed_name(".hidden"));
        assert!(!is_bad_embed_name("with space.txt"));
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "guff-embed-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_missing_pattern_names_itself() {
        let dir = tmp("missing");
        assert_eq!(
            resolve_embed(&dir, &["app/dist".to_string()]),
            Err(err("app/dist", "no matching files found"))
        );
        assert_eq!(
            resolve_embed(&dir, &["app/dist".to_string()])
                .unwrap_err()
                .text(),
            "pattern app/dist: no matching files found"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_first_failing_pattern_wins_in_sorted_order() {
        let dir = tmp("first");
        std::fs::write(dir.join("have.txt"), "x").unwrap();
        // `go/build` sorts, so `have.txt` is resolved before `nope.txt`.
        assert_eq!(
            resolve_embed(&dir, &["have.txt".to_string(), "nope.txt".to_string()])
                .unwrap_err()
                .pattern,
            "nope.txt"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_contributes_its_files_and_skips_dot_names() {
        let dir = tmp("dir");
        std::fs::create_dir_all(dir.join("data/sub")).unwrap();
        std::fs::write(dir.join("data/x.txt"), "x").unwrap();
        std::fs::write(dir.join("data/sub/y.txt"), "y").unwrap();
        std::fs::write(dir.join("data/.hidden"), "h").unwrap();
        std::fs::write(dir.join("data/_under"), "u").unwrap();
        assert_eq!(
            resolve_embed(&dir, &["data".to_string()]),
            Ok(vec!["data/sub/y.txt".to_string(), "data/x.txt".to_string()])
        );
        // `all:` takes the hidden ones too.
        assert_eq!(
            resolve_embed(&dir, &["all:data".to_string()]),
            Ok(vec![
                "data/.hidden".to_string(),
                "data/_under".to_string(),
                "data/sub/y.txt".to_string(),
                "data/x.txt".to_string(),
            ])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_directory_of_only_dot_names_is_not_empty_but_unembeddable() {
        let dir = tmp("onlydot");
        std::fs::create_dir_all(dir.join("only/.git")).unwrap();
        std::fs::write(dir.join("only/.git/config"), "x").unwrap();
        assert_eq!(
            resolve_embed(&dir, &["only".to_string()]).unwrap_err().text(),
            "pattern only: cannot embed directory only: contains no embeddable files"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_nested_module_ends_the_walk() {
        let dir = tmp("nested");
        std::fs::create_dir_all(dir.join("data/inner")).unwrap();
        std::fs::write(dir.join("data/keep.txt"), "x").unwrap();
        std::fs::write(dir.join("data/inner/go.mod"), "module x\n").unwrap();
        std::fs::write(dir.join("data/inner/drop.txt"), "y").unwrap();
        assert_eq!(
            resolve_embed(&dir, &["data".to_string()]),
            Ok(vec!["data/keep.txt".to_string()])
        );
        // Naming the nested module directly is the "in different module" case.
        assert_eq!(
            resolve_embed(&dir, &["data/inner".to_string()])
                .unwrap_err()
                .text(),
            "pattern data/inner: cannot embed directory data/inner: in different module"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_glob_that_matches_nothing_still_fails() {
        let dir = tmp("glob");
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        assert_eq!(
            resolve_embed(&dir, &["*.tmpl".to_string()])
                .unwrap_err()
                .text(),
            "pattern *.tmpl: no matching files found"
        );
        assert_eq!(
            resolve_embed(&dir, &["*.txt".to_string()]),
            Ok(vec!["a.txt".to_string()])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_parent_escape_is_a_syntax_error_not_a_missing_file() {
        let dir = tmp("syntax");
        assert_eq!(
            resolve_embed(&dir, &["../a".to_string()]).unwrap_err().text(),
            "pattern ../a: invalid pattern syntax"
        );
        assert_eq!(
            resolve_embed(&dir, &[".".to_string()]).unwrap_err().text(),
            "pattern .: invalid pattern syntax"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_all_prefix_stays_in_the_reported_pattern() {
        let dir = tmp("allprefix");
        assert_eq!(
            resolve_embed(&dir, &["all:hidden".to_string()])
                .unwrap_err()
                .text(),
            "pattern all:hidden: no matching files found"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_patterns_is_no_files_and_no_error() {
        let dir = tmp("none");
        assert_eq!(resolve_embed(&dir, &[]), Ok(Vec::new()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
