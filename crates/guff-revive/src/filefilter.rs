//! Port of revive's `lint/filefilter.go` — the per-rule `exclude` list.
//!
//! A rule entry in the config may carry `exclude`, and revive skips that rule
//! for any file the list matches (`lint/file.go`):
//!
//! ```go
//! ruleConfig := rulesConfig[currentRule.Name()]
//! if ruleConfig.MustExclude(f.Name) { continue }
//! ```
//!
//! `f.Name` is the file name golangci-lint handed revive, which is the absolute
//! path from the pass (`internal.GetGoFileNames`). Every glob here starts with
//! `**/` in practice for exactly that reason.
//!
//! Four forms, in the order `prepareRegexp` tests them:
//!
//! | raw | meaning |
//! |---|---|
//! | `""` | matches nothing |
//! | `*` or `~` | matches everything |
//! | `TEST` | rewritten to `~_test\.go` |
//! | `~…` | the rest is a regular expression |
//! | contains `*` | a glob, expanded below |
//! | anything else | a whole-file mask: `\` → `/`, dots escaped, anchored |

use regex::Regex;

/// One parsed entry of a rule's `exclude` list.
#[derive(Debug, Clone)]
pub struct FileFilter {
    raw: String,
    rx: Option<Regex>,
    matches_all: bool,
    matches_nothing: bool,
}

impl FileFilter {
    /// `ParseFileFilter`. An invalid pattern is a configuration error upstream;
    /// here it becomes a filter that matches nothing, so a bad entry cannot
    /// silently exclude everything.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim().to_string();
        let matches_nothing = raw.is_empty();
        let matches_all = raw == "*" || raw == "~";
        let rx = if matches_all || matches_nothing {
            None
        } else {
            prepare_regexp(&raw)
        };
        Self {
            matches_nothing: matches_nothing || (!matches_all && rx.is_none()),
            raw,
            rx,
            matches_all,
        }
    }

    /// The raw pattern as written in the config.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// `MatchFileName`. Backslashes are normalised to `/` first.
    pub fn matches(&self, name: &str) -> bool {
        if self.matches_all {
            return true;
        }
        if self.matches_nothing {
            return false;
        }
        let name = name.replace('\\', "/");
        self.rx.as_ref().is_some_and(|rx| rx.is_match(&name))
    }
}

/// `[^/]\*\*[^/]` — a `**` that is not its own path segment is rejected.
fn invalid_glob(src: &str) -> bool {
    let b = src.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'*' && i + 1 < b.len() && b[i + 1] == b'*' {
            let before_ok = i == 0 || b[i - 1] == b'/';
            let after_ok = i + 2 >= b.len() || b[i + 2] == b'/';
            if !before_ok && !after_ok {
                return true;
            }
        }
    }
    false
}

const ESCAPE_REGEX_SYMBOLS: &str = ".+{}()[]^$";

fn prepare_regexp(raw: &str) -> Option<Regex> {
    let src = if raw == "TEST" { r"~_test\.go" } else { raw };

    if let Some(rest) = src.strip_prefix('~') {
        return Regex::new(rest).ok();
    }

    if src.contains('*') {
        if invalid_glob(src) {
            return None;
        }
        // The `justDirGlob` flag is what makes `**/x` match a bare `x`: after a
        // `**` the following `/` is emitted as `/?`.
        let mut out = String::from("^");
        let mut was_star = false;
        let mut just_dir_glob = false;
        for c in src.chars() {
            if c == '*' {
                if was_star {
                    out.push_str(r"[\s\S]*");
                    was_star = false;
                    just_dir_glob = true;
                    continue;
                }
                was_star = true;
                continue;
            }
            if was_star {
                out.push_str("[^/]*");
                was_star = false;
            }
            if ESCAPE_REGEX_SYMBOLS.contains(c) {
                out.push('\\');
            }
            out.push(c);
            if c == '/' && just_dir_glob {
                out.push('?');
            }
            just_dir_glob = false;
        }
        if was_star {
            out.push_str("[^/]*");
        }
        out.push('$');
        return Regex::new(&out).ok();
    }

    // Whole-file mask.
    let mut fill = src.replace('\\', "/");
    fill = fill.replace('.', r"\.");
    Regex::new(&format!("^{fill}$")).ok()
}

/// `RuleConfig.MustExclude`.
pub fn must_exclude(filters: &[FileFilter], name: &str) -> bool {
    filters.iter().any(|f| f.matches(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pat: &str, name: &str) -> bool {
        FileFilter::parse(pat).matches(name)
    }

    #[test]
    fn glob_star_star_matches_any_depth_including_none() {
        assert!(m("**/accumulator.go", "accumulator.go"));
        assert!(m("**/accumulator.go", "agent/accumulator.go"));
        assert!(m("**/accumulator.go", "/abs/agent/accumulator.go"));
        assert!(!m("**/accumulator.go", "agent/accumulator_test.go"));
    }

    #[test]
    fn glob_directory_segment() {
        assert!(m("**/agent/**", "agent/accumulator.go"));
        assert!(m("**/agent/**", "/abs/x/agent/deep/f.go"));
        assert!(!m("**/agent/**", "/abs/x/agents/f.go"));
    }

    #[test]
    fn single_star_stops_at_a_separator() {
        assert!(m("plugins/*/x.go", "plugins/a/x.go"));
        assert!(!m("plugins/*/x.go", "plugins/a/b/x.go"));
    }

    #[test]
    fn test_marker_is_a_regexp() {
        assert!(m("TEST", "/abs/agent/accumulator_test.go"));
        assert!(!m("TEST", "/abs/agent/accumulator.go"));
    }

    #[test]
    fn tilde_is_a_raw_regexp() {
        assert!(m(r"~-tmp\.\d+\.go", "/abs/x-tmp.12.go"));
        assert!(!m(r"~-tmp\.\d+\.go", "/abs/x-tmp.go"));
    }

    #[test]
    fn star_and_tilde_alone_match_everything_and_empty_matches_nothing() {
        assert!(m("*", "anything.go"));
        assert!(m("~", "anything.go"));
        assert!(!m("", "anything.go"));
        assert!(!m("   ", "anything.go"));
    }

    #[test]
    fn plain_name_is_a_whole_file_mask() {
        assert!(m("pkg/mypkg/my.go", "pkg/mypkg/my.go"));
        assert!(!m("pkg/mypkg/my.go", "/abs/pkg/mypkg/my.go"));
        assert!(!m("my.go", "myXgo"));
    }

    #[test]
    fn a_star_star_glued_to_a_name_is_rejected() {
        // `[^/]\*\*[^/]` — upstream refuses the pattern; guff makes it match
        // nothing rather than everything.
        assert!(!m("a**b", "ab"));
    }

    #[test]
    fn backslashes_are_normalised_on_both_sides() {
        assert!(m("**/agent/**", r"C:\proj\agent\a.go"));
    }
}
