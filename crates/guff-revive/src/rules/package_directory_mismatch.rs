//! `package-directory-mismatch` — package name should match the containing directory.

use std::fs;
use std::path::Path;

use guff_analysis::Pass;

use crate::config;
use crate::failure::Failure;
use crate::settings::RuleArgument;

pub fn apply(pass: &Pass<'_>) -> Vec<Failure> {
    let ignored = ignored_directories(pass);
    let mut failures = Vec::new();
    let pkg = pass.pkg();
    if pkg.name == "main" {
        return failures;
    }

    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        let dir_path = path.parent().unwrap_or(Path::new("."));
        let dir_name = dir_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if dir_name.is_empty() || dir_name == "." || dir_name == "/" {
            continue;
        }
        if dir_name == "internal" {
            continue;
        }
        let dir_path_str = dir_path.to_string_lossy();
        if ignored.iter().any(|part| dir_path_str.contains(part)) {
            continue;
        }
        if is_root_dir(dir_path) {
            continue;
        }

        let package_name = &file.name.name;
        if semantically_equal(package_name, dir_name) {
            continue;
        }

        let is_test = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|stem| stem.ends_with("_test"));

        if is_test {
            if package_name == "main_test" {
                continue;
            }
            if semantically_equal(package_name, &format!("{dir_name}_test")) {
                continue;
            }
        }

        let mut message = format!(
            "package name \"{package_name}\" does not match directory name \"{dir_name}\""
        );

        if is_version_path(dir_name) {
            let parent = dir_path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if semantically_equal(package_name, parent) {
                continue;
            }
            if is_test && semantically_equal(package_name, &format!("{parent}_test")) {
                continue;
            }
            message = format!(
                "package name \"{package_name}\" does not match directory name \"{dir_name}\" or parent directory name \"{parent}\""
            );
        }

        failures.push(Failure {
            rule: "package-directory-mismatch",
            pos: file.name.name_pos.0 as u32,
            message,
            ..Failure::default()
        });
    }
    failures
}

fn ignored_directories(pass: &Pass<'_>) -> Vec<String> {
    if let Some(map) = config::rule_arg_map(pass, "package-directory-mismatch", 0) {
        for (key, value) in map {
            if !key.eq_ignore_ascii_case("ignore-directories")
                && !key.eq_ignore_ascii_case("ignoreDirectories")
            {
                continue;
            }
            if let RuleArgument::List(items) = value {
                return items
                    .iter()
                    .filter_map(|item| match item {
                        RuleArgument::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .collect();
            }
        }
        return Vec::new();
    }
    vec!["testdata".into()]
}

fn semantically_equal(package_name: &str, dir_name: &str) -> bool {
    let norm_pkg = normalize_path(package_name);
    let norm_dir = normalize_path(dir_name);
    norm_dir == norm_pkg || norm_dir == format!("go{norm_pkg}")
}

fn normalize_path(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '-' && *c != '_' && *c != '.')
        .collect()
}

fn is_version_path(name: &str) -> bool {
    let Some(rest) = name.strip_prefix('v').or_else(|| name.strip_prefix('V')) else {
        return false;
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn is_root_dir(dir_path: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir_path) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "go.mod" || name == ".git" {
            return true;
        }
    }
    false
}
