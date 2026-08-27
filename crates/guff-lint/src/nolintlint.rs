//! Port of golangci-lint's `nolintlint` linter
//! (`pkg/golinters/nolintlint/internal/nolintlint.go`).
//!
//! nolintlint reports on the `//nolint` directive *itself*, independently of
//! whether it suppressed anything: a leading space, a shape the directive
//! parser will not accept, a missing linter list, a missing explanation. The
//! fifth kind — "this directive is unused" — is a candidate that only the
//! nolint filter can settle, so it is produced in [`crate::nolint`] instead.
//!
//! Note that this file parses the directive a *second* time, with its own
//! regexes, and deliberately does not share the filter's parse: upstream keeps
//! the two apart and they disagree on real inputs. `//nolint:a b` is one
//! unknown linter named `a b` to the filter and a malformed directive to
//! nolintlint; `//nolint:ErrCheck` is `errcheck` to the filter (which
//! lowercases and resolves aliases) and the literal `ErrCheck` here.

use std::sync::OnceLock;

use regex::Regex;

/// Go's `\s` and `\w` are ASCII-only; Rust's are Unicode by default, so the
/// classes are spelled out rather than written `\s` / `\w`.
const SPACE: &str = "[\t\n\x0C\r ]";
const WORD: &str = "[0-9A-Za-z_]";

/// Which of nolintlint's optional checks are on.
///
/// "Machine-readable" is not here: `NewLinter` ORs `NeedsMachineOnly` into
/// `needs` unconditionally, so it runs whenever nolintlint is enabled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NolintlintStyle {
    /// `require-explanation`.
    pub require_explanation: bool,
    /// `require-specific`.
    pub require_specific: bool,
    /// `allow-no-explanation`: linters exempt from `require-explanation`.
    pub allow_no_explanation: Vec<String>,
    /// `allow-unused` inverted: report directives that suppressed nothing.
    /// Not consulted by [`messages`] — the filter is the only side that knows.
    pub report_unused: bool,
}

/// One `//nolint` comment as nolintlint parses it.
#[derive(Debug, Clone)]
pub struct Directive {
    /// The comment text, verbatim, as it appears in the message.
    pub text: String,
    pub line: i64,
    pub col: i64,
    /// Byte offset of the comment's first character in its file.
    ///
    /// Upstream's fixes are spelled in offsets — it writes
    /// `Pos: token.Pos(pos.Offset)` and relies on golangci's applier casting
    /// `token.Pos` straight back to an `int` without consulting the `FileSet`.
    /// guff's applier does consult it (`fix::resolve_edit`), so the offset is
    /// carried here and converted to a FileSet-wide `Pos` when the fix is
    /// built. That conversion is the port, not a workaround: the same
    /// offset-as-`Pos` conflation is what silently disables revive's `--fix`
    /// upstream (COMPAT-HARDENING 続き 44).
    pub offset: i64,
    /// Linter names exactly as written — not lowercased, not resolved through
    /// the registry. Empty for a bare `//nolint` and for anything starting
    /// with `all`.
    pub linters: Vec<String>,
    /// The directive does not match `fullDirectivePattern`; upstream stops
    /// after reporting it, so such a directive yields no unused candidates.
    pub malformed: bool,
}

fn comment_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^//{SPACE}*(nolint)(:{SPACE}*[0-9A-Za-z_\-]+{SPACE}*(?:,{SPACE}*[0-9A-Za-z_\-]+{SPACE}*)*)?\b"
        ))
        .expect("nolintlint comment regex")
    })
}

fn full_directive_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(
            r"^//{SPACE}*nolint(?::({SPACE}*[0-9A-Za-z_\-]+{SPACE}*(?:,{SPACE}*[0-9A-Za-z_\-]+{SPACE}*)*))?{SPACE}*(//.*)?{SPACE}*\n?$"
        ))
        .expect("nolintlint full directive regex")
    })
}

fn leading_space_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"^//({SPACE}*)")).expect("nolintlint leading space"))
}

fn trailing_blank_explanation() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(r"{SPACE}*(//{SPACE}*)?$")).expect("nolintlint trailing explanation")
    })
}

/// True when this comment is a `//nolint` directive in nolintlint's sense.
///
/// Not the same question as the filter's `^nolint( |:|$)`: `//nolintfoo` is
/// neither, but `//nolint :x` is a directive to both — malformed here, and
/// "suppress everything" there.
pub fn is_directive(text: &str) -> bool {
    comment_pattern().is_match(text)
}

/// The `//` prefix as the message should spell it: with one space when the
/// directive has any leading whitespace, and with the word that follows.
fn directive_with_optional_leading_space(text: &str, leading_space: &str) -> String {
    let mut out = String::from("//");
    if !leading_space.is_empty() {
        out.push(' ');
    }
    // `strings.Split(strings.SplitN(text, ":", 2)[0], "//")[1]`, trimmed.
    let before_colon = text.split_once(':').map_or(text, |(head, _)| head);
    let after_marker = before_colon.split("//").nth(1).unwrap_or("");
    out.push_str(after_marker.trim());
    out
}

fn leading_space(text: &str) -> String {
    leading_space_pattern()
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default()
}

/// Parse one comment, or `None` when it is not a directive.
pub fn parse(text: &str, line: i64, col: i64, offset: i64) -> Option<Directive> {
    if !is_directive(text) {
        return None;
    }
    let Some(full) = full_directive_pattern().captures(text) else {
        return Some(Directive {
            text: text.to_string(),
            line,
            col,
            offset,
            linters: Vec::new(),
            malformed: true,
        });
    };
    let linters_text = full.get(1).map(|m| m.as_str()).unwrap_or("");
    let mut linters = Vec::new();
    if !linters_text.is_empty() && !linters_text.starts_with("all") {
        for item in linters_text.split(',') {
            let name = item.trim();
            if !name.is_empty() {
                linters.push(name.to_string());
            }
        }
    }
    Some(Directive {
        text: text.to_string(),
        line,
        col,
        offset,
        linters,
        malformed: false,
    })
}

/// One nolintlint finding, and whether upstream attaches a fix to it.
#[derive(Debug, Clone)]
pub struct Message {
    pub text: String,
    /// `Some(n)`: replace the comment's first `n` bytes with `//`.
    ///
    /// Upstream builds this fix once (`removeWhitespace`) and attaches it to
    /// the leading-space finding **and to nothing else** — the malformed,
    /// not-specific and no-explanation findings all carry `Pos` alone. `n` is
    /// upstream's `len(commentMark) + len(leadingSpace)`.
    pub strip_to: Option<usize>,
}

impl Message {
    fn plain(text: String) -> Self {
        Self {
            text,
            strip_to: None,
        }
    }
}

/// The messages nolintlint produces for a directive on its own, in upstream's
/// order. The unused candidates are not here — see [`crate::nolint`].
pub fn messages(directive: &Directive, style: &NolintlintStyle) -> Vec<Message> {
    let mut out = Vec::new();
    let space = leading_space(&directive.text);
    let prefix = directive_with_optional_leading_space(&directive.text, &space);

    // NeedsMachineOnly is always set, so the "more than one leading space"
    // variant upstream keeps in the `else` branch is unreachable.
    if !space.is_empty() {
        let expected = format!(
            "//{}",
            directive.text[2..].trim_start_matches([' ', '\t', '\n', '\x0C', '\r'])
        );
        out.push(Message {
            text: format!(
                "directive `{}` should be written without leading space as `{expected}`",
                directive.text
            ),
            // `len("//") + len(leadingSpace)`.
            strip_to: Some(2 + space.len()),
        });
    }

    if directive.malformed {
        out.push(Message::plain(format!(
            "directive `{}` should match `{prefix}[:<comma-separated-linters>] [// <explanation>]`",
            directive.text
        )));
        return out;
    }

    if style.require_specific && directive.linters.is_empty() {
        out.push(Message::plain(format!(
            "directive `{}` should mention specific linter such as `{prefix}:my-linter`",
            directive.text
        )));
    }

    if style.require_explanation && !has_explanation(&directive.text) {
        let needs = directive.linters.is_empty()
            || directive
                .linters
                .iter()
                .any(|l| !style.allow_no_explanation.iter().any(|e| e == l));
        if needs {
            let without = trailing_blank_explanation().replace_all(&directive.text, "");
            out.push(Message::plain(format!(
                "directive `{}` should provide explanation such as `{without} // this is why`",
                directive.text
            )));
        }
    }

    out
}

/// `explanation == "" || strings.TrimSpace(explanation) == "//"`, where
/// `explanation` is the second capture of `fullDirectivePattern`.
fn has_explanation(text: &str) -> bool {
    let Some(full) = full_directive_pattern().captures(text) else {
        return false;
    };
    match full.get(2).map(|m| m.as_str()) {
        None | Some("") => false,
        Some(e) => e.trim() != "//",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The message texts alone: most of these tests predate `Message` and are
    /// about wording, not about which finding carries a fix.
    fn texts(d: &Directive, style: &NolintlintStyle) -> Vec<String> {
        messages(d, style).into_iter().map(|m| m.text).collect()
    }

    fn style() -> NolintlintStyle {
        NolintlintStyle::default()
    }

    #[test]
    fn plain_directive_is_clean() {
        let d = parse("//nolint:errcheck", 1, 1, 0).expect("directive");
        assert!(!d.malformed);
        assert_eq!(d.linters, vec!["errcheck".to_string()]);
        assert!(messages(&d, &style()).is_empty());
    }

    #[test]
    fn nolintfoo_is_not_a_directive() {
        assert!(parse("//nolintfoo", 1, 1, 0).is_none());
    }

    #[test]
    fn leading_space_is_reported_by_default() {
        let d = parse("// nolint:errcheck", 1, 1, 0).expect("directive");
        assert_eq!(
            texts(&d, &style()),
            vec![
                "directive `// nolint:errcheck` should be written without leading space as `//nolint:errcheck`"
                    .to_string()
            ]
        );
    }

    #[test]
    fn space_before_colon_is_malformed() {
        let d = parse("//nolint :errcheck", 1, 1, 0).expect("directive");
        assert!(d.malformed);
        assert_eq!(
            texts(&d, &style()),
            vec![
                "directive `//nolint :errcheck` should match `//nolint[:<comma-separated-linters>] [// <explanation>]`"
                    .to_string()
            ]
        );
    }

    #[test]
    fn linter_names_keep_their_case_and_all_is_dropped() {
        assert_eq!(
            parse("//nolint:ErrCheck", 1, 1, 0).expect("d").linters,
            vec!["ErrCheck".to_string()]
        );
        assert!(parse("//nolint:all", 1, 1, 0).expect("d").linters.is_empty());
        assert!(parse("//nolint", 1, 1, 0).expect("d").linters.is_empty());
    }

    #[test]
    fn require_specific_and_explanation() {
        let s = NolintlintStyle {
            require_explanation: true,
            require_specific: true,
            ..NolintlintStyle::default()
        };
        let d = parse("//nolint", 1, 1, 0).expect("d");
        assert_eq!(
            texts(&d, &s),
            vec![
                "directive `//nolint` should mention specific linter such as `//nolint:my-linter`"
                    .to_string(),
                "directive `//nolint` should provide explanation such as `//nolint // this is why`"
                    .to_string(),
            ]
        );

        let d = parse("//nolint:errcheck // because", 1, 1, 0).expect("d");
        assert!(messages(&d, &s).is_empty());
    }

    #[test]
    fn allow_no_explanation_exempts_only_listed_linters() {
        let s = NolintlintStyle {
            require_explanation: true,
            allow_no_explanation: vec!["errcheck".into()],
            ..NolintlintStyle::default()
        };
        assert!(messages(&parse("//nolint:errcheck", 1, 1, 0).expect("d"), &s).is_empty());
        assert_eq!(
            messages(&parse("//nolint:errcheck,govet", 1, 1, 0).expect("d"), &s).len(),
            1
        );
    }
}
