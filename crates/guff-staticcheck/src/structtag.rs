//! Struct tag parsing for SA5008.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5008/structtag.go`.

use std::collections::HashMap;

pub fn parse_struct_tag(tag: &str) -> Result<HashMap<String, Vec<String>>, String> {
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut rest = tag;
    while !rest.is_empty() {
        let mut i = 0;
        while i < rest.len() && rest.as_bytes()[i] == b' ' {
            i += 1;
        }
        rest = &rest[i..];
        if rest.is_empty() {
            break;
        }
        i = 0;
        while i < rest.len() {
            let c = rest.as_bytes()[i];
            if c <= b' ' || c == b':' || c == b'"' || c == 0x7f {
                break;
            }
            i += 1;
        }
        if i == 0 || i + 1 >= rest.len() || rest.as_bytes()[i] != b':' || rest.as_bytes()[i + 1] != b'"'
        {
            return Err("malformed struct tag".into());
        }
        let name = &rest[..i];
        rest = &rest[i + 1..];
        let mut j = 1;
        while j < rest.len() && rest.as_bytes()[j] != b'"' {
            if rest.as_bytes()[j] == b'\\' {
                j += 1;
            }
            j += 1;
        }
        if j >= rest.len() {
            return Err("malformed struct tag".into());
        }
        let qvalue = &rest[..=j];
        rest = &rest[j + 1..];
        let value = unquote(qvalue).map_err(|e| e.to_string())?;
        out.entry(name.to_string()).or_default().push(value);
    }
    Ok(out)
}

fn unquote(s: &str) -> Result<String, &'static str> {
    if s.len() < 2 || !s.starts_with('"') || !s.ends_with('"') {
        return Err("invalid quoted string");
    }
    Ok(s[1..s.len() - 1].replace("\\\"", "\"").replace("\\\\", "\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_tag() {
        let tags = parse_struct_tag(r#"json:"name,omitempty""#).unwrap();
        assert_eq!(tags.get("json").map(|v| v[0].as_str()), Some("name,omitempty"));
    }
}
