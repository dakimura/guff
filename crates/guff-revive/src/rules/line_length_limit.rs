//! `line-length-limit` — restrict maximum characters per line (default 80).

use std::fs;

use guff_analysis::Pass;
use regex::Regex;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;

const DEFAULT_MAX_LINE_LENGTH: i64 = 80;

/// `Configure`: `arguments[0]` is either the limit itself or (revive v1.17.0)
/// a map with `max` and `excludes`, the latter regexps matched against the raw
/// line. Without arguments the limit is 80 and nothing is excluded.
fn configure(pass: &Pass<'_>) -> (i64, Vec<Regex>) {
    let args = config::rule_arguments(pass, "line-length-limit");
    let mut max = DEFAULT_MAX_LINE_LENGTH;
    let mut excludes = Vec::new();
    match args.first() {
        Some(RuleArgument::Integer(n)) => max = *n,
        Some(RuleArgument::Map(map)) => {
            for (k, v) in map {
                if config::is_rule_option(k, "max") {
                    if let RuleArgument::Integer(n) = v {
                        max = *n;
                    }
                } else if config::is_rule_option(k, "excludes") {
                    if let RuleArgument::List(items) = v {
                        // Upstream refuses an empty or invalid pattern as a
                        // config error; guff drops it.
                        excludes = items
                            .iter()
                            .filter_map(|i| match i {
                                RuleArgument::String(s) if !s.is_empty() => Regex::new(s).ok(),
                                _ => None,
                            })
                            .collect();
                    }
                }
            }
        }
        _ => {}
    }
    (max, excludes)
}
const TAB_WIDTH: usize = 4;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let (max, excludes) = configure(pass);
    let mut failures = Vec::new();
    let fset = pass.fset();
    let pkg = pass.pkg();

    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        let src = if let Some(bytes) = pkg.source_bytes(i) {
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_owned(),
                Err(_) => continue,
            }
        } else {
            match fs::read_to_string(path) {
                Ok(s) => s,
                Err(_) => continue,
            }
        };
        let Some(ft) = fset.file(file.pos()) else {
            continue;
        };
        let tab_spaces = " ".repeat(TAB_WIDTH);
        for (idx, raw_line) in src.lines().enumerate() {
            let line_number = idx + 1;
            if excludes.iter().any(|re| re.is_match(raw_line)) {
                continue;
            }
            let line = raw_line.replace('\t', &tab_spaces);
            let char_count = line.chars().count();
            if char_count as i64 <= max {
                continue;
            }
            if line_number == 0 || line_number > ft.line_count() {
                continue;
            }
            failures.push(Failure::at_column(
                "line-length-limit",
                ft.line_start(line_number).0 as u32,
                // Upstream reports `token.Position{Line: l, Column: 0}` —
                // the whole line is at fault, so no column is meaningful.
                0,
                format!("line is {char_count} characters, out of limit {max}"),
            ));
        }
    }
    failures
}
