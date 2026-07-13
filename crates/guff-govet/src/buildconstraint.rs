//! Minimal `go/build/constraint` parsing for the buildtag analyzer.

pub fn is_go_build_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("//go:build") || line.starts_with("// go:build")
}

pub fn is_plus_build_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("// +build") || line.starts_with("//+build")
}

pub fn parse_go_build(line: &str) -> Result<(), String> {
    let line = line.trim();
    let rest = line
        .strip_prefix("//go:build")
        .or_else(|| line.strip_prefix("// go:build"))
        .ok_or_else(|| "not a go:build line".to_string())?;
    if !rest.is_empty() && !rest.starts_with(' ') && !rest.starts_with('\t') {
        return Err("malformed //go:build line (space between // and go:build)".into());
    }
    let expr = rest.trim();
    if expr.is_empty() {
        return Err("empty expression".into());
    }
    parse_expr(expr)
}

pub fn parse_plus_build(line: &str) -> Result<(), String> {
    let line = line.trim();
    let rest = line
        .strip_prefix("// +build")
        .or_else(|| line.strip_prefix("//+build"))
        .ok_or_else(|| "not a +build line".to_string())?;
    for arg in rest.split_whitespace() {
        for elem in arg.split(',') {
            let elem = elem.trim();
            if elem.starts_with("!!") {
                return Err(format!("invalid double negative in build constraint: {arg}"));
            }
            let elem = elem.strip_prefix('!').unwrap_or(elem);
            for c in elem.chars() {
                if !c.is_ascii_alphanumeric() && c != '_' && c != '.' {
                    return Err(format!("invalid non-alphanumeric build constraint: {arg}"));
                }
            }
            if malformed_go_tag(elem) {
                return Err(format!("invalid go version {elem:?} in build constraint"));
            }
        }
    }
    Ok(())
}

fn parse_expr(expr: &str) -> Result<(), String> {
    let mut depth = 0;
    for c in expr.chars() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return Err("unbalanced parentheses".into());
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("unbalanced parentheses".into());
    }
    Ok(())
}

pub fn malformed_go_tag(tag: &str) -> bool {
    if !tag.starts_with("go1") {
        for pre in ["go.", "g1.", "go"] {
            let suffix = tag.strip_prefix(pre).unwrap_or(tag);
            if suffix != tag && valid_go_version(&format!("go1.{suffix}")) {
                return true;
            }
        }
        return false;
    }
    !valid_go_version(tag)
}

fn valid_go_version(tag: &str) -> bool {
    let Some(rest) = tag.strip_prefix("go1.").or_else(|| tag.strip_prefix("go1")) else {
        return false;
    };
    if rest.is_empty() {
        return tag == "go1";
    }
    rest.chars().all(|c| c.is_ascii_digit())
}
