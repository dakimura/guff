//! ST1018 — avoid zero-width and control characters in string literals.
//!
//! Port of `honnef.co/go/tools/stylecheck/st1018`.

use std::sync::OnceLock;

use guff::token::Token;
use guff::walk::NodeRef;
use guff_analysis::passes::inspect;
use guff_analysis::{
    AnalysisResult, Analyzer, Diagnostic, Pass, RunError, RunFn, SuggestedFix, TextEdit,
};
use unicode_general_category::{get_general_category, GeneralCategory};

struct Invalid {
    r: char,
    /// Byte offset into the raw literal source (`BasicLit.value`).
    off: usize,
}

fn is_variation_selector(r: char) -> bool {
    matches!(r as u32, 0xFE00..=0xFE0F | 0xE0100..=0xE01EF)
}

fn is_symbol(r: char) -> bool {
    matches!(
        get_general_category(r),
        GeneralCategory::MathSymbol
            | GeneralCategory::CurrencySymbol
            | GeneralCategory::ModifierSymbol
            | GeneralCategory::OtherSymbol
    )
}

fn is_arabic_visible_format(r: char) -> bool {
    matches!(
        r as u32,
        0x0600 | 0x0601 | 0x0602 | 0x0603 | 0x0604 | 0x0605 | 0x0890 | 0x0891 | 0x08E2
    )
}

fn is_bidi_format(r: char) -> bool {
    matches!(
        r as u32,
        0x061C
            | 0x202A
            | 0x202B
            | 0x202D
            | 0x202E
            | 0x2066
            | 0x2067
            | 0x2068
            | 0x202C
            | 0x2069
    )
}

fn quote_rune_escape(r: char) -> String {
    // Mirror Go `strconv.QuoteRune` body (without surrounding quotes).
    match r {
        '\x07' => "\\a".into(),
        '\x08' => "\\b".into(),
        '\x0c' => "\\f".into(),
        '\n' => "\\n".into(),
        '\r' => "\\r".into(),
        '\t' => "\\t".into(),
        '\x0b' => "\\v".into(),
        '\\' => "\\\\".into(),
        '\'' => "\\'".into(),
        c if (c as u32) < 0x20 || c == '\x7f' => format!("\\x{:02x}", c as u32),
        c if (c as u32) < 0x10000 => format!("\\u{:04x}", c as u32),
        c => format!("\\U{:08x}", c as u32),
    }
}

fn scan_invalids(value: &str) -> (Vec<Invalid>, bool, bool) {
    let mut invalids = Vec::new();
    let mut has_format = false;
    let mut has_control = false;
    let mut prev: Option<char> = None;
    const ZWJ: char = '\u{200d}';

    for (off, r) in value.char_indices() {
        match get_general_category(r) {
            GeneralCategory::Format => {
                let u = r as u32;
                if (0xE0020..=0xE007F).contains(&u) {
                    // Flag emoji country-code tags.
                } else if is_variation_selector(r) {
                    // Always allow variation selectors.
                } else if r == ZWJ
                    && prev.is_some_and(|p| is_symbol(p) || is_variation_selector(p))
                {
                    // Allow ZWJ in emoji sequences.
                } else if is_arabic_visible_format(r) {
                    // Visible Arabic format characters.
                } else if is_bidi_format(r) {
                    invalids.push(Invalid { r, off });
                    has_format = true;
                } else {
                    invalids.push(Invalid { r, off });
                    has_format = true;
                }
            }
            GeneralCategory::Control if r != '\n' && r != '\t' && r != '\r' => {
                invalids.push(Invalid { r, off });
                has_control = true;
            }
            _ => {}
        }
        prev = Some(r);
    }
    (invalids, has_format, has_control)
}

fn kind_label(has_format: bool, has_control: bool, plural: bool) -> &'static str {
    match (has_format, has_control, plural) {
        (true, true, true) => "format and control",
        (true, true, false) => "format and control",
        (true, false, true) => "format",
        (true, false, false) => "format",
        (false, true, true) => "control",
        (false, true, false) => "control",
        _ => "format",
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "ST1018 requires inspect analyzer".to_string())?
        .clone();

    let mut pending: Vec<(u32, Diagnostic)> = Vec::new();
    {
        let files = pass.files();
        inspect.preorder(files, |n| {
            let NodeRef::BasicLit(lit) = n else {
                return;
            };
            if lit.kind != Some(Token::STRING) {
                return;
            }
            let (invalids, has_format, has_control) = scan_invalids(&lit.value);
            if invalids.is_empty() {
                return;
            }

            let base = lit.value_pos.0 as u32;
            let lit_end = lit.end().0 as u32;

            if invalids.len() == 1 {
                let inv = &invalids[0];
                let kind = kind_label(has_format, has_control, false);
                let esc = quote_rune_escape(inv.r);
                let msg = format!(
                    "string literal contains the Unicode {kind} character U+{:04X}, consider using the \"{esc}\" escape sequence instead",
                    inv.r as u32
                );
                let rune_len = inv.r.len_utf8() as u32;
                let pos = base + inv.off as u32;
                let end = pos + rune_len;
                pending.push((
                    base,
                    Diagnostic {
                        pos: base,
                        end: lit_end,
                        message: msg,
                        suggested_fixes: vec![
                            SuggestedFix {
                                message: format!(
                                    "replace {kind} character U+{:04X} with \"{esc}\"",
                                    inv.r as u32
                                ),
                                text_edits: vec![TextEdit {
                                    pos,
                                    end,
                                    new_text: esc,
                                }],
                            },
                            SuggestedFix {
                                message: format!(
                                    "delete {kind} character U+{:04X}",
                                    inv.r as u32
                                ),
                                text_edits: vec![TextEdit {
                                    pos,
                                    end,
                                    new_text: String::new(),
                                }],
                            },
                        ],
                        ..Diagnostic::default()
                    },
                ));
            } else {
                let kind = kind_label(has_format, has_control, true);
                let msg = format!(
                    "string literal contains Unicode {kind} characters, consider using escape sequences instead"
                );
                let mut edits = Vec::new();
                let mut deletions = Vec::new();
                for inv in &invalids {
                    let rune_len = inv.r.len_utf8() as u32;
                    let pos = base + inv.off as u32;
                    let end = pos + rune_len;
                    edits.push(TextEdit {
                        pos,
                        end,
                        new_text: quote_rune_escape(inv.r),
                    });
                    deletions.push(TextEdit {
                        pos,
                        end,
                        new_text: String::new(),
                    });
                }
                pending.push((
                    base,
                    Diagnostic {
                        pos: base,
                        end: lit_end,
                        message: msg,
                        suggested_fixes: vec![
                            SuggestedFix {
                                message: format!(
                                    "replace all {kind} characters with escape sequences"
                                ),
                                text_edits: edits,
                            },
                            SuggestedFix {
                                message: format!("delete all {kind} characters"),
                                text_edits: deletions,
                            },
                        ],
                        ..Diagnostic::default()
                    },
                ));
            }
        });
    }

    for (_, diag) in pending {
        pass.report(diag);
    }
    Ok(None)
}

fn st1018_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "ST1018",
        doc: "avoid zero-width and control characters in string literals",
        url: "https://staticcheck.dev/docs/checks/#ST1018",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(st1018_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn st1018_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    #[test]
    fn detects_bel_and_zwsp() {
        let (inv, has_f, has_c) = scan_invalids("\"\u{0007}\"");
        assert_eq!(inv.len(), 1);
        assert!(has_c && !has_f);
        let (inv, has_f, has_c) = scan_invalids("`Zero\u{200b}Width`");
        assert_eq!(inv.len(), 1);
        assert!(has_f && !has_c);
    }
}
