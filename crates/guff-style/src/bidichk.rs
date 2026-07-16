//! Port of [`github.com/breml/bidichk`](https://github.com/breml/bidichk).
//!
//! Scans raw Go source bytes for dangerous Unicode bidirectional formatting
//! characters (Trojan Source). Unlike AST-based linters, this inspects comments
//! and string literals too.

use std::collections::HashMap;
use std::fs;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::BidichkOptions;

const RUNE_LRE: char = '\u{202A}';
const RUNE_RLE: char = '\u{202B}';
const RUNE_PDF: char = '\u{202C}';
const RUNE_LRO: char = '\u{202D}';
const RUNE_RLO: char = '\u{202E}';
const RUNE_LRI: char = '\u{2066}';
const RUNE_RLI: char = '\u{2067}';
const RUNE_FSI: char = '\u{2068}';
const RUNE_PDI: char = '\u{2069}';

const RUNE_NAME_LRE: &str = "LEFT-TO-RIGHT-EMBEDDING";
const RUNE_NAME_RLE: &str = "RIGHT-TO-LEFT-EMBEDDING";
const RUNE_NAME_PDF: &str = "POP-DIRECTIONAL-FORMATTING";
const RUNE_NAME_LRO: &str = "LEFT-TO-RIGHT-OVERRIDE";
const RUNE_NAME_RLO: &str = "RIGHT-TO-LEFT-OVERRIDE";
const RUNE_NAME_LRI: &str = "LEFT-TO-RIGHT-ISOLATE";
const RUNE_NAME_RLI: &str = "RIGHT-TO-LEFT-ISOLATE";
const RUNE_NAME_FSI: &str = "FIRST-STRONG-ISOLATE";
const RUNE_NAME_PDI: &str = "POP-DIRECTIONAL-ISOLATE";

fn default_disallowed() -> HashMap<&'static str, char> {
    HashMap::from([
        (RUNE_NAME_LRE, RUNE_LRE),
        (RUNE_NAME_RLE, RUNE_RLE),
        (RUNE_NAME_PDF, RUNE_PDF),
        (RUNE_NAME_LRO, RUNE_LRO),
        (RUNE_NAME_RLO, RUNE_RLO),
        (RUNE_NAME_LRI, RUNE_LRI),
        (RUNE_NAME_RLI, RUNE_RLI),
        (RUNE_NAME_FSI, RUNE_FSI),
        (RUNE_NAME_PDI, RUNE_PDI),
    ])
}

fn resolve_disallowed(opts: &BidichkOptions) -> HashMap<&'static str, char> {
    if opts.disallowed_runes.is_empty() {
        return default_disallowed();
    }
    let defaults = default_disallowed();
    let mut out = HashMap::new();
    for name in &opts.disallowed_runes {
        if let Some((&key, &r)) = defaults.iter().find(|(k, _)| *k == name) {
            out.insert(key, r);
        }
    }
    out
}

fn check_body(body: &[u8], base_pos: u32, disallowed: &HashMap<&str, char>, pending: &mut Vec<(u32, String)>) {
    let body_str = match std::str::from_utf8(body) {
        Ok(s) => s,
        Err(_) => return,
    };
    for (&name, &r) in disallowed {
        let mut start = 0usize;
        while let Some(idx) = body_str[start..].find(r) {
            let abs = start + idx;
            pending.push((
                base_pos + abs as u32,
                format!("found dangerous unicode character sequence {name}"),
            ));
            start = abs + r.len_utf8();
        }
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "bidichk requires inspect analyzer".to_string())?;

    let options = pass
        .settings::<BidichkOptions>("bidichk")
        .cloned()
        .unwrap_or_default();
    let disallowed = resolve_disallowed(&options);

    let mut pending = Vec::new();
    let paths = pass.pkg().compiled_go_files.clone();
    let files = pass.files();

    for (i, file) in files.iter().enumerate() {
        let Some(path) = paths.get(i) else {
            continue;
        };
        let Ok(body) = fs::read(path) else {
            continue;
        };
        let base_pos = file.file_start.0 as u32;
        check_body(&body, base_pos, &disallowed, &mut pending);
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "bidichk",
        doc: "Checks for dangerous unicode character sequences",
        url: "https://github.com/breml/bidichk",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_includes_rlo() {
        let opts = BidichkOptions::default();
        let d = resolve_disallowed(&opts);
        assert!(d.contains_key(RUNE_NAME_RLO));
    }

    #[test]
    fn partial_settings_limit_runes() {
        let opts = BidichkOptions {
            disallowed_runes: vec![RUNE_NAME_LRO.into()],
        };
        let d = resolve_disallowed(&opts);
        assert_eq!(d.len(), 1);
        assert!(d.contains_key(RUNE_NAME_LRO));
        assert!(!d.contains_key(RUNE_NAME_RLO));
    }
}
