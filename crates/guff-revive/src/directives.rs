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
//! DEFERRED: `directive-specify-disable-reason`, which turns a directive with
//! no trailing reason into a failure of its own.

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

/// Upstream `handleConfig`: a toggle is recorded only when it changes the
/// state, and a leading `enable` for a rule that was never disabled is dropped.
fn handle_config(map: &mut HashMap<String, Vec<Toggle>>, enabled: bool, line: i64, name: &str) {
    let existing = map.entry(name.to_string()).or_default();
    if (existing.len() > 1 && existing[existing.len() - 1].enabled == enabled)
        || (existing.is_empty() && enabled)
    {
        return;
    }
    existing.push(Toggle { enabled, line });
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
            // A one-line window: open and close it on the same line.
            "line" => {
                handle_config(map, enabled, line, name);
                handle_config(map, !enabled, line, name);
            }
            "next-line" => {
                handle_config(map, enabled, line + 1, name);
                handle_config(map, !enabled, line + 1, name);
            }
            _ => handle_config(map, enabled, line, name),
        }
    }
}

/// Collect the directives in `pass`'s files.
///
/// `all_rules` is the enabled rule set, used when a directive names none.
pub fn collect(pass: &Pass<'_>, all_rules: &[String]) -> Directives {
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
