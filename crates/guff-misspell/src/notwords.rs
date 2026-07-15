//! Strip URLs, paths, emails, and hostnames before spell-checking word positions.
//!
//! Port of `misspell/notwords.go` and `misspell/url.go` (simplified).

use regex::Regex;
use std::sync::LazyLock;

static RE_EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[[:alnum:]_.%+-]+@[[:alnum:]-.]+\.[[:alpha:]]{2,6}[^[:alpha:]]").unwrap());
static RE_HOST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([[:alnum:]-]+\.)+[[:alpha:]]{2,63}").unwrap());
static RE_BACKSLASH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\\[[:lower:]]").unwrap());
static RE_URL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(https?|ftp)://(-\.)?([^\s/?.#]+\.?)+(/\S*)?").unwrap()
});

fn replace_with_blanks(s: &str) -> String {
    " ".repeat(s.len())
}

fn replace_host(s: &str) -> String {
    if s.chars().any(|c| c.is_ascii_uppercase()) {
        s.to_string()
    } else {
        replace_with_blanks(s)
    }
}

fn remove_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let Some(idx) = rest.find('/') else {
            out.push_str(rest);
            break;
        };
        let _start = idx.saturating_sub(1);
        let chclass = match rest.as_bytes().get(idx) {
            Some(b'/') | Some(b' ') | Some(b'\n') | Some(b'\t') | Some(b'\r') => " \n\r\t",
            Some(b'[') => "]\n",
            Some(b'(') => ")\n",
            _ => {
                let take = (idx + 2).min(rest.len());
                out.push_str(&rest[..take]);
                rest = &rest[take..];
                continue;
            }
        };
        if let Some(endx) = rest[idx + 1..].find(|c: char| chclass.contains(c)) {
            out.push_str(&rest[..idx + 1]);
            out.push_str(&" ".repeat(endx));
            rest = &rest[idx + endx + 1..];
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}

fn strip_url(s: &str) -> String {
    RE_URL
        .replace_all(s, |caps: &regex::Captures| replace_with_blanks(&caps[0]))
        .into_owned()
}

/// Blank out substrings that should not be spell-checked.
pub fn remove_not_words(s: &str) -> String {
    let s = strip_url(s);
    let s = remove_path(&s);
    let s = RE_EMAIL
        .replace_all(&s, |caps: &regex::Captures| replace_with_blanks(&caps[0]))
        .into_owned();
    let s = RE_HOST
        .replace_all(&s, |caps: &regex::Captures| replace_host(&caps[0]))
        .into_owned();
    RE_BACKSLASH
        .replace_all(&s, |caps: &regex::Captures| replace_with_blanks(&caps[0]))
        .into_owned()
}
