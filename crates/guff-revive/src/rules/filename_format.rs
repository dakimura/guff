//! `filename-format` — enforce source filename conventions.

use std::path::Path;
use std::sync::OnceLock;

use guff_analysis::Pass;
use regex::Regex;

use crate::failure::Failure;

fn default_format() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[_A-Za-z0-9][_A-Za-z0-9-]*\.go$").expect("valid regex"))
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let format = default_format();
    let mut failures = Vec::new();
    let pkg = pass.pkg();
    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown.go");
        if format.is_match(filename) {
            continue;
        }
        let extra = non_ascii_message(filename);
        failures.push(Failure {
            rule: "filename-format",
            pos: file.name.name_pos.0 as u32,
            message: format!(
                "Filename {filename} is not of the format {}.{extra}",
                format.as_str()
            ),
            confidence: None,
        });
    }
    failures
}

fn non_ascii_message(filename: &str) -> String {
    let mut out = String::new();
    for ch in filename.chars() {
        if ch.is_ascii() {
            continue;
        }
        out.push_str(&format!(" Non ASCII character {ch} ({:#x}) found.", ch as u32));
    }
    out
}

#[allow(unused_imports)]
use std::path::Path as _;
