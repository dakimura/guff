//! revive's `//revive:disable` directive comments.
//!
//! Port of `lint.File.disabledIntervals` + `filterFailures`. A directive turns
//! a rule off from its line to the end of the file, or for a single line with
//! the `-line` / `-next-line` modifiers, and an `enable` closes the interval a
//! `disable` opened. Naming no rule applies it to every enabled rule.
//!
//! ```go
//! var directiveRegexp = regexp.MustCompile(
//!     `^//[\s]*revive:(enable|disable)(?:-(line|next-line))?(?::([^\s]+))?[\s]*(?: (.+))?$`)
//! ```
//!
//! guff had none of this, so gitea's fourteen `//revive:disable-line:exported`
//! comments — the ordinary way to keep a name that stutters — were findings
//! golangci-lint does not report.
//!
//! The `directives` config adds two checks on a `disable`: `specify-disable-reason`
//! (no trailing reason) and revive v1.17.0's `specify-disable-rule` (no rule
//! named). Either is a failure of its own at the comment, and the directive it
//! rejects is then not applied.

use std::collections::HashMap;
use std::sync::OnceLock;

use guff_analysis::Pass;
use regex::Regex;

use crate::failure::Failure;

fn directive_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^//[\s]*revive:(enable|disable)(?:-(line|next-line))?(?::([^\s]+))?[\s]*(?: (.+))?$")
            .expect("revive directive regex")
    })
}

/// One `[from, to]` line interval, inclusive, during which a rule is off.
#[derive(Debug, Clone, Copy)]
struct Interval {
    from: i64,
    to: i64,
}

#[derive(Debug, Clone, Copy)]
struct Toggle {
    enabled: bool,
    line: i64,
}

/// `disabledIntervals` **per file**, keyed by rule name.
///
/// Upstream computes them one file at a time (`lint.File.disabledIntervals`),
/// and that is load-bearing: a `//revive:disable:exported` in one file must not
/// silence the next file in the same package.
#[derive(Default)]
pub struct Directives {
    per_file: HashMap<String, HashMap<String, Vec<Interval>>>,
}

impl Directives {
    /// True when `rule` is disabled at `line` of `file`.
    pub fn disabled(&self, file: &str, rule: &str, line: i64) -> bool {
        let Some(intervals) = self.per_file.get(file).and_then(|m| m.get(rule)) else {
            return false;
        };
        intervals.iter().any(|i| line >= i.from && line <= i.to)
    }

    pub fn is_empty(&self) -> bool {
        self.per_file.is_empty()
    }
}

/// Upstream `handleConfig` (revive v1.17.0): a toggle is recorded only when it
/// changes the state — a rule with no toggles yet is enabled — and the answer
/// says whether it did.
fn handle_config(
    map: &mut HashMap<String, Vec<Toggle>>,
    enabled: bool,
    line: i64,
    name: &str,
) -> bool {
    let existing = map.entry(name.to_string()).or_default();
    let currently_enabled = existing.last().is_none_or(|t| t.enabled);
    if currently_enabled == enabled {
        return false;
    }
    existing.push(Toggle { enabled, line });
    true
}

fn handle_rules(
    map: &mut HashMap<String, Vec<Toggle>>,
    modifier: &str,
    enabled: bool,
    line: i64,
    rule_names: &[String],
) {
    for name in rule_names {
        match modifier {
            // A one-line window: open and close it on the same line — but only
            // when opening changed anything, so a `disable-line` inside a
            // disabled range does not end the range (revive v1.17.0).
            "line" => {
                if handle_config(map, enabled, line, name) {
                    handle_config(map, !enabled, line, name);
                }
            }
            "next-line" => {
                if handle_config(map, enabled, line + 1, name) {
                    handle_config(map, !enabled, line + 1, name);
                }
            }
            _ => {
                handle_config(map, enabled, line, name);
            }
        }
    }
}

pub const SPECIFY_DISABLE_REASON: &str = "specify-disable-reason";
pub const SPECIFY_DISABLE_RULE: &str = "specify-disable-rule";

/// Collect the directives in `pass`'s files.
///
/// `all_rules` is the enabled rule set, used when a directive names none.
pub fn collect(
    pass: &Pass<'_>,
    all_rules: &[String],
    settings: &crate::Settings,
    failures: &mut Vec<Failure>,
) -> Directives {
    let must_reason = settings.directive_enabled(SPECIFY_DISABLE_REASON);
    let must_rule = settings.directive_enabled(SPECIFY_DISABLE_RULE);
    let mut per_file: HashMap<String, HashMap<String, Vec<Interval>>> = HashMap::new();
    let pkg = pass.pkg();
    for (i, file) in pass.files().iter().enumerate() {
        let Some(path) = pkg.compiled_go_files.get(i) else {
            continue;
        };
        let Some(reparsed) = crate::util::reparse_with_comments(path, pkg.source_bytes(i)) else {
            continue;
        };
        let Some(ft) = pass.fset().file(file.pos()) else {
            continue;
        };
        let file_name = ft.name().to_string();
        let mut map: HashMap<String, Vec<Toggle>> = HashMap::new();
        for group in &reparsed.file.comments {
            // Upstream keys the directive on the line of the group's *end*.
            let line = reparsed.fset.position(group.end()).line;
            for c in &group.list {
                let Some(m) = directive_re().captures(c.text.as_str()) else {
                    continue;
                };
                let directive = m.get(1).map_or("", |g| g.as_str());
                let modifier = m.get(2).map_or("", |g| g.as_str());
                let rules_field = m.get(3).map_or("", |g| g.as_str());
                let mut rule_names: Vec<String> = rules_field
                    .split(',')
                    .map(|s| s.trim_matches('\n'))
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                let reason = m.get(4).map_or("", |g| g.as_str());
                let at = || {
                    crate::util::map_reparsed_pos(pass, file, &reparsed.fset, c.pos().0)
                };
                if must_reason && directive == "disable" && reason.trim_matches(' ').is_empty() {
                    if let Some(pos) = at() {
                        failures.push(Failure::with_confidence(
                            SPECIFY_DISABLE_REASON,
                            pos,
                            "reason of lint disabling not found",
                            1.0,
                        ));
                    }
                    continue;
                }
                if must_rule && directive == "disable" && rule_names.is_empty() {
                    if let Some(pos) = at() {
                        failures.push(Failure::with_confidence(
                            SPECIFY_DISABLE_RULE,
                            pos,
                            "rule name for lint disabling not found",
                            1.0,
                        ));
                    }
                    continue;
                }
                if rule_names.is_empty() {
                    rule_names = all_rules.to_vec();
                }
                handle_rules(&mut map, modifier, directive == "enable", line, &rule_names);
            }
        }

        if map.is_empty() {
            continue;
        }
        let mut intervals: HashMap<String, Vec<Interval>> = HashMap::new();
        for (rule, toggles) in map {
            let mut out: Vec<Interval> = Vec::new();
            for (i, t) in toggles.iter().enumerate() {
                if i % 2 == 0 {
                    out.push(Interval {
                        from: t.line,
                        // Upstream: `math.MaxInt32` until an `enable` closes it.
                        to: i64::from(i32::MAX),
                    });
                } else if let Some(last) = out.last_mut() {
                    last.to = t.line;
                }
            }
            intervals.insert(rule, out);
        }
        per_file.insert(file_name, intervals);
    }

    Directives { per_file }
}

/// Upstream `filterFailures`: drop a failure whose line falls in a disabled
/// interval for its own rule.
pub fn filter(pass: &Pass<'_>, directives: &Directives, failures: Vec<Failure>) -> Vec<Failure> {
    if directives.is_empty() {
        return failures;
    }
    failures
        .into_iter()
        .filter(|f| {
            let pos = pass.fset().position(guff::position::Pos(i64::from(f.pos)));
            !directives.disabled(&pos.filename, f.rule, pos.line)
        })
        .collect()
}
