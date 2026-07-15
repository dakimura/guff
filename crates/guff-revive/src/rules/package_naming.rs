//! `package-naming` — enforce package naming conventions.

use guff_analysis::Pass;

use crate::failure::Failure;

const BAD_NAMES: &[&str] = &[
    "common", "interface", "interfaces", "misc", "type", "types", "util", "utils",
];

const COMMON_STD: &[(&str, &str)] = &[
    ("fmt", "fmt"),
    ("http", "net/http"),
    ("json", "encoding/json"),
    ("os", "os"),
    ("strings", "strings"),
    ("sync", "sync"),
    ("time", "time"),
];

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut failures = Vec::new();
    let file = match pass.files().first() {
        Some(f) => f,
        None => return failures,
    };
    let pkg_name = &file.name.name;
    let without_test = pkg_name.strip_suffix("_test").unwrap_or(pkg_name);

    if without_test.contains('_') {
        failures.push(Failure {
            rule: "package-naming",
            pos: file.name.name_pos.0 as u32,
            message: format!("don't use package name {pkg_name:?} that contains an underscore"),
        });
        return failures;
    }
    if has_mixed_caps(without_test) {
        failures.push(Failure {
            rule: "package-naming",
            pos: file.name.name_pos.0 as u32,
            message: format!("don't use package name {pkg_name:?} that contains MixedCaps"),
        });
        return failures;
    }

    let lower = without_test.to_ascii_lowercase();
    if BAD_NAMES.contains(&lower.as_str()) {
        failures.push(Failure {
            rule: "package-naming",
            pos: file.name.name_pos.0 as u32,
            message: format!(
                "don't use {pkg_name:?} because it is a bad package name according to https://go.dev/blog/package-names#bad-package-names"
            ),
        });
        return failures;
    }

    if let Some((_, std_path)) = COMMON_STD.iter().find(|(name, _)| *name == lower) {
        failures.push(Failure {
            rule: "package-naming",
            pos: file.name.name_pos.0 as u32,
            message: format!(
                "don't use {pkg_name:?} because it conflicts with common Go standard library package {std_path:?}"
            ),
        });
    }
    failures
}

fn has_mixed_caps(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}
