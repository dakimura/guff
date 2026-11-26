//! Port of [`github.com/OpenPeeDeeP/depguard/v2`](https://github.com/OpenPeeDeeP/depguard)
//! (golangci-lint wrapper in `pkg/golinters/depguard`).
//!
//! Default (no custom rules) matches upstream: only allow `$gostd` in all files
//! under the list name `Main`.
//!
//! Settings: `linters.settings.depguard` (`rules` with `list-mode` / `files` /
//! `allow` / `deny`). Path placeholders `${base-path}` / `${config-path}` and
//! exotic globs remain DEFERRED.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

use crate::options::{DenyEntry, DepguardOptions, DepguardRule, ListMode};

fn unquote_import(path: &str) -> &str {
    path.trim_matches('"').trim_matches('`')
}

/// Top-level GOROOT/src directory names (`$gostd` expander).
fn gostd_prefixes() -> &'static [String] {
    static PREFIXES: OnceLock<Vec<String>> = OnceLock::new();
    PREFIXES.get_or_init(|| {
        let goroot = find_goroot();
        let root = Path::new(&goroot).join("src");
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&root) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    if let Some(name) = entry.file_name().to_str() {
                        out.push(name.to_string());
                    }
                }
            }
        }
        out.sort();
        out
    })
}

fn find_goroot() -> String {
    if let Ok(env) = std::env::var("GOROOT") {
        if !env.is_empty() {
            return env;
        }
    }
    if let Ok(out) = Command::new("go").args(["env", "GOROOT"]).output() {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    String::new()
}

/// Prefix-list match from depguard `strInPrefixList`.
fn prefix_match(imp: &str, prefixes: &[String]) -> Option<usize> {
    let idx = prefixes
        .binary_search_by(|p| {
            let key = p.trim_end_matches('$');
            if key > imp {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Less
            }
        })
        .unwrap_or_else(|i| i);
    if idx == 0 {
        return None;
    }
    let i = idx - 1;
    let ioc = &prefixes[i];
    if let Some(exact) = ioc.strip_suffix('$') {
        return if imp == exact { Some(i) } else { None };
    }
    if !ioc.contains('.') && !ioc.contains('/') && imp.contains('.') {
        return None;
    }
    if imp.starts_with(ioc.as_str()) {
        if imp.len() == ioc.len() || imp.as_bytes().get(ioc.len()) == Some(&b'/') {
            return Some(i);
        }
        return Some(i);
    }
    None
}

fn expand_packages(list: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in list {
        if p == "$gostd" {
            out.extend(gostd_prefixes().iter().cloned());
        } else {
            out.push(p.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn expand_deny(deny: &[DenyEntry]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in deny {
        if d.pkg == "$gostd" {
            for p in gostd_prefixes() {
                out.push((p.clone(), d.desc.clone()));
            }
        } else {
            out.push((d.pkg.clone(), d.desc.clone()));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn match_len(imp: &str, prefixes: &[String]) -> Option<usize> {
    prefix_match(imp, prefixes).map(|i| {
        let p = &prefixes[i];
        p.trim_end_matches('$').len()
    })
}

fn deny_match_len(imp: &str, deny: &[(String, String)]) -> Option<(usize, String)> {
    let pkgs: Vec<String> = deny.iter().map(|(p, _)| p.clone()).collect();
    prefix_match(imp, &pkgs).map(|i| {
        let (pkg, desc) = &deny[i];
        (pkg.trim_end_matches('$').len(), desc.clone())
    })
}

fn is_allowed(imp: &str, rule: &DepguardRule) -> (bool, Option<String>) {
    let allow = expand_packages(&rule.allow);
    let deny = expand_deny(&rule.deny);
    let allow_hit = match_len(imp, &allow);
    let deny_hit = deny_match_len(imp, &deny);

    match rule.list_mode {
        ListMode::Original => {
            let allowed = allow.is_empty() || allow_hit.is_some();
            if !allowed {
                return (false, None);
            }
            if let Some((_, desc)) = deny_hit {
                return (false, Some(desc));
            }
            (true, None)
        }
        ListMode::Strict => {
            let Some(allow_len) = allow_hit else {
                return (false, None);
            };
            if let Some((deny_len, desc)) = deny_hit {
                if allow_len > deny_len {
                    return (true, None);
                }
                return (false, Some(desc));
            }
            (true, None)
        }
        ListMode::Lax => {
            if let Some((deny_len, desc)) = &deny_hit {
                if let Some(allow_len) = allow_hit {
                    if allow_len > *deny_len {
                        return (true, None);
                    }
                }
                return (false, Some(desc.clone()));
            }
            (true, None)
        }
    }
}

/// Whether `filename` matches the rule's `files` list (AND of patterns).
///
/// Supports `$all`, `$test`, `!` negation, and simple suffix / substring globs
/// (`*_test.go`, `**/foo/*.go`). Full glob-library parity is DEFERRED.
fn file_matches(filename: &str, files: &[String]) -> bool {
    if files.is_empty() {
        return true;
    }
    let normalized = filename.replace('\\', "/");
    let base = normalized.rsplit('/').next().unwrap_or(&normalized);
    let is_test = base.ends_with("_test.go");

    for pat in files {
        let (neg, body) = match pat.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, pat.as_str()),
        };
        let matches = match body {
            "$all" => true,
            "$test" => is_test,
            other => path_glob_match(other, &normalized, base),
        };
        if neg {
            if matches {
                return false;
            }
        } else if !matches {
            return false;
        }
    }
    true
}

fn path_glob_match(pattern: &str, full: &str, base: &str) -> bool {
    if pattern == full || pattern == base {
        return true;
    }
    // `*_test.go` / `**.go` style: strip `**/` prefix and treat remaining `*` as wildcard.
    let pat = pattern.trim_start_matches("**/");
    if !pat.contains('*') {
        return full.ends_with(pat) || base == pat || full.contains(pat);
    }
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 2 {
        let (pre, suf) = (parts[0], parts[1]);
        return (full.starts_with(pre) || base.starts_with(pre) || full.contains(pre))
            && (full.ends_with(suf) || base.ends_with(suf));
    }
    // Fallback: all literal segments appear in order.
    let mut rest = full;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if let Some(idx) = rest.find(part) {
            if i == 0 && !pat.starts_with('*') && idx != 0 && !base.starts_with(part) {
                // Prefer matching from start when pattern has no leading *.
                if !base.contains(part) && !full.contains(part) {
                    return false;
                }
            }
            rest = &rest[idx + part.len()..];
        } else if base.contains(part) {
            continue;
        } else {
            return false;
        }
    }
    true
}

fn effective_rules(opts: &DepguardOptions) -> Vec<DepguardRule> {
    if opts.rules.is_empty() {
        vec![DepguardRule::default()]
    } else {
        opts.rules.clone()
    }
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _ = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "depguard requires inspect analyzer".to_string())?;

    let opts = pass
        .settings::<DepguardOptions>("depguard")
        .cloned()
        .unwrap_or_default();
    let rules = effective_rules(&opts);

    let mut pending = Vec::new();

    for file in pass.files() {
        let filename = pass.fset().position(file.pos()).filename;
        let filename = filename.replace('\\', "/");
        if !filename.ends_with(".go") && !filename.is_empty() {
            continue;
        }

        for rule in &rules {
            if !file_matches(&filename, &rule.files) {
                continue;
            }
            for imp in &file.imports {
                let path = unquote_import(&imp.path.value);
                let (ok, deny_desc) = is_allowed(path, rule);
                if !ok {
                    let mut message =
                        format!("import '{path}' is not allowed from list '{}'", rule.name);
                    if let Some(desc) = deny_desc {
                        if !desc.is_empty() {
                            message.push_str(": ");
                            message.push_str(&desc);
                        }
                    }
                    pending.push((imp.path.value_pos.0 as u32, message));
                }
            }
        }
    }

    for (pos, message) in pending {
        pass.reportf(pos, message);
    }
    Ok(None)
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(|| Analyzer {
        name: "depguard",
        doc: "Go linter that checks if package imports are in a list of acceptable packages",
        url: "https://github.com/OpenPeeDeeP/depguard",
        run: run as RunFn,
        run_despite_errors: true,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule_blocks_non_std() {
        let rule = DepguardRule::default();
        let (ok, _) = is_allowed("github.com/foo/bar", &rule);
        assert!(!ok);
        let (ok, _) = is_allowed("fmt", &rule);
        assert!(ok);
    }

    #[test]
    fn lax_deny_only() {
        let rule = DepguardRule {
            name: "Main".into(),
            list_mode: ListMode::Lax,
            files: Vec::new(),
            allow: Vec::new(),
            deny: vec![DenyEntry {
                pkg: "github.com/sirupsen/logrus".into(),
                desc: "use log/slog".into(),
            }],
        };
        let (ok, desc) = is_allowed("github.com/sirupsen/logrus", &rule);
        assert!(!ok);
        assert_eq!(desc.as_deref(), Some("use log/slog"));
        let (ok, _) = is_allowed("github.com/foo/bar", &rule);
        assert!(ok);
    }

    #[test]
    fn file_matches_test_negation() {
        assert!(file_matches("a.go", &[]));
        assert!(file_matches(
            "a.go",
            &["$all".into(), "!$test".into()]
        ));
        assert!(!file_matches(
            "a_test.go",
            &["$all".into(), "!$test".into()]
        ));
        assert!(file_matches("a_test.go", &["$test".into()]));
    }
}
