//! Struct tag parsing for SA5008.
//!
//! Port of `honnef.co/go/tools/staticcheck/sa5008/structtag.go`, which is
//! itself a copy of `reflect.StructTag.Lookup`'s scanner.
//!
//! The scanner is **lenient by design**: anything that does not look like a
//! `name:"value"` pair simply ends the scan, and what was parsed so far is
//! returned. Only `strconv.Unquote` failing is an error. That distinction is
//! the whole reported/silent boundary for the `unparseable struct tag`
//! diagnostic — a tag of `` `notatag` ``, `` `json` ``, `` `json:"e `` or a
//! valid tag with trailing junk is *not* a finding upstream, while
//! `` `json:"\q"` `` is.

use std::collections::HashMap;

use crate::gostd::strconv;

/// Parses a struct tag into `name -> values`, in the order the names appear.
///
/// `Err` only for an unquotable value, carrying `strconv`'s own message so the
/// diagnostic reads `unparseable struct tag: invalid syntax` as upstream's
/// does.
pub fn parse_struct_tag(tag: &str) -> Result<Vec<(String, Vec<String>)>, String> {
    let mut order: Vec<String> = Vec::new();
    let mut out: HashMap<String, Vec<String>> = HashMap::new();
    let mut rest = tag;
    while !rest.is_empty() {
        // Skip leading space.
        let mut i = 0;
        while i < rest.len() && rest.as_bytes()[i] == b' ' {
            i += 1;
        }
        rest = &rest[i..];
        if rest.is_empty() {
            break;
        }

        // Scan to colon. A space, a quote or a control character ends the name.
        i = 0;
        while i < rest.len() {
            let c = rest.as_bytes()[i];
            if c <= b' ' || c == b':' || c == b'"' || c == 0x7f {
                break;
            }
            i += 1;
        }
        // Not a `name:"` pair: stop scanning and keep what we have. Upstream
        // `break`s here rather than reporting, so a struct tag that is not a
        // tag at all is silent.
        if i == 0
            || i + 1 >= rest.len()
            || rest.as_bytes()[i] != b':'
            || rest.as_bytes()[i + 1] != b'"'
        {
            break;
        }
        let name = &rest[..i];
        rest = &rest[i + 1..];

        // Scan the quoted string to find the value.
        let mut j = 1;
        while j < rest.len() && rest.as_bytes()[j] != b'"' {
            if rest.as_bytes()[j] == b'\\' {
                j += 1;
            }
            j += 1;
        }
        // Unterminated quote: also a plain `break` upstream.
        if j >= rest.len() {
            break;
        }
        let qvalue = &rest[..=j];
        rest = &rest[j + 1..];

        let value = strconv::unquote(qvalue).map_err(|e| e.text().to_string())?;
        let entry = out.entry(name.to_string());
        if matches!(entry, std::collections::hash_map::Entry::Vacant(_)) {
            order.push(name.to_string());
        }
        entry.or_default().push(value);
    }
    Ok(order
        .into_iter()
        .map(|name| {
            let values = out.remove(&name).unwrap_or_default();
            (name, values)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<'a>(tags: &'a [(String, Vec<String>)], key: &str) -> Option<&'a [String]> {
        tags.iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_slice())
    }

    #[test]
    fn parses_json_tag() {
        let tags = parse_struct_tag(r#"json:"name,omitempty""#).unwrap();
        assert_eq!(
            get(&tags, "json").map(|v| v[0].as_str()),
            Some("name,omitempty")
        );
    }

    /// Everything the scanner cannot read is silently dropped, not an error:
    /// these four shapes were all reported as `unparseable struct tag` before,
    /// and none of them is a finding upstream.
    #[test]
    fn malformed_structure_stops_the_scan_without_an_error() {
        for tag in ["notatag", "json", r#"json:"e"#, r#"json:"b" trailing"#] {
            let tags = parse_struct_tag(tag).unwrap_or_else(|e| panic!("{tag:?} errored: {e}"));
            // The valid prefix, if any, still parses.
            let expected = if tag.starts_with(r#"json:"b""#) { 1 } else { 0 };
            assert_eq!(tags.len(), expected, "{tag:?} -> {tags:?}");
        }
    }

    /// An unquotable *value* is the one real error, and it carries strconv's
    /// own wording.
    #[test]
    fn an_invalid_escape_is_the_only_error() {
        assert_eq!(
            parse_struct_tag(r#"json:"\q""#).unwrap_err(),
            "invalid syntax"
        );
        // Valid escapes are not.
        assert!(parse_struct_tag(r#"json:"a\tb""#).is_ok());
        assert!(parse_struct_tag(r#"json:"\x41""#).is_ok());
    }

    /// Names keep source order so a field carrying both `json` and `xml` tags
    /// reports deterministically (Go's map iteration is random, and golangci
    /// sorts by position only).
    #[test]
    fn names_keep_source_order() {
        let tags = parse_struct_tag(r#"xml:"x" json:"j""#).unwrap();
        assert_eq!(
            tags.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
            ["xml", "json"]
        );
    }

    #[test]
    fn repeated_names_collect_every_value() {
        let tags = parse_struct_tag(r#"choice:"a" choice:"b""#).unwrap();
        assert_eq!(get(&tags, "choice").map(|v| v.len()), Some(2));
    }
}
