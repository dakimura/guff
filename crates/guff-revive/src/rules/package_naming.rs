//! `package-naming` — enforce package naming conventions.

use guff::walk::{self, NodeRef};
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

pub struct Checker {
    checked: bool,
    failures: Vec<Failure>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            checked: false,
            failures: Vec::new(),
        }
    }

    pub fn visit(&mut self, n: NodeRef<'_>) {
        // Original checked only pass.files().first(); package name is shared.
        if self.checked {
            return;
        }
        let NodeRef::File(file) = n else {
            return;
        };
        self.checked = true;
        check_pkg_name(&file.name.name, file.name.name_pos.0 as u32, &mut self.failures);
    }

    pub fn into_failures(self) -> Vec<Failure> {
        self.failures
    }
}

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let mut c = Checker::new();
    for file in pass.files() {
        walk::inspect(NodeRef::File(file), |n| {
            if let Some(n) = n {
                c.visit(n);
            }
            true
        });
    }
    c.into_failures()
}

fn check_pkg_name(pkg_name: &str, pos: u32, failures: &mut Vec<Failure>) {
    let without_test = pkg_name.strip_suffix("_test").unwrap_or(pkg_name);

    if without_test.contains('_') {
        failures.push(Failure {
            rule: "package-naming",
            pos,
            message: format!("don't use package name {pkg_name:?} that contains an underscore"),
            ..Failure::default()
        });
        return;
    }
    if has_mixed_caps(without_test) {
        failures.push(Failure {
            rule: "package-naming",
            pos,
            message: format!("don't use package name {pkg_name:?} that contains MixedCaps"),
            ..Failure::default()
        });
        return;
    }

    // Only the convention checks above look past the `_test` suffix; upstream
    // lowercases the *full* name for the lookups below, so an external test
    // package `util_test` is not the bad name `util`.
    let lower = pkg_name.to_ascii_lowercase();
    if BAD_NAMES.contains(&lower.as_str()) {
        failures.push(Failure {
            rule: "package-naming",
            pos,
            message: format!(
                "don't use {pkg_name:?} because it is a bad package name according to https://go.dev/blog/package-names#bad-package-names"
            ),
            ..Failure::default()
        });
        return;
    }

    if let Some((_, std_path)) = COMMON_STD.iter().find(|(name, _)| *name == lower) {
        failures.push(Failure {
            rule: "package-naming",
            pos,
            message: format!(
                "don't use {pkg_name:?} because it conflicts with common Go standard library package {std_path:?}"
            ),
            ..Failure::default()
        });
    }
}

fn has_mixed_caps(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_uppercase())
}
