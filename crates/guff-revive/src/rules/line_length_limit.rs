//! `line-length-limit` — restrict maximum characters per line (default 80).

use std::fs;

use guff_analysis::Pass;

use crate::failure::Failure;

const MAX_LINE_LENGTH: usize = 80;
const TAB_WIDTH: usize = 4;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
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
            if char_count <= MAX_LINE_LENGTH {
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
                format!("line is {char_count} characters, out of limit {MAX_LINE_LENGTH}"),
            ));
        }
    }
    failures
}
