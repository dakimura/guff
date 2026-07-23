//! Strip URLs, paths, emails, and hostnames before spell-checking word positions.
//!
//! Port of `misspell/notwords.go` and `misspell/url.go` (simplified).

use regex::Regex;
use std::borrow::Cow;
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

fn remove_path(s: &str) -> Cow<'_, str> {
    if !s.as_bytes().contains(&b'/') {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while !rest.is_empty() {
        let Some(idx) = rest.find('/') else {
            out.push_str(rest);
            break;
        };
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
    Cow::Owned(out)
}

/// Apply `re.replace_all`, preserving `Cow::Borrowed` when nothing changes.
fn replace_cow<'a>(
    s: Cow<'a, str>,
    re: &Regex,
    repl: impl Fn(&regex::Captures<'_>) -> String,
) -> Cow<'a, str> {
    match s {
        Cow::Borrowed(b) => match re.replace_all(b, &repl) {
            Cow::Borrowed(b2) => Cow::Borrowed(b2),
            Cow::Owned(o) => Cow::Owned(o),
        },
        Cow::Owned(o) => match re.replace_all(&o, &repl) {
            // No match: keep the existing allocation.
            Cow::Borrowed(_) => Cow::Owned(o),
            Cow::Owned(o2) => Cow::Owned(o2),
        },
    }
}

fn strip_url(s: &str) -> Cow<'_, str> {
    // Scheme always contains "://"; skip the URL regex otherwise.
    if !s.contains("://") {
        return Cow::Borrowed(s);
    }
    RE_URL.replace_all(s, |caps: &regex::Captures| replace_with_blanks(&caps[0]))
}

/// Blank out substrings that should not be spell-checked.
///
/// Returns `Cow::Borrowed` when the line needs no redaction, avoiding the
/// previous per-line chain of ~5 owned `String`s on the common path.
pub fn remove_not_words(s: &str) -> Cow<'_, str> {
    let bytes = s.as_bytes();
    let has_slash = bytes.contains(&b'/');
    let has_at = bytes.contains(&b'@');
    let has_dot = bytes.contains(&b'.');
    let has_bs = bytes.contains(&b'\\');

    // Host / URL / path / email / escape redaction all require at least one
    // of these markers. Plain prose lines skip every regex.
    if !has_slash && !has_at && !has_dot && !has_bs {
        return Cow::Borrowed(s);
    }

    let mut out: Cow<'_, str> = Cow::Borrowed(s);

    if has_slash {
        out = match out {
            Cow::Borrowed(b) => strip_url(b),
            Cow::Owned(o) => match strip_url(&o) {
                Cow::Borrowed(_) => Cow::Owned(o),
                Cow::Owned(o2) => Cow::Owned(o2),
            },
        };
        out = match out {
            Cow::Borrowed(b) => remove_path(b),
            Cow::Owned(o) => match remove_path(&o) {
                Cow::Borrowed(_) => Cow::Owned(o),
                Cow::Owned(o2) => Cow::Owned(o2),
            },
        };
    }

    if has_at {
        out = replace_cow(out, &RE_EMAIL, |caps| replace_with_blanks(&caps[0]));
    }

    if has_dot {
        out = replace_cow(out, &RE_HOST, |caps| replace_host(&caps[0]));
    }

    if has_bs {
        out = replace_cow(out, &RE_BACKSLASH, |caps| replace_with_blanks(&caps[0]));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_line_is_borrowed() {
        let s = "grill brocoli now";
        match remove_not_words(s) {
            Cow::Borrowed(b) => assert_eq!(b, s),
            Cow::Owned(_) => panic!("expected borrowed for plain line"),
        }
    }

    #[test]
    fn url_is_blanked() {
        let s = "see https://example.com/foo for docs";
        let out = remove_not_words(s);
        assert!(!out.contains("example"));
        assert_eq!(out.len(), s.len());
    }

    #[test]
    fn email_is_blanked() {
        let s = "contact user@example.com please";
        let out = remove_not_words(s);
        assert!(!out.contains("user@example"));
        assert_eq!(out.len(), s.len());
    }

    #[test]
    fn backslash_escape_is_blanked() {
        let s = r"path \n next";
        let out = remove_not_words(s);
        assert!(!out.as_ref().contains("\\n"));
        assert_eq!(out.len(), s.len());
    }
}
