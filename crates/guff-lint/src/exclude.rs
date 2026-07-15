//! Post-processing filters for lint issues (golangci `result/processors`).
//!
//! Pipeline order (subset of golangci-lint):
//! path (dirs/files) → text exclude → exclude-rules (+ default excludes) →
//! nolint → max-per-linter → max-same → severity (+ unused nolintlint).

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::position::FileSet;
use guff_analysis::Diagnostic;
use guff_packages::Package;
use regex::Regex;

use crate::config::{ExcludeRule, IssuesConfig, SeverityConfig};
use crate::nolint::NolintIndex;
use crate::registry::linter_name_for_analyzer;

/// Default directory patterns used when `exclude-dirs-use-default` is true
/// (golangci `StdExcludeDirRegexps`).
pub const DEFAULT_EXCLUDE_DIRS: &[&str] = &[
    r"(^|[/\\])vendor([/\\]|$)",
    r"(^|[/\\])third_party([/\\]|$)",
    r"(^|[/\\])testdata([/\\]|$)",
    r"(^|[/\\])examples([/\\]|$)",
    r"(^|[/\\])Godeps([/\\]|$)",
    r"(^|[/\\])builtin([/\\]|$)",
];

/// One of golangci's built-in default exclude patterns (`EXC0001`…`EXC0015`).
#[derive(Debug, Clone, Copy)]
pub struct DefaultExcludePattern {
    pub id: &'static str,
    pub pattern: &'static str,
    pub linter: &'static str,
}

/// golangci-lint default exclude patterns for `exclude-use-default: true`.
pub fn default_exclude_patterns() -> &'static [DefaultExcludePattern] {
    static PATTERNS: &[DefaultExcludePattern] = &[
        DefaultExcludePattern {
            id: "EXC0001",
            pattern: r"Error return value of .((os\.)?std(out|err)\..*|.*Close|.*Flush|os\.Remove(All)?|.*print(f|ln)?|os\.(Un)?Setenv). is not checked",
            linter: "errcheck",
        },
        DefaultExcludePattern {
            id: "EXC0002",
            pattern: r"(comment on exported (method|function|type|const)|should have( a package)? comment|comment should be of the form)",
            linter: "golint",
        },
        DefaultExcludePattern {
            id: "EXC0003",
            pattern: r"func name will be used as test\.Test.* by other packages, and that stutters; consider calling this",
            linter: "golint",
        },
        DefaultExcludePattern {
            id: "EXC0004",
            pattern: r"(possible misuse of unsafe.Pointer|should have signature)",
            linter: "govet",
        },
        DefaultExcludePattern {
            id: "EXC0005",
            pattern: "SA4011",
            linter: "staticcheck",
        },
        DefaultExcludePattern {
            id: "EXC0006",
            pattern: "G103: Use of unsafe calls should be audited",
            linter: "gosec",
        },
        DefaultExcludePattern {
            id: "EXC0007",
            pattern: "G204: Subprocess launched with variable",
            linter: "gosec",
        },
        DefaultExcludePattern {
            id: "EXC0008",
            pattern: "G104",
            linter: "gosec",
        },
        DefaultExcludePattern {
            id: "EXC0009",
            pattern: r"(G301|G302|G307): Expect (directory permissions to be 0750|file permissions to be 0600) or less",
            linter: "gosec",
        },
        DefaultExcludePattern {
            id: "EXC0010",
            pattern: "G304: Potential file inclusion via variable",
            linter: "gosec",
        },
        DefaultExcludePattern {
            id: "EXC0011",
            pattern: r"(ST1000|ST1020|ST1021|ST1022)",
            linter: "stylecheck",
        },
        DefaultExcludePattern {
            id: "EXC0012",
            pattern: r"exported (.+) should have comment( \(or a comment on this block\))? or be unexported",
            linter: "revive",
        },
        DefaultExcludePattern {
            id: "EXC0013",
            pattern: r#"package comment should be of the form "(.+)...""#,
            linter: "revive",
        },
        DefaultExcludePattern {
            id: "EXC0014",
            pattern: r#"comment on exported (.+) should be of the form "(.+)...""#,
            linter: "revive",
        },
        DefaultExcludePattern {
            id: "EXC0015",
            pattern: "should have a package comment",
            linter: "revive",
        },
    ];
    PATTERNS
}

/// A normalized issue for filtering and output.
#[derive(Debug, Clone)]
pub struct Issue {
    /// golangci linter name (`errcheck`, `staticcheck`, `govet`, …).
    pub from_linter: String,
    /// Analyzer pass name used in text output (`errcheck`, `SA1004`, `printf`, …).
    pub analyzer: String,
    pub text: String,
    pub severity: String,
    pub filename: String,
    pub line: i64,
    pub column: i64,
    pub source_line: Option<String>,
    pub diagnostic: Diagnostic,
}

/// Compiled issue filter built from `issues` + `severity` config.
#[derive(Debug, Default, Clone)]
pub struct IssueFilter {
    exclude_dir_res: Vec<Regex>,
    exclude_file_res: Vec<Regex>,
    exclude_text_res: Vec<Regex>,
    exclude_rules: Vec<CompiledRule>,
    max_issues_per_linter: i32,
    max_same_issues: i32,
    uniq_by_line: bool,
    default_severity: Option<String>,
    severity_rules: Vec<(CompiledRule, String)>,
    /// When true, unused `//nolint` directives are reported as `nolintlint`.
    pub report_unused_nolint: bool,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    linters: Vec<String>,
    path: Option<Regex>,
    path_except: Option<Regex>,
    text: Option<Regex>,
    source: Option<Regex>,
}

impl IssueFilter {
    /// Build a filter from config. Invalid regexes are skipped (logged to stderr).
    pub fn from_config(issues: &IssuesConfig, severity: &SeverityConfig) -> Self {
        let case_prefix = if issues.exclude_case_sensitive {
            ""
        } else {
            "(?i)"
        };

        let mut filter = Self {
            max_issues_per_linter: issues.max_issues_per_linter,
            max_same_issues: issues.max_same_issues,
            uniq_by_line: issues.uniq_by_line.unwrap_or(true),
            default_severity: severity.default_severity.clone(),
            ..Self::default()
        };

        let use_default_dirs = issues.exclude_dirs_use_default.unwrap_or(true);
        if use_default_dirs {
            for p in DEFAULT_EXCLUDE_DIRS {
                push_re(&mut filter.exclude_dir_res, p, "exclude-dirs-use-default");
            }
        }
        for p in &issues.exclude_dirs {
            push_re(&mut filter.exclude_dir_res, p, "exclude-dirs");
        }
        for p in &issues.exclude_files {
            push_re(&mut filter.exclude_file_res, &normalize_path_regex(p), "exclude-files");
        }
        for p in &issues.exclude {
            let pat = format!("{case_prefix}{p}");
            push_re(&mut filter.exclude_text_res, &pat, "exclude");
        }

        let mut rules = issues.exclude_rules.clone();
        if issues.exclude_use_default {
            let include: std::collections::HashSet<&str> =
                issues.include.iter().map(|s| s.as_str()).collect();
            for pat in default_exclude_patterns() {
                if include.contains(pat.id) {
                    continue;
                }
                rules.push(ExcludeRule {
                    linters: vec![pat.linter.to_string()],
                    text: Some(pat.pattern.to_string()),
                    ..ExcludeRule::default()
                });
            }
        }

        for rule in &rules {
            match compile_rule(rule, case_prefix) {
                Ok(c) => filter.exclude_rules.push(c),
                Err(e) => eprintln!("guff: skipping exclude-rule: {e}"),
            }
        }

        let sev_prefix = if severity.case_sensitive {
            ""
        } else {
            "(?i)"
        };
        for rule in &severity.rules {
            let base = ExcludeRule {
                linters: rule.linters.clone(),
                path: rule.path.clone(),
                path_except: rule.path_except.clone(),
                text: rule.text.clone(),
                source: rule.source.clone(),
            };
            match compile_rule(&base, sev_prefix) {
                Ok(c) => filter.severity_rules.push((c, rule.severity.clone())),
                Err(e) => eprintln!("guff: skipping severity rule: {e}"),
            }
        }

        // DEFERRED: issues.new / new-from-rev diff filtering (needs git integration).
        let _ = (&issues.new, &issues.new_from_rev);

        filter
    }

    /// Convert runner diagnostics into [`Issue`]s (before filtering).
    pub fn collect_issues(
        fset: &FileSet,
        diagnostics: &[(String, Diagnostic)],
    ) -> Vec<Issue> {
        let mut out = Vec::with_capacity(diagnostics.len());
        for (action_id, diag) in diagnostics {
            let analyzer = action_id
                .split('@')
                .next()
                .unwrap_or(action_id)
                .to_string();
            let from_linter = linter_name_for_analyzer(&analyzer).to_string();
            let (filename, line, column) = if diag.pos != 0 {
                let pos = fset.position(guff::Pos(diag.pos as i64));
                (pos.filename, pos.line, pos.column)
            } else {
                (String::new(), 0, 0)
            };
            let source_line = read_source_line(&filename, line);
            out.push(Issue {
                from_linter,
                analyzer,
                text: diag.message.clone(),
                severity: diag.severity.clone(),
                filename,
                line,
                column,
                source_line,
                diagnostic: diag.clone(),
            });
        }
        out
    }

    /// Apply the post-processing pipeline. Returns filtered issues (possibly
    /// with severity assigned).
    ///
    /// `packages` supplies source paths for `//nolint` indexing.
    pub fn apply(&self, mut issues: Vec<Issue>, packages: &[Arc<Package>]) -> Vec<Issue> {
        issues.retain(|issue| !self.is_excluded_by_path(issue));
        issues.retain(|issue| {
            !self
                .exclude_text_res
                .iter()
                .any(|re| re.is_match(&issue.text))
        });
        issues.retain(|issue| {
            !self
                .exclude_rules
                .iter()
                .any(|rule| rule.matches(issue))
        });

        if !packages.is_empty() {
            let mut nolint = NolintIndex::from_packages(packages);
            issues = nolint.filter_issues(issues, self.report_unused_nolint);
        }

        if self.uniq_by_line {
            let mut seen = std::collections::HashSet::new();
            issues.retain(|issue| {
                let key = (
                    issue.filename.clone(),
                    issue.line,
                    issue.from_linter.clone(),
                );
                seen.insert(key)
            });
        }

        if self.max_issues_per_linter > 0 {
            let mut counts: HashMap<String, i32> = HashMap::new();
            issues.retain(|issue| {
                let n = counts.entry(issue.from_linter.clone()).or_insert(0);
                *n += 1;
                *n <= self.max_issues_per_linter
            });
        }
        if self.max_same_issues > 0 {
            let mut counts: HashMap<String, i32> = HashMap::new();
            issues.retain(|issue| {
                let n = counts.entry(issue.text.clone()).or_insert(0);
                *n += 1;
                *n <= self.max_same_issues
            });
        }

        for issue in &mut issues {
            self.assign_severity(issue);
        }

        issues
    }

    fn is_excluded_by_path(&self, issue: &Issue) -> bool {
        let path = issue.filename.replace('\\', "/");
        let parent = Path::new(&path)
            .parent()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        for re in &self.exclude_dir_res {
            if re.is_match(&parent) || re.is_match(&path) {
                return true;
            }
        }
        for re in &self.exclude_file_res {
            if re.is_match(&path) {
                return true;
            }
        }
        false
    }

    fn assign_severity(&self, issue: &mut Issue) {
        for (rule, sev) in &self.severity_rules {
            if rule.matches(issue) {
                if sev != "@linter" {
                    issue.severity = sev.clone();
                }
                return;
            }
        }
        if let Some(default) = &self.default_severity {
            if default != "@linter" {
                issue.severity = default.clone();
            }
        }
    }
}

impl CompiledRule {
    fn matches(&self, issue: &Issue) -> bool {
        let empty = self.linters.is_empty()
            && self.path.is_none()
            && self.path_except.is_none()
            && self.text.is_none()
            && self.source.is_none();
        if empty {
            return false;
        }
        if let Some(re) = &self.text {
            if !re.is_match(&issue.text) {
                return false;
            }
        }
        let path = issue.filename.replace('\\', "/");
        if let Some(re) = &self.path {
            if !re.is_match(&path) {
                return false;
            }
        }
        if let Some(re) = &self.path_except {
            if re.is_match(&path) {
                return false;
            }
        }
        if !self.linters.is_empty() {
            let ok = self.linters.iter().any(|l| {
                l == &issue.from_linter || l == &issue.analyzer
            });
            if !ok {
                return false;
            }
        }
        if let Some(re) = &self.source {
            match &issue.source_line {
                Some(line) if re.is_match(line) => {}
                _ => return false,
            }
        }
        true
    }
}

fn compile_rule(rule: &ExcludeRule, case_prefix: &str) -> Result<CompiledRule, String> {
    let path = match rule.path.as_deref() {
        Some(p) if !p.is_empty() => {
            let normalized = normalize_path_regex(p);
            optional_re(Some(&normalized), "")?
        }
        _ => None,
    };
    let path_except = match rule.path_except.as_deref() {
        Some(p) if !p.is_empty() => {
            let normalized = normalize_path_regex(p);
            optional_re(Some(&normalized), "")?
        }
        _ => None,
    };
    Ok(CompiledRule {
        linters: rule.linters.clone(),
        path,
        path_except,
        text: optional_re(rule.text.as_deref(), case_prefix)?,
        source: optional_re(rule.source.as_deref(), case_prefix)?,
    })
}

fn optional_re(pat: Option<&str>, prefix: &str) -> Result<Option<Regex>, String> {
    match pat {
        None | Some("") => Ok(None),
        Some(p) => {
            let full = format!("{prefix}{p}");
            Regex::new(&full)
                .map(Some)
                .map_err(|e| format!("invalid regex `{full}`: {e}"))
        }
    }
}

fn push_re(out: &mut Vec<Regex>, pat: &str, what: &str) {
    match Regex::new(pat) {
        Ok(re) => out.push(re),
        Err(e) => eprintln!("guff: invalid {what} regex `{pat}`: {e}"),
    }
}

/// Replace `/` with a class that matches either separator (golangci
/// `NormalizePathInRegex` subset).
fn normalize_path_regex(pat: &str) -> String {
    pat.replace('/', r"[/\\]")
}

fn read_source_line(filename: &str, line: i64) -> Option<String> {
    if filename.is_empty() || line <= 0 {
        return None;
    }
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Vec<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut guard = cache.lock().ok()?;
    let lines = guard.entry(filename.to_string()).or_insert_with(|| {
        std::fs::read_to_string(filename)
            .map(|s| s.lines().map(|l| l.to_string()).collect())
            .unwrap_or_default()
    });
    let idx = (line as usize).checked_sub(1)?;
    lines.get(idx).cloned()
}

/// Build an [`Issue`] from a cached diagnostic's already-resolved position
/// (filename/line/column stored in the issues cache), without a `FileSet`.
/// The lazy warm path uses this so cache hits never need parsing/type-checking.
#[allow(clippy::too_many_arguments)]
pub fn issue_from_cached(
    analyzer: &str,
    filename: &str,
    line: i64,
    column: i64,
    message: &str,
    category: &str,
    url: &str,
    severity: &str,
) -> Issue {
    let source_line = read_source_line(filename, line);
    Issue {
        from_linter: linter_name_for_analyzer(analyzer).to_string(),
        analyzer: analyzer.to_string(),
        text: message.to_string(),
        severity: severity.to_string(),
        filename: filename.to_string(),
        line,
        column,
        source_line,
        diagnostic: Diagnostic {
            message: message.to_string(),
            category: category.to_string(),
            url: url.to_string(),
            severity: severity.to_string(),
            ..Diagnostic::default()
        },
    }
}

/// Convenience: build issues from diagnostics and apply `filter`.
pub fn process_diagnostics(
    fset: &FileSet,
    diagnostics: &[(String, Diagnostic)],
    filter: &IssueFilter,
    packages: &[Arc<Package>],
) -> Vec<Issue> {
    filter.apply(IssueFilter::collect_issues(fset, diagnostics), packages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(linter: &str, file: &str, text: &str) -> Issue {
        Issue {
            from_linter: linter.into(),
            analyzer: linter.into(),
            text: text.into(),
            severity: String::new(),
            filename: file.into(),
            line: 1,
            column: 1,
            source_line: None,
            diagnostic: Diagnostic {
                message: text.into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn exclude_rules_by_path_and_linter() {
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude_rules: vec![ExcludeRule {
                linters: vec!["errcheck".into()],
                path: Some(r"bad\.go".into()),
                ..ExcludeRule::default()
            }],
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue("errcheck", "/tmp/pkg/bad.go", "unchecked error"),
                issue("errcheck", "/tmp/pkg/ok.go", "unchecked error"),
                issue("govet", "/tmp/pkg/bad.go", "something"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|i| i.filename.ends_with("ok.go")));
        assert!(kept.iter().any(|i| i.from_linter == "govet"));
    }

    #[test]
    fn exclude_text_pattern() {
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude: vec!["hidden".into()],
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue("errcheck", "a.go", "this is hidden"),
                issue("errcheck", "a.go", "visible"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].text, "visible");
    }

    #[test]
    fn max_issues_per_linter() {
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 1,
            max_same_issues: 0,
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue("errcheck", "a.go", "one"),
                issue("errcheck", "a.go", "two"),
                issue("govet", "a.go", "three"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].text, "one");
        assert_eq!(kept[1].from_linter, "govet");
    }

    #[test]
    fn severity_default_applied() {
        let severity = SeverityConfig {
            default_severity: Some("warning".into()),
            ..SeverityConfig::default()
        };
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &severity);
        let kept = filter.apply(vec![issue("errcheck", "a.go", "x")], &[]);
        assert_eq!(kept[0].severity, "warning");
    }

    #[test]
    fn severity_rule_overrides_default() {
        use crate::config::SeverityRule;
        let severity = SeverityConfig {
            default_severity: Some("warning".into()),
            rules: vec![SeverityRule {
                linters: vec!["errcheck".into()],
                severity: "error".into(),
                ..SeverityRule::default()
            }],
            ..SeverityConfig::default()
        };
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &severity);
        let kept = filter.apply(
            vec![
                issue("errcheck", "a.go", "x"),
                issue("govet", "a.go", "y"),
            ],
            &[],
        );
        assert_eq!(kept[0].severity, "error");
        assert_eq!(kept[1].severity, "warning");
    }

    #[test]
    fn severity_at_linter_keeps_revive_rule_severity() {
        let severity = SeverityConfig {
            default_severity: Some("@linter".into()),
            ..SeverityConfig::default()
        };
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &severity);
        let mut issue = issue("revive", "a.go", "dot-imports: should not use dot imports");
        issue.severity = "warning".into();
        let kept = filter.apply(vec![issue], &[]);
        assert_eq!(kept[0].severity, "warning");
    }
}
