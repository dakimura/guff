//! Differential tests for `gostd::url` and `gostd::strconv` against Go itself.
//!
//! `testdata/gostd/url_parse.tsv` and `testdata/gostd/quote.tsv` are generated
//! by `compat/oracles/{gourl,goquote}`, which run the real standard library.
//! Nothing here is hand-written.
//!
//! Regenerate with `compat/oracles/regen.sh goquote gourl`.

use guff_staticcheck::gostd;

const URL_GROUND_TRUTH: &str = include_str!("testdata/gostd/url_parse.tsv");
const QUOTE_GROUND_TRUTH: &str = include_str!("testdata/gostd/quote.tsv");

fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0, "odd-length hex field: {s:?}");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex field"))
        .collect()
}

#[test]
fn parse_matches_go_on_every_corpus_url() {
    let mut rows = 0usize;
    let mut errors = 0usize;
    let mut mismatches = Vec::new();

    for (lineno, line) in URL_GROUND_TRUTH.lines().enumerate() {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.splitn(3, '\t');
        let display = fields.next().expect("quoted input");
        let hex = fields
            .next()
            .unwrap_or_else(|| panic!("line {}: no hex field", lineno + 1));
        let want = fields
            .next()
            .unwrap_or_else(|| panic!("line {}: no error field", lineno + 1));

        let raw = String::from_utf8(unhex(hex)).expect("corpus inputs are UTF-8");
        rows += 1;
        if !want.is_empty() {
            errors += 1;
        }

        let got = gostd::url::parse(&raw).err().unwrap_or_default();
        if got != want {
            mismatches.push(format!(
                "  input {display}\n    go:   {want:?}\n    guff: {got:?}"
            ));
        }
    }

    assert!(rows > 6000, "ground truth looks truncated: {rows} rows");
    assert!(
        errors > 2000,
        "ground truth has too few error rows to be a real differential: {errors}"
    );
    assert!(
        mismatches.is_empty(),
        "{} of {rows} URLs disagree with Go:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// `strconv.IsPrint` decides every `\u` escape in every message above, so it is
/// checked over the whole rune space rather than sampled.
#[test]
fn is_print_matches_go_for_every_rune() {
    let mut printable = vec![false; 0x11_0000];
    let mut ranges = 0usize;
    let mut quote_cases = 0usize;
    let mut mismatches = Vec::new();

    for line in QUOTE_GROUND_TRUTH.lines() {
        let mut fields = line.splitn(3, '\t');
        match fields.next() {
            Some("print") => {
                let lo = u32::from_str_radix(fields.next().expect("lo"), 16).expect("lo hex");
                let hi = u32::from_str_radix(fields.next().expect("hi"), 16).expect("hi hex");
                for c in lo..=hi {
                    printable[c as usize] = true;
                }
                ranges += 1;
            }
            Some("quote") => {
                let raw = unhex(fields.next().expect("hex"));
                let want = fields.next().expect("quoted");
                let got = gostd::strconv::quote_bytes(&raw);
                if got != want {
                    mismatches.push(format!("  quote({raw:?})\n    go:   {want}\n    guff: {got}"));
                }
                quote_cases += 1;
            }
            other => panic!("unexpected section {other:?}"),
        }
    }

    assert!(ranges > 100, "print ranges look truncated: {ranges}");
    assert!(quote_cases > 20, "quote cases look truncated: {quote_cases}");

    for n in 0..0x11_0000u32 {
        let Some(c) = char::from_u32(n) else {
            continue; // surrogate: not a Rust char, and Go says unprintable
        };
        if gostd::strconv::is_print(c) != printable[n as usize] {
            mismatches.push(format!(
                "  is_print(U+{n:04X}) go={} guff={}",
                printable[n as usize],
                gostd::strconv::is_print(c)
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "{} disagreements with Go:\n{}",
        mismatches.len(),
        mismatches
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
