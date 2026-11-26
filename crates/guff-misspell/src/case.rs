//! Word casing helpers (port of `misspell/case.go`).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseStyle {
    Lower,
    Upper,
    Title,
    Unknown,
}

/// Returns what case style a word is in.
pub fn case_style(word: &str) -> CaseStyle {
    let mut upper_count = 0usize;
    let mut lower_count = 0usize;

    for ch in word.bytes() {
        match ch {
            b'a'..=b'z' => lower_count += 1,
            b'A'..=b'Z' => upper_count += 1,
            _ => {}
        }
    }

    match (upper_count, lower_count) {
        (u, 0) if u > 0 => CaseStyle::Upper,
        (0, l) if l > 0 => CaseStyle::Lower,
        (1, l) if l > 0 => {
            if word
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_uppercase())
            {
                CaseStyle::Title
            } else {
                CaseStyle::Unknown
            }
        }
        _ => CaseStyle::Unknown,
    }
}

/// Apply `corrected_lower` using the casing style of `original`.
pub fn apply_case(corrected_lower: &str, style: CaseStyle) -> String {
    match style {
        CaseStyle::Lower => corrected_lower.to_string(),
        CaseStyle::Upper => corrected_lower.to_ascii_uppercase(),
        CaseStyle::Title => {
            let mut out = corrected_lower.to_string();
            if let Some(first) = out.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            out
        }
        CaseStyle::Unknown => corrected_lower.to_string(),
    }
}
