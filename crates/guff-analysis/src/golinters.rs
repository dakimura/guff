//! Helpers golangci-lint's own linter wrappers share
//! (`pkg/golinters/internal`).

/// `internal.FormatCode`: `%#q` — backquoted when that reads back unchanged,
/// a double-quoted Go string otherwise. A code holding a backquote is left
/// bare (upstream's own TODO).
///
/// Up to golangci-lint 2.12 this was always `` `code` ``; 2.14 switched to
/// `%#q`, which only differs for a control character, DEL or a BOM.
pub fn format_code(code: &str) -> String {
    if code.contains('`') {
        return code.to_string();
    }
    guff_gostd::strconv::quote_sharp(code)
}

/// `fmt.Sprintf("%#q", s)` — what misspell's and nolintlint's messages use
/// directly, without `FormatCode`'s backquote exception: a string holding a
/// backquote is double-quoted.
pub fn sharp_q(s: &str) -> String {
    guff_gostd::strconv::quote_sharp(s)
}

/// Port of golangci-lint's `fsutils.ShortestRelPath(path, "")`: the path
/// relative to the process working directory, with symlinks resolved. Falls
/// back to the path as given when either step fails, as there is nothing better
/// to say.
pub fn shortest_rel_path(path: &str) -> String {
    let p = std::path::Path::new(path);
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let Ok(wd) = std::env::current_dir() else {
        return path.to_string();
    };
    let wd = std::fs::canonicalize(&wd).unwrap_or(wd);
    match resolved.strip_prefix(&wd) {
        Ok(rel) => rel.to_string_lossy().into_owned(),
        Err(_) => resolved.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_code;

    #[test]
    fn format_code_matches_sharp_q() {
        assert_eq!(format_code("foo"), "`foo`");
        assert_eq!(format_code("a`b"), "a`b");
        assert_eq!(format_code("a\tb"), "`a\tb`");
        assert_eq!(format_code("a\nb"), "\"a\\nb\"");
        assert_eq!(format_code("\u{7f}"), "\"\\x7f\"");
        assert_eq!(super::sharp_q("a`b"), "\"a`b\"");
    }
}
