//! `//nolint` directive parsing and issue filtering (golangci `nolint_filter`).
//!
//! Inline and preceding-line directives suppress matching issues. This is the
//! filter half; [`crate::nolintlint`] is the linter half, and the two parse the
//! same comment with different regexes because upstream does. The one finding
//! that needs both — "this directive suppressed nothing" — is settled here,
//! since only the filter knows what matched.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use guff::commentmap::{node_end, node_pos};
use guff::parser::{parse_file, COMMENTS_ONLY};
use guff::position::FileSet;
use guff::walk::{preorder, NodeRef};
use guff_analysis::Diagnostic;
use guff_packages::Package;
use memchr::memmem::Finder;
use regex::Regex;

use crate::config::normalize_linter_name;
use crate::exclude::Issue;
use crate::nolintlint::{self, Directive, NolintlintStyle};
use crate::registry::{analyzers_for_linter, linter_name_for_analyzer};

pub const NOLINTLINT_NAME: &str = "nolintlint";

/// One `//nolint` coverage range (possibly AST-expanded).
#[derive(Debug, Clone)]
struct IgnoredRange {
    /// First line the directive suppresses (the whole enclosing comment group).
    from: i64,
    /// Last line the directive suppresses.
    to: i64,
    /// Line the directive comment itself is on, used when reporting an unused
    /// directive. This is not `from`: a `//nolint` at the end of a godoc block
    /// suppresses the whole block but must still be reported on its own line.
    report_line: i64,
    /// Column of the directive comment, used when reporting it unused.
    report_col: i64,
    /// Column of the enclosing *comment group*, which is what the range
    /// expander compares against a node's start column
    /// (`rangeExpander.Visit`: `nodeStartPos.Column == r.col`). It is the
    /// group's and not the comment's: upstream builds the range from `g.Pos()`.
    col: i64,
    /// Empty = all linters (except nolintlint itself).
    linters: Vec<String>,
    /// Directive text for unused messages.
    comment_text: String,
    /// True when this range is an AST expansion of a preceding-line directive.
    is_expansion: bool,
}

impl IgnoredRange {
    fn does_match(&self, issue: &Issue) -> bool {
        if issue.line < self.from || issue.line > self.to {
            return false;
        }

        // Bare `//nolint` suppresses every linter except nolintlint.
        //
        // Only the *linter* name counts, never the analyzer's: upstream's
        // `doesMatch` tests `slices.Contains(i.linters, issue.FromLinter)`, and
        // the names in the directive have already been resolved through the
        // linter registry, so `//nolint:printf` (a govet analyzer) is an
        // unknown name that matches nothing.
        self.linters.is_empty() && issue.from_linter != NOLINTLINT_NAME
            || self.linters.iter().any(|name| name == &issue.from_linter)
    }
}

/// Per-file nolint index built from source (re-parsed with comments).
#[derive(Debug, Default, Clone)]
pub struct NolintIndex {
    /// Keys: absolute path, and basename, both normalized with `/`.
    files: HashMap<String, Vec<IgnoredRange>>,
    /// The same comments as seen by nolintlint's own parser, which disagrees
    /// with the filter's on malformed and mixed-case directives.
    directives: HashMap<String, Vec<Directive>>,
    /// `(file, directive line, directive column)` → the linters whose findings
    /// that directive suppressed. Upstream keeps this map on the range and
    /// copies a pointer to the pre-expansion range so both stay in sync; here
    /// the directive's own position is the identity, which every expansion of
    /// it already carries.
    matched: HashMap<(String, i64, i64), HashSet<String>>,
    /// `Some` when nolintlint is enabled, carrying its optional checks.
    style: Option<NolintlintStyle>,
    unknown_linters: HashSet<String>,
    /// Linters enabled for this run. Unused `//nolint:L` is suppressed when `L`
    /// is absent (golangci `nolint_filter` skips disabled linters).
    /// Empty = do not apply this filter (tests / callers that omit the set).
    enabled_linters: HashSet<String>,
    /// Absolute paths belonging to packages guff marked `ill_typed`. Analyzers
    /// with `run_despite_errors: false` are skipped there, so their `//nolint`
    /// directives cannot be consumed — do not report them as unused.
    ill_typed_files: HashSet<String>,
}

impl NolintIndex {
    /// Build an index by re-parsing each compiled Go file with comments.
    pub fn from_packages(packages: &[Arc<Package>]) -> Self {
        Self::build(packages, None, None)
    }

    /// Like [`from_packages`], but when nolintlint is off only files
    /// referenced by `issues` are considered. Reporting on the directives
    /// themselves needs every file, whether or not it produced a finding.
    pub fn from_packages_for_issues(
        packages: &[Arc<Package>],
        issues: &[Issue],
        style: Option<&NolintlintStyle>,
    ) -> Self {
        if style.is_some() {
            return Self::build(packages, None, style);
        }
        if issues.is_empty() {
            // Empty issues + nolintlint off → nothing to suppress.
            return Self::default();
        }
        let needed = issue_path_keys(issues);
        Self::build(packages, Some(&needed), None)
    }

    fn build(
        packages: &[Arc<Package>],
        only: Option<&HashSet<String>>,
        style: Option<&NolintlintStyle>,
    ) -> Self {
        let mut index = Self::default();
        index.style = style.cloned();
        for pkg in packages {
            for path in &pkg.compiled_go_files {
                if let Some(needed) = only {
                    if !path_is_needed(path, needed) {
                        continue;
                    }
                }
                if pkg.ill_typed {
                    let path_str = path.to_string_lossy().replace('\\', "/");
                    index.ill_typed_files.insert(path_str);
                }
                index.add_file(path);
            }
        }
        if !index.unknown_linters.is_empty() {
            let mut names: Vec<_> = index.unknown_linters.iter().cloned().collect();
            names.sort();
            eprintln!(
                "guff: found unknown linters in //nolint directives: {}",
                names.join(", ")
            );
        }
        index
    }

    /// Restrict unused-directive reporting to these enabled linter names.
    pub fn set_enabled_linters<I>(&mut self, names: I)
    where
        I: IntoIterator<Item = String>,
    {
        self.enabled_linters = names.into_iter().collect();
    }

    fn add_file(&mut self, path: &Path) {
        let src = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return,
        };
        // Cheap reject: real `//nolint` / `/*nolint` directives always contain
        // this literal. Skipping the full comment-aware parse for the common
        // no-directive case is the bulk of `issues+filter` time on large trees.
        // Precomputed `Finder` is SIMD-accelerated vs naive `windows(6)`.
        static NOLINT_FINDER: OnceLock<Finder<'static>> = OnceLock::new();
        let finder = NOLINT_FINDER.get_or_init(|| Finder::new(b"nolint"));
        if finder.find(&src).is_none() {
            return;
        }
        let path_str = path.to_string_lossy().replace('\\', "/");
        let fset = FileSet::new();
        let file = match parse_file(&fset, &path_str, &src, COMMENTS_ONLY) {
            Ok(f) => f,
            Err(_) => return,
        };

        // Only when nolintlint is enabled: this is a second parse of the same
        // comments, and every finding it feeds — the directive-shape ones and
        // the unused candidates alike — belongs to that linter.
        if self.style.is_some() {
            let directives = extract_directives(&fset, &file.comments);
            if !directives.is_empty() {
                self.directives.insert(path_str.clone(), directives);
            }
        }

        let inline = self.extract_inline_ranges(&fset, &file.comments);
        if inline.is_empty() {
            return;
        }
        let expanded = expand_ranges(&fset, &file, &inline);
        let mut all = inline;
        all.extend(expanded);

        self.files.insert(path_str, all);
        // Basename is resolved in lookup_mut — do not store a second clone,
        // or suppress() via basename would mark a duplicate while
        // collect_unused() still sees the absolute entry as unused
        // (formatters often hit basename fallback).
    }

    fn extract_inline_ranges(
        &mut self,
        fset: &FileSet,
        comments: &[guff::ast::CommentGroup],
    ) -> Vec<IgnoredRange> {
        let pattern = nolint_pattern();
        let mut out = Vec::new();
        for g in comments {
            for c in &g.list {
                if let Some(ir) = extract_range(c, g, fset, pattern, &mut self.unknown_linters) {
                    out.push(ir);
                }
            }
        }
        out
    }

    /// Record which directives would suppress `issues` without dropping them.
    ///
    /// Used so that findings later removed by exclusion presets still count as
    /// "using" a `//nolint` (golangci analysis-level nolint parity).
    pub fn mark_matches(&mut self, issues: &[Issue]) {
        for issue in issues {
            if issue.from_linter == NOLINTLINT_NAME {
                continue;
            }
            let _ = self.suppress(issue);
        }
    }

    /// Drop issues covered by a nolint directive. Records matched linters.
    ///
    /// When `report_unused`, unused directives become `nolintlint` issues.
    ///
    /// Every nolintlint finding leaves here **behind** every other linter's,
    /// and that ordering is observable: `issues.uniq-by-line` is on by default
    /// and keeps whichever finding on a line arrived first, so a directive that
    /// shares its line with a real finding must lose. Upstream gets the same
    /// answer from `linter.LastLinter` — nolintlint runs last because it reads
    /// the other linters' results — which `combineGoAnalysisLinters` sorts
    /// behind every other linter whatever its name. A plain name sort would put
    /// it ahead of `revive`, `staticcheck` and `unused`.
    /// Gated by `compat/golden/cases/issues-uniq-by-line-order`.
    pub fn filter_issues(&mut self, issues: Vec<Issue>, report_unused: bool) -> Vec<Issue> {
        let mut normal = Vec::new();
        let mut nolintlint_issues = Vec::new();
        for issue in issues {
            if issue.from_linter == NOLINTLINT_NAME {
                nolintlint_issues.push(issue);
            } else {
                normal.push(issue);
            }
        }

        let mut kept = Vec::new();
        for issue in normal {
            if self.suppress(&issue) {
                continue;
            }
            kept.push(issue);
        }

        for issue in nolintlint_issues {
            if self.suppress(&issue) {
                continue;
            }
            kept.push(issue);
        }

        if self.style.is_some() {
            // These are ordinary nolintlint findings, so a `//nolint` that
            // names nolintlint suppresses them like any other issue.
            for issue in self.style_issues() {
                if !self.suppress(&issue) {
                    kept.push(issue);
                }
            }
        }
        if report_unused {
            for issue in self.collect_unused() {
                if !self.suppress(&issue) {
                    kept.push(issue);
                }
            }
        }
        kept
    }

    /// golangci `shouldPassIssue`: the **first** matching range wins and gets
    /// the credit; the rest are not consulted.
    fn suppress(&mut self, issue: &Issue) -> bool {
        let Some(key) = self.resolve_key(&issue.filename) else {
            return false;
        };
        let Some(ranges) = self.files.get(&key) else {
            return false;
        };
        let Some(hit) = ranges.iter().find(|ir| ir.does_match(issue)) else {
            return false;
        };
        let pos = (key, hit.report_line, hit.report_col);
        self.matched
            .entry(pos)
            .or_default()
            .insert(issue.from_linter.clone());
        true
    }

    /// The key `files` / `directives` are stored under.
    ///
    /// The index is keyed by absolute path but issues carry a **module-relative**
    /// one, so this fallback is the common path, not an edge case. It used to
    /// match on the basename alone and take the first key that matched, which is
    /// wrong as soon as two packages hold a file of the same name — and Go trees
    /// are full of `doc.go`, `main.go`, `types.go`. A `//nolint` in one
    /// `bad.go` then suppressed a finding at the same line of a *different*
    /// `bad.go`, silently: the finding simply never appeared.
    ///
    /// Matching the whole relative path as a suffix resolves that case exactly
    /// (`assert/bad.go` matches only one absolute key), and still resolves a
    /// bare basename when it is unique — which is what the fallback was added
    /// for, since formatters report issues by basename. When the suffix is
    /// genuinely ambiguous no key is returned: failing to suppress is visible
    /// to the user and is theirs to fix, while suppressing the wrong file's
    /// finding deletes it with no trace anywhere.
    ///
    /// Found by `compat/fuzz.py` (COMPAT-HARDENING Phase 6) appending a bare
    /// `//nolint` to `defaults/bad.go:9` in `cases/errcheck-asserts`, which made
    /// `assert/bad.go:9` disappear.
    fn resolve_key(&self, filename: &str) -> Option<String> {
        let norm = filename.replace('\\', "/");
        if self.files.contains_key(&norm) {
            return Some(norm);
        }
        // Issues reach the filter with the path the runner handed them, which
        // for a formatter is `./a.go` — the `./` is only stripped later, on the
        // way to output. Matching the raw string as a suffix therefore looks
        // for `/./a.go` and finds nothing, which silently disables *every*
        // suppression on that file. Strip the prefix before matching.
        let mut rel = norm.as_str();
        while let Some(stripped) = rel.strip_prefix("./") {
            rel = stripped;
        }
        if let Some(k) = self.files.get_key_value(rel).map(|(k, _)| k.clone()) {
            return Some(k);
        }
        let suffix = format!("/{rel}");
        let mut hits = self.files.keys().filter(|k| k.ends_with(&suffix));
        let first = hits.next()?;
        if hits.next().is_some() {
            return None; // ambiguous — see above
        }
        Some(first.clone())
    }

    /// The `nolintlint` findings that do not depend on what was suppressed:
    /// a leading space, a malformed directive, and — under their settings —
    /// a missing linter list or a missing explanation.
    fn style_issues(&self) -> Vec<Issue> {
        let Some(style) = self.style.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (filename, directives) in &self.directives {
            for d in directives {
                for text in nolintlint::messages(d, style) {
                    out.push(nolintlint_issue(filename, d.line, d.col, text));
                }
            }
        }
        out
    }

    /// The unused-directive candidates, settled against what each directive
    /// actually suppressed.
    ///
    /// The names come from nolintlint's parse, not the filter's, so they are
    /// spelled as the user wrote them. That is what makes the enabled-linter
    /// test below case-sensitive upstream: `//nolint:ErrCheck` produces a
    /// candidate for a linter called `ErrCheck`, which is not enabled under
    /// that name, so it is dropped rather than reported.
    fn collect_unused(&self) -> Vec<Issue> {
        let mut out = Vec::new();
        for (filename, directives) in &self.directives {
            // Absolute paths only (basename aliases are not stored).
            if !filename.contains('/') && !filename.contains('\\') {
                continue;
            }
            for d in directives {
                // A malformed directive is reported as malformed and nothing
                // else: upstream `continue`s before the unused block.
                if d.malformed {
                    continue;
                }
                if d.linters.is_empty() {
                    if !self.unused_is_cancelled(filename, d.line, None) {
                        out.push(unused_issue(filename, d.line, d.col, &d.text, None));
                    }
                    continue;
                }
                let matched = self.matched.get(&(filename.clone(), d.line, d.col));
                for lint in &d.linters {
                    // Don't report unused for linters we cannot run yet —
                    // golangci would have consumed these directives.
                    if KNOWN_UNIMPLEMENTED_LINTERS.contains(&lint.as_str()) {
                        continue;
                    }
                    // golangci: don't expect disabled linters to cover their
                    // nolint statements (nolint_filter shouldPassIssue).
                    if !self.enabled_linters.is_empty()
                        && !self.enabled_linters.contains(lint.as_str())
                    {
                        continue;
                    }
                    // guff may mark a package ill_typed while go/types is
                    // clean (e.g. gin). Analyzers that refuse ill_typed
                    // packages never see those files, so their directives
                    // stay unmatched — that is not an unused-nolintlint hit.
                    if self.ill_typed_files.contains(filename)
                        && linter_skipped_on_ill_typed(&self.enabled_linters, lint)
                    {
                        continue;
                    }
                    if matched.is_some_and(|m| m.contains(lint.as_str()))
                        || self.unused_is_cancelled(filename, d.line, Some(lint))
                    {
                        continue;
                    }
                    out.push(unused_issue(filename, d.line, d.col, &d.text, Some(lint)));
                }
            }
        }
        out
    }

    /// `doesMatch`'s third arm, which asks a wider question than it looks like.
    ///
    /// nolintlint does not decide that a directive is unused: it emits a
    /// *candidate* for every directive and the nolint filter cancels the ones
    /// that turn out to be used. Cancelling runs the candidate through the same
    /// range loop as any other issue, so **any** range covering the candidate's
    /// line can cancel it — not only the range the directive itself created:
    ///
    /// ```go
    /// //nolint:errcheck // covers the whole file   <- suppresses line 4
    /// package p
    ///
    /// func A()     { mkerr() }
    /// func B() int { x := 1; x = 2 /*nolint*/; return x }   <- unused, unreported
    /// ```
    ///
    /// The file-level directive's range spans the file and has matched the
    /// errcheck finding, so `len(matchedIssueFromLinter) > 0` holds for it and
    /// the *other* directive's unused candidate is filtered out with it. Take
    /// away the errcheck finding and both directives are reported unused —
    /// which is how this was confirmed rather than assumed.
    ///
    /// Reading it as "did my own directive suppress anything" gives the right
    /// answer whenever there is one directive per range, which is every fixture
    /// anyone writes by hand; a differential fuzzer put a second directive
    /// inside a file-level one and the two readings came apart.
    fn unused_is_cancelled(&self, filename: &str, line: i64, specific: Option<&str>) -> bool {
        let Some(ranges) = self.files.get(filename) else {
            return false;
        };
        ranges.iter().any(|ir| {
            if line < ir.from || line > ir.to {
                return false;
            }
            let matched = self
                .matched
                .get(&(filename.to_string(), ir.report_line, ir.report_col));
            match specific {
                Some(l) => matched.is_some_and(|m| m.contains(l)),
                None => matched.is_some_and(|m| !m.is_empty()),
            }
        })
    }
}

/// True when the runner may skip type-sensitive analyzers for `lint` on
/// ill-typed packages.
///
/// Multi-check linters such as staticcheck mix `run_despite_errors` flags. If
/// *any* analyzer refuses ill-typed packages, unmatched `//nolint:<lint>`
/// directives are not unused — the finding they suppress may simply have been
/// skipped (cobra SA1029, restic QF1001, …).
///
/// `unused` is registered with `run_despite_errors` when `nolintlint` is also
/// enabled (see [`crate::registry::resolve_linters_with_settings`]).
fn linter_skipped_on_ill_typed(enabled: &HashSet<String>, lint: &str) -> bool {
    if lint == "unused" && enabled.contains("nolintlint") {
        return false;
    }
    let Some(analyzers) = analyzers_for_linter(lint) else {
        return false;
    };
    !analyzers.is_empty() && analyzers.iter().any(|a| !a.run_despite_errors)
}

fn unused_issue(
    filename: &str,
    line: i64,
    column: i64,
    comment: &str,
    specific: Option<&str>,
) -> Issue {
    let text = match specific {
        Some(l) => format!("directive `{comment}` is unused for linter {l:?}"),
        None => format!("directive `{comment}` is unused"),
    };
    nolintlint_issue(filename, line, column, text)
}

fn nolintlint_issue(filename: &str, line: i64, column: i64, text: String) -> Issue {
    Issue {
        from_linter: NOLINTLINT_NAME.into(),
        analyzer: NOLINTLINT_NAME.into(),
        text: text.clone(),
        severity: String::new(),
        filename: filename.into(),
        line,
        column,
        source_line: None,
        diagnostic: Diagnostic {
            message: text,
            ..Diagnostic::default()
        },
    }
}

fn issue_path_keys(issues: &[Issue]) -> HashSet<String> {
    let mut keys = HashSet::with_capacity(issues.len() * 2);
    for issue in issues {
        if issue.filename.is_empty() {
            continue;
        }
        let norm = issue.filename.replace('\\', "/");
        if let Some(base) = Path::new(&norm).file_name().and_then(|s| s.to_str()) {
            keys.insert(base.to_string());
        }
        keys.insert(norm);
    }
    keys
}

fn path_is_needed(path: &Path, needed: &HashSet<String>) -> bool {
    let path_str = path.to_string_lossy().replace('\\', "/");
    if needed.contains(path_str.as_str()) {
        return true;
    }
    path.file_name()
        .and_then(|s| s.to_str())
        .is_some_and(|base| needed.contains(base))
}

/// Every `//nolint` comment as nolintlint sees it, in source order.
fn extract_directives(fset: &FileSet, comments: &[guff::ast::CommentGroup]) -> Vec<Directive> {
    let mut out = Vec::new();
    for g in comments {
        for c in &g.list {
            let pos = fset.position(c.pos());
            if let Some(d) = nolintlint::parse(&c.text, pos.line, pos.column) {
                out.push(d);
            }
        }
    }
    out
}

fn nolint_pattern() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^nolint( |:|$)").expect("nolint regex"))
}

fn extract_range(
    comment: &guff::ast::Comment,
    group: &guff::ast::CommentGroup,
    fset: &FileSet,
    pattern: &Regex,
    unknown: &mut HashSet<String>,
) -> Option<IgnoredRange> {
    // `strings.TrimLeft(text, "/ ")`: any run of slashes and *spaces*, in any
    // order. Not `trim_start()` — a tab after `//` is not trimmed upstream, so
    // `//\tnolint` is not a directive.
    let text = comment.text.trim_start_matches(['/', ' ']);
    if !pattern.is_match(text) {
        return None;
    }

    // golangci `extractInlineRangeFromComment` builds the suppression range
    // from the enclosing CommentGroup, so a `//nolint` on the last line of a
    // godoc block covers the whole block — e.g. package docs that spell
    // "cancelled" to match a proto enum silence misspell this way.
    // The directive's own position is kept separately for unused reporting.
    let pos = fset.position(comment.pos());
    let group_pos = fset.position(group.pos());
    let group_end = fset.position(group.end());
    let build = |linters: Vec<String>| IgnoredRange {
        from: group_pos.line,
        to: group_end.line,
        report_line: pos.line,
        report_col: pos.column,
        col: group_pos.column,
        linters,
        comment_text: comment.text.clone(),
        is_expansion: false,
    };

    if text.starts_with("nolint:all") || !text.starts_with("nolint:") {
        return Some(build(Vec::new()));
    }

    // Specific linters: strip trailing `// explanation`.
    let body = text.split("//").next().unwrap_or(text);
    let list = body.strip_prefix("nolint:").unwrap_or("");
    let mut linters = Vec::new();
    for item in list.split(',') {
        let name = item.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        if name == "all" {
            unknown.clear();
            return Some(build(Vec::new()));
        }
        let canonical = normalize_linter_name(&name).to_string();
        if !is_known_nolint_target(&canonical) {
            unknown.insert(canonical.clone());
        }
        linters.push(canonical);
    }
    Some(build(linters))
}

fn is_known_nolint_target(name: &str) -> bool {
    name == NOLINTLINT_NAME
        || analyzers_for_linter(name).is_some()
        // Formatters are not analysis linters but are valid //nolint targets
        // (golangci treats gofumpt/gofmt/… the same as linters for nolint).
        || matches!(
            name,
            "gofmt" | "gofumpt" | "goimports" | "gci" | "golines" | "swaggo"
        )
        // Enabled-in-config but not-yet-implemented linters (e.g. contextcheck)
        // still appear in //nolint; treat them as known so we don't warn.
        || KNOWN_UNIMPLEMENTED_LINTERS.contains(&name)
        || linter_name_for_analyzer(name) != name
}

/// Linters documented as not yet implemented in guff. `//nolint:<name>` must
/// not be reported as unknown; unused-nolintlint also skips them (golangci
/// would have matched real findings we cannot emit yet).
const KNOWN_UNIMPLEMENTED_LINTERS: &[&str] = &[];

/// golangci `rangeExpander.Visit`: a directive on its own line stretches over
/// the node that starts on the next line **in the directive's own column**.
///
/// There is no separate file-level rule. `ast.Walk` visits the `*ast.File`
/// first, and its `Pos()` is the `package` keyword, so a directive on the line
/// directly above `package` at column 1 expands to the end of the file exactly
/// like any other node — and one separated from it by a blank line does not.
fn expand_ranges(
    fset: &FileSet,
    file: &guff::ast::File,
    inline: &[IgnoredRange],
) -> Vec<IgnoredRange> {
    let mut expanded = Vec::new();
    preorder(NodeRef::File(file), |node| {
        let npos = node_pos(node);
        if npos.0 == 0 {
            return true;
        }
        let start = fset.position(npos);
        let end = fset.position(node_end(node));
        for r in inline {
            if r.to != start.line - 1 || r.col != start.column {
                continue;
            }
            let mut er = r.clone();
            er.is_expansion = true;
            if er.to < end.line {
                er.to = end.line;
            }
            expanded.push(er);
            break;
        }
        true
    });
    expanded
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

    /// An index with nolintlint enabled, which is what `filter_issues(_, true)`
    /// means: the unused candidates come from that linter's parse.
    fn nolintlint_index(packages: &[Arc<Package>]) -> NolintIndex {
        NolintIndex::from_packages_for_issues(
            packages,
            &[],
            Some(&NolintlintStyle {
                report_unused: true,
                ..NolintlintStyle::default()
            }),
        )
    }

    fn issue(linter: &str, file: &str, line: i64, text: &str) -> Issue {
        Issue {
            from_linter: linter.into(),
            analyzer: linter.into(),
            text: text.into(),
            severity: String::new(),
            filename: file.into(),
            line,
            column: 1,
            source_line: None,
            diagnostic: Diagnostic {
                message: text.into(),
                ..Diagnostic::default()
            },
        }
    }

    #[test]
    fn package_level_nolint_covers_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "// nolint\npackage p\n\nfunc f() error { return nil }\n\nfunc g() {\n\tf()\n}\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let issues = vec![issue("gosec", path.to_str().unwrap(), 7, "G104")];
        let kept = index.filter_issues(issues, false);
        assert!(kept.is_empty(), "{kept:?}");
    }

    /// A `//nolint` must not reach a same-numbered line in a *different* file.
    ///
    /// The index is keyed by absolute path, issues arrive with a
    /// module-relative one, and the fallback used to take the first key with a
    /// matching basename. Two packages each holding a `bad.go` is ordinary Go,
    /// so the directive in one silently deleted the other's finding.
    /// Found by compat/fuzz.py — COMPAT-HARDENING.md, 13th session.
    #[test]
    fn nolint_does_not_leak_to_a_same_named_file_in_another_dir() {
        let dir = tempfile::tempdir().unwrap();
        let a_dir = dir.path().join("suppressed");
        let b_dir = dir.path().join("reported");
        std::fs::create_dir_all(&a_dir).unwrap();
        std::fs::create_dir_all(&b_dir).unwrap();
        let a = a_dir.join("bad.go");
        let b = b_dir.join("bad.go");
        let body = "package p\n\nfunc f() error { return nil }\n\nfunc g() {\n\tf()";
        std::fs::write(&a, format!("{body} //nolint\n}}\n")).unwrap();
        std::fs::write(&b, format!("{body}\n}}\n")).unwrap();

        let pkg = Package {
            compiled_go_files: vec![a.clone(), b.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        // Both issues sit on line 6, and both files are named bad.go.
        let issues = vec![
            issue("errcheck", "suppressed/bad.go", 6, "unchecked"),
            issue("errcheck", "reported/bad.go", 6, "unchecked"),
        ];
        let kept = index.filter_issues(issues, false);
        let files: Vec<&str> = kept.iter().map(|i| i.filename.as_str()).collect();
        assert_eq!(files, vec!["reported/bad.go"], "kept: {files:?}");
    }

    /// Formatters hand the filter a `./`-prefixed path (the prefix is stripped
    /// on the way to output, not before filtering), so the lookup has to
    /// normalize it. Missing this disabled every suppression on the file —
    /// gin's `//nolint:gofumpt` stopped working, which the OSS tier caught.
    #[test]
    fn dot_slash_prefixed_issue_path_still_resolves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\nfunc f() error { return nil }\n\nfunc g() {\n\tf() //nolint:errcheck\n}\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let kept = index.filter_issues(vec![issue("errcheck", "./f.go", 6, "unchecked")], false);
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn same_line_nolint_suppresses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\nfunc f() error { return nil }\n\nfunc g() {\n\tf() //nolint:errcheck\n}\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let issues = vec![issue("errcheck", path.to_str().unwrap(), 6, "unchecked")];
        let kept = index.filter_issues(issues, false);
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn preceding_nolint_expands_to_func() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\n//nolint:errcheck\nfunc g() {\n\tf()\n}\n\nfunc f() error { return nil }\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let issues = vec![issue("errcheck", path.to_str().unwrap(), 5, "unchecked")];
        let kept = index.filter_issues(issues, false);
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn nolint_covers_the_whole_enclosing_comment_group() {
        // golangci builds the suppression range from the CommentGroup, so a
        // `//nolint` on the last line of a doc comment also covers the prose
        // above it (a real-world case: silencing misspell on package docs
        // that spell "cancelled" to match a proto enum).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "// Package p documents the cancelled status.\n             // Another line mentioning cancelled again.\n             //\n             //nolint:misspell // matches the proto enum.\n             package p\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let file = path.to_str().unwrap();
        let issues = vec![
            issue("misspell", file, 1, "`cancelled` is a misspelling"),
            issue("misspell", file, 2, "`cancelled` is a misspelling"),
        ];
        let kept = index.filter_issues(issues, false);
        assert!(kept.is_empty(), "{kept:?}");
    }

    #[test]
    fn unused_directive_is_reported_on_its_own_line_not_the_group_start() {
        // The suppression range starts at the comment group, but an unused
        // directive must still be reported where the user wrote it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\n             // Doc line one.\n             // Doc line two.\n             //nolint:errcheck // nothing to suppress here.\n             var x int\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = nolintlint_index(&[Arc::new(pkg)]);
        let kept = index.filter_issues(Vec::new(), true);
        let unused: Vec<&Issue> = kept
            .iter()
            .filter(|i| i.from_linter == NOLINTLINT_NAME)
            .collect();
        assert_eq!(unused.len(), 1, "{kept:?}");
        assert_eq!(unused[0].line, 5, "should report the directive line: {kept:?}");
    }

    #[test]
    fn unused_nolint_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(&path, "package p\n\nvar x int //nolint:errcheck\n").unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = nolintlint_index(&[Arc::new(pkg)]);
        let kept = index.filter_issues(Vec::new(), true);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].from_linter, NOLINTLINT_NAME);
        assert!(kept[0].text.contains("unused"), "{}", kept[0].text);
    }

    #[test]
    fn other_linter_not_suppressed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\nfunc g() {\n\tf() //nolint:errcheck\n}\n\nfunc f() error { return nil }\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
        let issues = vec![issue("staticcheck", path.to_str().unwrap(), 4, "SA")];
        let kept = index.filter_issues(issues, false);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn unused_skipped_for_disabled_linter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(&path, "package p\n\nvar x int //nolint:errcheck\n").unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = nolintlint_index(&[Arc::new(pkg)]);
        index.set_enabled_linters(["nolintlint".into(), "gosec".into()]);
        let kept = index.filter_issues(Vec::new(), true);
        assert!(
            kept.is_empty(),
            "expected no unused for disabled errcheck, got {kept:?}"
        );
    }

    #[test]
    fn unused_skipped_for_ill_typed_type_sensitive_linter() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(&path, "package p\n\nvar x int //nolint:ineffassign\n").unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ill_typed: true,
            ..Package::default()
        };
        let mut index = nolintlint_index(&[Arc::new(pkg)]);
        index.set_enabled_linters(["nolintlint".into(), "ineffassign".into()]);
        let kept = index.filter_issues(Vec::new(), true);
        assert!(
            kept.is_empty(),
            "expected no unused for skipped-on-ill_typed ineffassign, got {kept:?}"
        );
    }

    #[test]
    fn unused_skipped_for_ill_typed_staticcheck_despite_mixed_flags() {
        // staticcheck mixes run_despite_errors; unmatched //nolint:staticcheck on
        // ill-typed packages must not be reported as unused (cobra SA1029).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(
            &path,
            "package p\n\nfunc F() {\n\t_ = 1 //nolint:staticcheck\n}\n",
        )
        .unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ill_typed: true,
            ..Package::default()
        };
        let mut index = nolintlint_index(&[Arc::new(pkg)]);
        index.set_enabled_linters(["nolintlint".into(), "staticcheck".into()]);
        let kept = index.filter_issues(Vec::new(), true);
        assert!(
            kept.is_empty(),
            "expected no unused for staticcheck on ill_typed, got {kept:?}"
        );
    }
}
