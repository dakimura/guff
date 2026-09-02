//! Post-processing filters for lint issues (golangci `result/processors`).
//!
//! Pipeline order (subset of golangci-lint):
//! GOCACHE/cgo → typecheck-overrides-everything → path (dirs/files) → text exclude →
//! exclude-rules (+ default excludes) →
//! nolint → generated → diff (new-from-*) → uniq-by-line → max-per-linter → max-same →
//! severity (+ unused nolintlint).

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use guff::position::FileSet;
use guff_analysis::Diagnostic;
use guff_packages::Package;
use guff_runner::{default_go_cache_dir, is_under_go_cache};
use regex::Regex;

use crate::config::{ExcludeRule, IssuesConfig, SeverityConfig};
use crate::diff::{build_diff_state, DiffFilterSpec};
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
    /// When non-empty (v2 `linters.exclusions.paths-except`), only matching
    /// paths are kept.
    paths_except_res: Vec<Regex>,
    exclude_text_res: Vec<Regex>,
    exclude_rules: Vec<CompiledRule>,
    max_issues_per_linter: i32,
    max_same_issues: i32,
    uniq_by_line: bool,
    default_severity: Option<String>,
    severity_rules: Vec<(CompiledRule, String)>,
    /// `Some` when nolintlint is enabled at all, carrying its settings.
    ///
    /// One field and not a `report_unused` bool beside it: the directive-shape
    /// findings (leading space, malformed, and the two `require-*` settings)
    /// are not optional — upstream forces `NeedsMachineOnly` on whenever the
    /// linter runs — so "nolintlint is on" and "report unused directives" are
    /// different questions and used to be settable inconsistently.
    pub nolintlint: Option<crate::nolintlint::NolintlintStyle>,
    /// Linters enabled for this run (for unused-nolintlint parity with golangci).
    pub enabled_linters: HashSet<String>,
    /// Go build cache directory; issues under it are dropped (cgo artifacts).
    go_cache_dir: Option<PathBuf>,
    /// Diff-based "new issues only" filter (golangci Diff processor).
    diff_spec: Option<DiffFilterSpec>,
    /// `linters.exclusions.generated` mode (`None` → do not filter).
    generated: Option<guff_fmt::GeneratedMode>,
    /// Base directory for path-pattern matching (golangci `relative-path-mode`,
    /// default ≈ config-file dir). Absolute issue paths are relativized against
    /// this before `exclusions.paths` / rule `path` regexes run — otherwise a
    /// pattern like `.github` matches `/github.com/` in the absolute path.
    path_base: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    /// Linter names as written in the config, compared verbatim.
    linters: Vec<String>,
    path: Option<Regex>,
    path_except: Option<Regex>,
    text: Option<Regex>,
    source: Option<Regex>,
}

/// What `apply_with_fixer` needs to write a fix: the run's shared `FileSet`
/// (edits are resolved through it) and the meta formatter golangci runs over
/// every file it touched.
pub struct FixerCtx<'a> {
    pub fset: &'a FileSet,
    pub formatter: Option<&'a guff_fmt::MetaFormatter>,
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
            go_cache_dir: default_go_cache_dir().ok(),
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
        for p in &issues.paths_except {
            push_re(
                &mut filter.paths_except_res,
                &normalize_path_regex(p),
                "paths-except",
            );
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

        let diff_spec = DiffFilterSpec {
            new: issues.new,
            new_from_rev: issues.new_from_rev.clone(),
            new_from_merge_base: issues.new_from_merge_base.clone(),
            new_from_patch: issues.new_from_patch.clone(),
            whole_files: issues.whole_files,
        };
        if diff_spec.enabled() {
            filter.diff_spec = Some(diff_spec);
        }

        // v2 default for linters.exclusions.generated is lax when the key is
        // present; when absent (None) we leave filtering off so v1 configs and
        // tests without the key keep prior behavior. Callers that fold v2
        // exclusions should set `issues.generated` explicitly (including
        // `Some("lax")` when the YAML key is present).
        if let Some(ref mode) = issues.generated {
            filter.generated = Some(guff_fmt::GeneratedMode::parse(Some(mode.as_str())));
        }

        filter
    }

    /// Set the directory used to relativize absolute issue paths before
    /// matching `exclusions.paths` / rule `path` patterns (golangci cfg/gomod).
    pub fn with_path_base(mut self, base: impl Into<PathBuf>) -> Self {
        self.path_base = Some(base.into());
        self
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
            let column = diag.column.map_or(column, i64::from);
            let source_line = None; // filled lazily in apply / for printing
            // Match golangci: when the pass/check name differs from the linter
            // name, prefix Text (`inline: …`, `SA1004: …`, category for suites).
            let text = format_issue_text(&from_linter, &analyzer, &diag.category, &diag.message);
            out.push(Issue {
                from_linter,
                analyzer,
                text,
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
    /// Where `--fix` writes, when it is on.
    ///
    /// golangci runs the Fixer as a *processor*, between `Diff` and
    /// `UniqByLine` (`pkg/lint/runner.go:110`). That placement is observable:
    /// `Fixer.Process` returns `notFixableIssues`, so an issue it fixed leaves
    /// the stream before dedup and before every cap — it can lose its place in
    /// the report and still have edited the file.
    ///
    /// guff used to apply fixes *after* the whole pipeline, on the survivors,
    /// which silently left unfixed anything `uniq-by-line`,
    /// `max-issues-per-linter` or `max-same-issues` dropped. The
    /// `issues-uniq-by-line-order` case is built to make dedup decide
    /// something, and it is where that showed: three `//nolint` directives
    /// upstream deletes, whose nolintlint findings all lose their line to
    /// revive, `unused` and errcheck (COMPAT-HARDENING 続き 78).
    pub fn apply(&self, issues: Vec<Issue>, packages: &[Arc<Package>]) -> Vec<Issue> {
        self.apply_with_fixer(issues, packages, None).0
    }

    /// [`apply`], with `--fix` applied at golangci's Fixer position.
    ///
    /// Returns the issues that remain — the ones no fix consumed — and how many
    /// were fixed, which is the same contract as upstream's `notFixableIssues`.
    pub fn apply_with_fixer(
        &self,
        mut issues: Vec<Issue>,
        packages: &[Arc<Package>],
        fixer: Option<FixerCtx<'_>>,
    ) -> (Vec<Issue>, usize) {
        // golangci attributes findings to the enabled alias name when the
        // deprecated parent is not also enabled (e.g. enable: [gomodguard_v2]).
        remap_enabled_alias_from_linters(&mut issues, &self.enabled_linters);

        // golangci runs linters in name order (`GetOptimizedLinters` sorts, and
        // `Runner.Run` appends each linter's issues in turn), so by the time the
        // processors see the slice it is grouped by linter name. That order is
        // observable: `uniq-by-line` keeps the *first* issue on a line whatever
        // linter produced it, and `max-same-issues` keeps the first N. guff
        // produces diagnostics in analyzer×package graph order instead, so sort
        // here — stably, to leave each linter's own order alone.
        //
        // Name order is the whole rule for everything that reaches this point.
        // Upstream's two exceptions do not apply: `DoesChangeTypes` (which
        // would move `unused` to the end) only orders the *top-level* linter
        // list, and in golangci-lint 2.12.2 every linter is inside the one
        // metalinter, whose own sort is by name; and `linter.LastLinter`
        // (nolintlint) is handled where nolintlint's findings are born, which
        // is after this sort — see `NolintIndex::filter_issues`.
        issues.sort_by(|a, b| a.from_linter.cmp(&b.from_linter));

        // golangci Cgo processor: drop issues under GOCACHE / _cgo_gotypes.go.
        let go_cache = self.go_cache_dir.as_deref();
        issues.retain(|issue| {
            !is_under_go_cache(Path::new(&issue.filename), go_cache)
        });

        // golangci `InvalidIssue`, fourth in its processor list: a run with any
        // typecheck issue reports *only* those. It is here, ahead of every
        // exclusion, because that is where upstream puts it — an exclude rule
        // cannot bring the silenced findings back.
        crate::typecheck::keep_only_typecheck(&mut issues);

        issues.retain(|issue| !self.is_excluded_by_path(issue));
        issues.retain(|issue| {
            !self
                .exclude_text_res
                .iter()
                .any(|re| re.is_match(&issue.text))
        });

        // Every exclusion runs *before* the nolint processor upstream —
        // `exclude` (the presets and `exclude` patterns) and `exclude_rules`
        // both sit ahead of it in `Runner.Run`'s processor list — so a finding
        // an exclusion removes never reaches a `//nolint` directive, and the
        // directive is left unused.
        //
        // This used to mark matches *first*, with a comment claiming that a
        // directive covering a preset-excluded finding still counts as used.
        // It does not, measured both ways: with `exclusions.rules` matching
        // `source: Rollback` (syncthing, five directives) and with the
        // `std-error-handling` preset's EXC0001 covering `defer f.Close()`,
        // upstream reports `directive … is unused` and guff said nothing.
        if self.nolintlint.is_some() && !packages.is_empty() {
            issues.retain(|issue| {
                !self
                    .exclude_rules
                    .iter()
                    .any(|rule| rule.matches(issue, self.path_base.as_deref()))
            });
            let mut idx =
                NolintIndex::from_packages_for_issues(packages, &issues, self.nolintlint.as_ref());
            idx.set_enabled_linters(self.enabled_linters.iter().cloned());
            idx.mark_matches(&issues);
            let report_unused = self
                .nolintlint
                .as_ref()
                .is_some_and(|s| s.report_unused);
            issues = idx.filter_issues(issues, report_unused);

            // `filter_issues` is where nolintlint's own findings are born, and
            // that is *after* the three exclusion filters above already ran —
            // so until now nothing could exclude them. Upstream has no such
            // hole: nolintlint is an ordinary linter there, its issues exist
            // before any processor runs, and `ExclusionPaths` / exclude-text /
            // `ExclusionRules` all see them. Re-run the three; they are
            // idempotent for the issues that already passed.
            //
            // Measured on k9s, whose config excludes the path `internal/x`
            // (an unanchored regex, so it covers `internal/xray/`): upstream
            // drops the nolintlint finding on `internal/xray/section.go:64`
            // along with every other finding in that tree, and guff kept it.
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
                    .any(|rule| rule.matches(issue, self.path_base.as_deref()))
            });
        } else {
            issues.retain(|issue| {
                !self
                    .exclude_rules
                    .iter()
                    .any(|rule| rule.matches(issue, self.path_base.as_deref()))
            });
            if !packages.is_empty() {
                let mut idx = NolintIndex::from_packages_for_issues(packages, &issues, None);
                idx.set_enabled_linters(self.enabled_linters.iter().cloned());
                issues = idx.filter_issues(issues, false);
            }
        }

        // Drop issues in generated files (linters.exclusions.generated).
        if let Some(mode) = self.generated {
            if mode != guff_fmt::GeneratedMode::Disable {
                let mut cache: HashMap<String, bool> = HashMap::new();
                issues.retain(|issue| {
                    if issue.filename.is_empty() {
                        return true;
                    }
                    let is_gen = *cache.entry(issue.filename.clone()).or_insert_with(|| {
                        file_is_generated(&issue.filename, mode)
                    });
                    !is_gen
                });
            }
        }

        // Diff processor: keep only issues in the new/changed region.
        // Runs after excludes/nolint and before max-* (golangci Diff order).
        if let Some(ref spec) = self.diff_spec {
            match build_diff_state(spec) {
                Ok(Some(state)) => {
                    issues.retain(|issue| state.keeps(issue));
                }
                Ok(None) => {}
                Err(e) => {
                    eprintln!("guff: diff filter disabled: {e}");
                }
            }
        }

        // The Fixer sits here, between `Diff` and `UniqByLine`
        // (golangci `pkg/lint/runner.go:107..113`). Everything below this line
        // therefore sees only what no fix consumed — dedup and the caps count
        // the *unfixed* remainder, as upstream's do.
        let mut fixes_applied = 0;
        if let Some(ctx) = fixer {
            match crate::fix::apply_fixes(ctx.fset, &issues, ctx.formatter) {
                Ok((remaining, n)) => {
                    issues = remaining;
                    fixes_applied = n;
                }
                Err(e) => eprintln!("guff: --fix failed: {e}"),
            }
        }

        if self.uniq_by_line {
            // golangci's `UniqByLine` counts per (file, line) only — not per
            // linter, and not per column. One line yields at most one issue in
            // the whole run, and the survivor is whichever arrived first (see
            // the sort at the top of `apply`). Keying on the linter as well used
            // to let, say, errcheck and staticcheck's SA4017 both report the
            // same ignored call.
            let mut seen = std::collections::HashSet::new();
            issues.retain(|issue| seen.insert((issue.filename.clone(), issue.line)));
        }

        // Order matters when both limits are set, and it is `MaxSameIssues`
        // then `MaxFromLinter` (golangci `Runner.Processors`). The per-linter
        // counter only ever sees what survived the per-text cut, so a linter
        // that reported the same text N times spends one slot on it, not N.
        // Reversed — as this was — `max-issues-per-linter: 3` with
        // `max-same-issues: 1` fills the linter's budget with three copies of
        // one text and then drops two of them, losing the findings that would
        // have come after.
        if self.max_same_issues > 0 {
            let mut counts: HashMap<String, i32> = HashMap::new();
            issues.retain(|issue| {
                let n = counts.entry(issue.text.clone()).or_insert(0);
                *n += 1;
                *n <= self.max_same_issues
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

        for issue in &mut issues {
            self.assign_severity(issue);
        }

        // Source lines are only needed for printing / JSON SourceLines — load
        // after filtering so we don't read ~10k files that exclude-rules drop.
        for issue in &mut issues {
            if issue.source_line.is_none() {
                issue.source_line = read_source_line(&issue.filename, issue.line);
            }
        }

        (issues, fixes_applied)
    }

    fn is_excluded_by_path(&self, issue: &Issue) -> bool {
        let path = path_for_match(&issue.filename, self.path_base.as_deref());
        let parent = dirname_slash(path.as_ref());

        for re in &self.exclude_dir_res {
            if re.is_match(parent) || re.is_match(path.as_ref()) {
                return true;
            }
        }
        for re in &self.exclude_file_res {
            if re.is_match(path.as_ref()) {
                return true;
            }
        }
        // paths-except: if configured, drop anything that does not match.
        if !self.paths_except_res.is_empty()
            && !self
                .paths_except_res
                .iter()
                .any(|re| re.is_match(path.as_ref()))
        {
            return true;
        }
        false
    }

    fn assign_severity(&self, issue: &mut Issue) {
        for (rule, sev) in &self.severity_rules {
            if rule.matches(issue, self.path_base.as_deref()) {
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
    fn matches(&self, issue: &Issue, path_base: Option<&Path>) -> bool {
        let empty = self.linters.is_empty()
            && self.path.is_none()
            && self.path_except.is_none()
            && self.text.is_none()
            && self.source.is_none();
        if empty {
            return false;
        }
        // Linters first: most rules are linter-scoped; avoid text/path regex
        // work on the other ~10k issues that will never match.
        //
        // The comparison is `slices.Contains(r.linters, issue.FromLinter)` —
        // verbatim, against the linter name only. It is deliberately not the
        // analyzer name: `//nolint:printf` and `linters: [printf]` both name
        // something golangci-lint has never heard of, and both do nothing,
        // however prominently `printf: ` is printed in the message. Matching
        // the analyzer too (and matching alias-normalized names) silently
        // removed findings upstream keeps.
        if !self.linters.is_empty() && !self.linters.iter().any(|l| l == &issue.from_linter) {
            return false;
        }
        if let Some(re) = &self.text {
            if !re.is_match(&issue.text) {
                return false;
            }
        }
        if self.path.is_some() || self.path_except.is_some() {
            let path = path_for_match(&issue.filename, path_base);
            if let Some(re) = &self.path {
                if !re.is_match(path.as_ref()) {
                    return false;
                }
            }
            if let Some(re) = &self.path_except {
                if re.is_match(path.as_ref()) {
                    return false;
                }
            }
        }
        if let Some(re) = &self.source {
            // Prefer a pre-filled line; otherwise read just for this match so we
            // don't force a full-tree source preload when only a few rules use
            // `source:` (e.g. prometheus godot).
            let owned;
            let line = if let Some(ref l) = issue.source_line {
                l.as_str()
            } else {
                owned = read_source_line(&issue.filename, issue.line).unwrap_or_default();
                owned.as_str()
            };
            if !re.is_match(line) {
                return false;
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

/// Relativize `filename` against `base` for path-pattern matching.
///
/// golangci matches `exclusions.paths` against paths relative to the config /
/// module root. Matching the absolute path lets `.github` (regex `.` = any
/// char) hit `/github.com/` in the checkout path — a nats-server OSS hunt
/// false negative for every finding under a `github.com/...` tree.
///
/// When `base` does not prefix the file (e.g. harness copies the config to a
/// temp dir), walk up from the file looking for `go.mod` and relativize to
/// that module root.
fn path_for_match<'a>(filename: &'a str, base: Option<&Path>) -> Cow<'a, str> {
    let norm = normalize_slashes(filename);
    if let Some(base) = base {
        if let Some(rel) = strip_base_prefix(norm.as_ref(), base) {
            return Cow::Owned(rel);
        }
    }
    if let Some(rel) = relativize_via_gomod(norm.as_ref()) {
        return Cow::Owned(rel);
    }
    norm
}

fn strip_base_prefix(norm: &str, base: &Path) -> Option<String> {
    let base_s = normalize_slashes(&base.to_string_lossy()).into_owned();
    let base_slash = base_s.trim_end_matches('/');
    if base_slash.is_empty() {
        return None;
    }
    let prefix = format!("{base_slash}/");
    if let Some(rel) = norm.strip_prefix(&prefix) {
        return Some(rel.to_string());
    }
    if norm == base_slash {
        return Some(String::new());
    }
    None
}

fn relativize_via_gomod(abs: &str) -> Option<String> {
    let path = Path::new(abs);
    if !path.is_absolute() {
        return None;
    }
    let parent = path.parent()?;
    // Cache module roots: path exclusion runs per-issue and must not walk
    // the filesystem on every call (prometheus / OSS hunt scale).
    static CACHE: OnceLock<std::sync::Mutex<HashMap<String, Option<PathBuf>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let parent_key = normalize_slashes(&parent.to_string_lossy()).into_owned();
    if let Ok(mut guard) = cache.lock() {
        if let Some(cached) = guard.get(&parent_key) {
            return cached
                .as_ref()
                .and_then(|root| strip_base_prefix(abs, root));
        }
        let mut dir = parent;
        let found = loop {
            if dir.join("go.mod").is_file() {
                break Some(dir.to_path_buf());
            }
            match dir.parent() {
                Some(p) => dir = p,
                None => break None,
            }
        };
        // Also remember intermediate dirs under the same root.
        if let Some(ref root) = found {
            let mut d = parent;
            loop {
                let key = normalize_slashes(&d.to_string_lossy()).into_owned();
                guard.insert(key, Some(root.clone()));
                if d == root.as_path() {
                    break;
                }
                match d.parent() {
                    Some(p) => d = p,
                    None => break,
                }
            }
        } else {
            guard.insert(parent_key, None);
        }
        return found.as_ref().and_then(|root| strip_base_prefix(abs, root));
    }
    // Mutex poisoned — fall back to a one-shot walk.
    let mut dir = parent;
    loop {
        if dir.join("go.mod").is_file() {
            return strip_base_prefix(abs, dir);
        }
        dir = dir.parent()?;
    }
}

/// Normalize `\` → `/` without allocating when the path already uses `/`.
fn normalize_slashes(path: &str) -> Cow<'_, str> {
    if path.as_bytes().contains(&b'\\') {
        Cow::Owned(path.replace('\\', "/"))
    } else {
        Cow::Borrowed(path)
    }
}

/// Parent directory of a slash-normalized path (no allocation).
fn dirname_slash(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) => "/",
        Some(i) => &path[..i],
        None => "",
    }
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

/// Leading-bytes generated-file check (avoids reading multi-MB sources fully).
fn file_is_generated(path: &str, mode: guff_fmt::GeneratedMode) -> bool {
    const PREFIX: u64 = 16 * 1024;
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut buf = vec![0u8; PREFIX as usize];
    let n = f.read(&mut buf).unwrap_or(0);
    buf.truncate(n);
    guff_fmt::is_generated(&buf, mode)
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
    // Defer source_line to IssueFilter::apply (after exclude pipeline).
    let from_linter = linter_name_for_analyzer(analyzer).to_string();
    let text = format_issue_text(&from_linter, analyzer, category, message);
    Issue {
        from_linter,
        analyzer: analyzer.to_string(),
        text,
        severity: severity.to_string(),
        filename: filename.to_string(),
        line,
        column,
        source_line: None,
        diagnostic: Diagnostic {
            message: message.to_string(),
            category: category.to_string(),
            url: url.to_string(),
            severity: severity.to_string(),
            ..Diagnostic::default()
        },
    }
}

/// When a deprecated linter and its alias share an analyzer, prefer the
/// enabled alias name for `FromLinter` (golangci-lint v2 parity).
fn remap_enabled_alias_from_linters(issues: &mut [Issue], enabled: &HashSet<String>) {
    const PAIRS: &[(&str, &str)] = &[("gomodguard", "gomodguard_v2")];
    if enabled.is_empty() {
        return;
    }
    for &(canon, alias) in PAIRS {
        if enabled.contains(alias) && !enabled.contains(canon) {
            for issue in issues.iter_mut() {
                if issue.from_linter == canon {
                    issue.from_linter = alias.to_string();
                }
            }
        }
    }
}

/// golangci-style Text: prefix with pass/check name when it differs from the linter.
fn format_issue_text(from_linter: &str, analyzer: &str, category: &str, message: &str) -> String {
    let pass_name = if !category.is_empty() {
        category
    } else {
        analyzer
    };
    if pass_name != from_linter {
        format!("{pass_name}: {message}")
    } else {
        message.to_string()
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

    /// Like [`issue`], but on an explicit line. `uniq-by-line` keys on
    /// (file, line) alone, so a test that wants two issues to survive in the
    /// same file has to put them on different lines.
    fn issue_at(linter: &str, file: &str, line: i64, text: &str) -> Issue {
        Issue {
            line,
            ..issue(linter, file, text)
        }
    }

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
                issue_at("errcheck", "a.go", 1, "one"),
                issue_at("errcheck", "a.go", 2, "two"),
                issue_at("govet", "a.go", 3, "three"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].text, "one");
        assert_eq!(kept[1].from_linter, "govet");
    }

    #[test]
    fn max_same_issues_runs_before_max_issues_per_linter() {
        // golangci `Runner.Processors` is MaxSameIssues then MaxFromLinter, so
        // the per-linter budget is spent on what survived the per-text cut.
        // Reversed, the three copies of "dup" fill errcheck's budget of 3 and
        // the per-text cut then leaves one of them — losing "tail" outright.
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 3,
            max_same_issues: 1,
            uniq_by_line: Some(false),
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue_at("errcheck", "a.go", 1, "dup"),
                issue_at("errcheck", "a.go", 2, "dup"),
                issue_at("errcheck", "a.go", 3, "dup"),
                issue_at("errcheck", "a.go", 4, "tail"),
            ],
            &[],
        );
        let texts: Vec<&str> = kept.iter().map(|i| i.text.as_str()).collect();
        assert_eq!(texts, vec!["dup", "tail"]);
    }

    #[test]
    fn uniq_by_line_keeps_one_issue_per_line_across_linters() {
        // golangci's UniqByLine counts per (file, line) — not per linter and not
        // per column — and keeps whichever issue arrived first. Linters run in
        // name order there, so errcheck beats staticcheck on a shared line. This
        // is what hides SA4017 on a call errcheck already flagged.
        let filter =
            IssueFilter::from_config(&IssuesConfig::default(), &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue_at("staticcheck", "a.go", 7, "SA4017: mayErr doesn't have side effects"),
                issue_at("errcheck", "a.go", 7, "Error return value is not checked"),
                issue_at("errcheck", "a.go", 8, "Error return value is not checked"),
                issue_at("staticcheck", "b.go", 7, "SA4017: mayErr doesn't have side effects"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 3);
        assert_eq!(kept[0].from_linter, "errcheck");
        assert_eq!(kept[0].line, 7);
        assert_eq!(kept[1].line, 8);
        assert_eq!(kept[2].filename, "b.go");
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
                issue_at("errcheck", "a.go", 1, "x"),
                issue_at("govet", "a.go", 2, "y"),
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

    #[test]
    fn drops_issues_under_go_cache() {
        let mut filter = IssueFilter::default();
        filter.go_cache_dir = Some(PathBuf::from("/var/gocache"));
        let kept = filter.apply(
            vec![
                issue("gosec", "/var/gocache/xyz/cgo.go", "G103"),
                issue("gosec", "/src/main.go", "G103"),
                issue("gosec", "/tmp/_cgo_gotypes.go", "G103"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].filename, "/src/main.go");
    }

    #[test]
    fn comments_preset_drops_st1000_with_staticcheck_from_linter() {
        use crate::config::parse_config_str;
        let yaml = r#"
version: "2"
linters:
  default: none
  enable: [staticcheck]
  exclusions:
    presets: [comments]
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let filter = IssueFilter::from_config(&cfg.effective_issues(), &SeverityConfig::default());
        let mut st = issue(
            "staticcheck",
            "a.go",
            "ST1000: at least one file in a package should have a package comment",
        );
        st.analyzer = "ST1000".into();
        let kept = filter.apply(vec![st, issue("errcheck", "a.go", "unchecked")], &[]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].from_linter, "errcheck");
    }

    #[test]
    fn exclude_rule_linters_names_the_linter_not_the_analyzer() {
        // `baseRule.matchLinter` is `slices.Contains(r.linters, FromLinter)`.
        // `printf` is govet's analyzer, prominent in the message and useless
        // here — matching it as well removed findings upstream keeps.
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            uniq_by_line: Some(false),
            exclude_rules: vec![ExcludeRule {
                linters: vec!["printf".into()],
                path: Some(r"a\.go".into()),
                ..ExcludeRule::default()
            }],
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let mut printf = issue("govet", "a.go", "printf: wrong type");
        printf.analyzer = "printf".into();
        let kept = filter.apply(vec![printf], &[]);
        assert_eq!(kept.len(), 1, "a rule naming the analyzer matches nothing");
    }

    #[test]
    fn v2_exclusion_rule_text_is_case_sensitive() {
        // v2 compiles exclusion rules with an empty prefix and has no
        // `exclude-case-sensitive` key; the v1 default of `(?i)` would widen
        // every pattern.
        use crate::config::parse_config_str;
        let yaml = r#"
version: "2"
linters:
  default: none
  enable: [errcheck]
  exclusions:
    rules:
      - linters: [errcheck]
        text: ERROR RETURN VALUE
"#;
        let cfg = parse_config_str(yaml).unwrap();
        let filter = IssueFilter::from_config(&cfg.effective_issues(), &cfg.effective_severity());
        let kept = filter.apply(
            vec![issue("errcheck", "a.go", "Error return value is not checked")],
            &[],
        );
        assert_eq!(kept.len(), 1, "the pattern is used verbatim");
    }

    #[test]
    fn generated_lax_drops_issues_in_generated_file() {
        let dir = tempfile::tempdir().unwrap();
        let gen = dir.path().join("gen.go");
        let hand = dir.path().join("hand.go");
        std::fs::write(
            &gen,
            "// Code generated by tool. DO NOT EDIT.\npackage p\n",
        )
        .unwrap();
        std::fs::write(&hand, "package p\n").unwrap();

        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            generated: Some("lax".into()),
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let kept = filter.apply(
            vec![
                issue("revive", gen.to_str().unwrap(), "unused-parameter: x"),
                issue("revive", hand.to_str().unwrap(), "unused-parameter: y"),
            ],
            &[],
        );
        assert_eq!(kept.len(), 1);
        assert!(kept[0].filename.ends_with("hand.go"));
    }

    #[test]
    fn diff_filter_spec_wired_from_config() {
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            max_issues_per_linter: 0,
            max_same_issues: 0,
            whole_files: true,
            new_from_merge_base: Some("origin/main".into()),
            ..IssuesConfig::default()
        };
        let filter = IssueFilter::from_config(&issues_cfg, &SeverityConfig::default());
        let spec = filter.diff_spec.expect("diff enabled");
        assert!(spec.whole_files);
        assert_eq!(
            spec.new_from_merge_base.as_deref(),
            Some("origin/main")
        );
    }

    #[test]
    fn dirname_slash_matches_path_parent() {
        assert_eq!(dirname_slash("/tmp/pkg/a.go"), "/tmp/pkg");
        assert_eq!(dirname_slash("/a.go"), "/");
        assert_eq!(dirname_slash("a.go"), "");
        assert_eq!(dirname_slash("pkg/a.go"), "pkg");
    }

    #[test]
    fn path_for_match_strips_base_so_dot_github_does_not_hit_github_com() {
        // Absolute checkout under github.com must not match exclusions.paths: .github
        let abs = "/Users/me/src/github.com/nats-io/nats-server/server/test_test.go";
        let base = Path::new("/Users/me/src/github.com/nats-io/nats-server");
        let rel = path_for_match(abs, Some(base));
        assert_eq!(rel.as_ref(), "server/test_test.go");
        let re = Regex::new(&normalize_path_regex(".github")).unwrap();
        assert!(
            !re.is_match(rel.as_ref()),
            ".github must not match relativized path {rel}"
        );
        // Without base, `.` matches `/` before `github.com`.
        assert!(re.is_match(abs));
    }

    #[test]
    fn path_for_match_falls_back_to_gomod_when_base_misses() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("github.com").join("nats-io").join("mod");
        let server = root.join("server");
        std::fs::create_dir_all(&server).unwrap();
        std::fs::write(root.join("go.mod"), "module example.com/mod\n").unwrap();
        let file = server.join("test_test.go");
        std::fs::write(&file, "package server\n").unwrap();

        // Config copied outside the module (hunt harness) — base does not prefix.
        let rel = path_for_match(file.to_str().unwrap(), Some(Path::new("/tmp/hunt-results")));
        assert_eq!(rel.as_ref(), "server/test_test.go");
        let re = Regex::new(&normalize_path_regex(".github")).unwrap();
        assert!(!re.is_match(rel.as_ref()));
    }

    #[test]
    fn exclusions_paths_dot_github_with_path_base_keeps_server_files() {
        let issues_cfg = IssuesConfig {
            exclude_use_default: false,
            exclude_dirs_use_default: Some(false),
            max_issues_per_linter: 0,
            max_same_issues: 0,
            exclude_files: vec![".github".into()],
            ..IssuesConfig::default()
        };
        let base = PathBuf::from("/Users/me/src/github.com/nats-io/nats-server");
        let filter =
            IssueFilter::from_config(&issues_cfg, &SeverityConfig::default()).with_path_base(base);
        let kept = filter.apply(
            vec![issue(
                "govet",
                "/Users/me/src/github.com/nats-io/nats-server/server/test_test.go",
                "inline: Constant reflect.Ptr should be inlined",
            )],
            &[],
        );
        assert_eq!(kept.len(), 1, "server file must not be dropped by .github");
    }

    #[test]
    fn normalize_slashes_avoids_alloc_without_backslash() {
        let p = "/tmp/pkg/a.go";
        match normalize_slashes(p) {
            Cow::Borrowed(s) => assert_eq!(s, p),
            Cow::Owned(_) => panic!("expected borrow"),
        }
        assert_eq!(normalize_slashes(r"C:\tmp\a.go").as_ref(), "C:/tmp/a.go");
    }
}
