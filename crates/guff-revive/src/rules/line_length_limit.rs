//! `line-length-limit` — restrict maximum characters per line (default 80).

use std::fs;

use guff_analysis::Pass;

use crate::failure::Failure;

const DEFAULT_MAX_LINE_LENGTH: i64 = 80;

/// `Configure`: `arguments[0]` is the limit, 80 when there is none.
fn max_line_length(pass: &Pass<'_>) -> i64 {
    crate::config::rule_arg_int(pass, "line-length-limit", 0).unwrap_or(DEFAULT_MAX_LINE_LENGTH)
}
const TAB_WIDTH: usize = 4;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let max = max_line_length(pass);
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
