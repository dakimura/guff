//! `file-length-limit` — enforce a maximum number of lines per file.

use std::fs;

use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let max = config::file_length_limit_max(pass);
    if max == 0 {
        return Vec::new();
    }

    let mut failures = Vec::new();
    let pkg = pass.pkg();
    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        let lines = if let Some(bytes) = pkg.source_bytes(i) {
            match std::str::from_utf8(bytes) {
                Ok(s) => s.lines().count(),
                Err(_) => continue,
            }
        } else {
            match fs::read_to_string(path) {
                Ok(s) => s.lines().count(),
                Err(_) => continue,
            }
        };
        if lines > max {
            failures.push(Failure {
                rule: "file-length-limit",
                pos: file.package.0 as u32,
                message: format!(
                    "file length is {lines} lines, which exceeds the limit of {max}"
                ),
            confidence: None,
        });
        }
    }
    failures
}
