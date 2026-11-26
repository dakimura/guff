//! Lightweight parsing of Go source file headers for `go/build`.

/// Information extracted from the header of a `.go` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoFileInfo {
    pub package_name: String,
    pub imports_c: bool,
    /// Import paths in declaration order (including `"C"` when present).
    pub imports: Vec<String>,
}

/// Parses `package` name, import paths, and whether the file imports `"C"`.
///
/// Port of the package/import scan in `build.readGoInfo` (simplified).
pub fn parse_go_file_info(content: &[u8]) -> Result<GoFileInfo, String> {
    let mut data = strip_bom(content);
    data = skip_space_and_comments(data);

    let (word, rest) = parse_word(data).ok_or_else(|| "expected package clause".to_string())?;
    if word != b"package" {
        return Err("expected package clause".to_string());
    }

    let data = skip_space_and_comments(rest);
    let (pkg_bytes, mut data) = parse_word(data).ok_or_else(|| "expected package name".to_string())?;
    let package_name = std::str::from_utf8(pkg_bytes)
        .map_err(|_| "invalid package name".to_string())?
        .to_string();

    if package_name == "documentation" {
        return Ok(GoFileInfo {
            package_name,
            imports_c: false,
            imports: Vec::new(),
        });
    }

    let mut imports_c = false;
    let mut imports = Vec::new();
    loop {
        data = skip_space_and_comments(data);
        if data.is_empty() {
            break;
        }
        let (word, rest) = match parse_word(data) {
            Some(p) => p,
            None => break,
        };
        if word != b"import" {
            break;
        }
        data = skip_space_and_comments(rest);
        if data.first() == Some(&b'(') {
            data = &data[1..];
            loop {
                data = skip_space_and_comments(data);
                if data.first() == Some(&b')') {
                    data = &data[1..];
                    break;
                }
                if data.is_empty() {
                    break;
                }
                if let Some(path) = parse_import_path_spec(data) {
                    if path == "C" {
                        imports_c = true;
                    }
                    imports.push(path.to_string());
                }
                let rest = skip_import_spec(data);
                // A spec shape we cannot parse leaves `data` untouched; without
                // this guard the loop spins forever on it. go/build stops the
                // import scan at a syntax error, so stop here too.
                if rest.len() == data.len() {
                    break;
                }
                data = rest;
            }
        } else if let Some(path) = parse_import_path_spec(data) {
            if path == "C" {
                imports_c = true;
            }
            imports.push(path.to_string());
            data = skip_import_spec(data);
        } else {
            break;
        }
    }

    Ok(GoFileInfo {
        package_name,
        imports_c,
        imports,
    })
}

fn strip_bom(content: &[u8]) -> &[u8] {
    if content.starts_with(&[0xef, 0xbb, 0xbf]) {
        &content[3..]
    } else {
        content
    }
}

fn skip_space_and_comments(mut data: &[u8]) -> &[u8] {
    loop {
        while let Some(&b) = data.first() {
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b';') {
                data = &data[1..];
            } else {
                break;
            }
        }
        if data.starts_with(b"//") {
            if let Some(i) = data.iter().position(|&b| b == b'\n') {
                data = &data[i + 1..];
                continue;
            }
            return &[];
        }
        if data.starts_with(b"/*") {
            if let Some(i) = find_subslice(&data[2..], b"*/") {
                data = &data[2 + i + 2..];
                continue;
            }
            return &[];
        }
        break;
    }
    data
}

fn parse_word(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let data = skip_space_and_comments(data);
    if data.is_empty() {
        return None;
    }
    let mut end = 0;
    while end < data.len() {
        let b = data[end];
        if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    Some((&data[..end], &data[end..]))
}

/// True for the two quote bytes that can open an import path.
fn is_quote(b: Option<&u8>) -> bool {
    matches!(b, Some(&b'"') | Some(&b'`'))
}

/// Reads a quoted import path at the start of `data`, returning the path and
/// the number of bytes it occupies (quotes included).
///
/// `importReader.readString` (go/build) accepts an interpreted string (`"p"`)
/// *and* a raw string literal (`` `p` ``); asm2asm-generated files write the
/// backquoted form.
fn read_quoted(data: &[u8]) -> Option<(&str, usize)> {
    let quote = *data.first()?;
    if quote != b'"' && quote != b'`' {
        return None;
    }
    let rest = &data[1..];
    let end = rest.iter().position(|&b| b == quote)?;
    Some((std::str::from_utf8(&rest[..end]).ok()?, end + 2))
}

fn parse_import_path(data: &[u8]) -> Option<&str> {
    let data = skip_space_and_comments(data);
    read_quoted(data).map(|(path, _)| path)
}

/// Parses an import spec path, allowing an optional identifier alias before the string.
fn parse_import_path_spec(data: &[u8]) -> Option<&str> {
    let data = skip_space_and_comments(data);
    if is_quote(data.first()) {
        return parse_import_path(data);
    }
    // Optional name / `.` / `_` before the path string.
    let rest = if data.first() == Some(&b'.') {
        &data[1..]
    } else if let Some((_, rest)) = parse_word(data) {
        rest
    } else {
        return None;
    };
    parse_import_path(rest)
}

fn skip_import_spec(mut data: &[u8]) -> &[u8] {
    data = skip_space_and_comments(data);
    // Do not treat identifiers that merely *start with* `import` (e.g. `importCmd`)
    // as the `import` keyword — that failed to advance and spun forever while
    // `parse_go_file_info` pushed the same path on every iteration (cli
    // `pkg/cmd/alias` OOM).
    if is_quote(data.first()) {
        return match read_quoted(data) {
            Some((_, len)) => &data[len..],
            None => &[],
        };
    }
    // Skip optional name / `.` / `_` before path.
    let after_name = if data.first() == Some(&b'.') {
        &data[1..]
    } else if let Some((_, rest)) = parse_word(data) {
        rest
    } else {
        return data;
    };
    let rest = skip_space_and_comments(after_name);
    if is_quote(rest.first()) {
        return match read_quoted(rest) {
            Some((_, len)) => &rest[len..],
            None => &[],
        };
    }
    data
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}
