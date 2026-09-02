//! The `typecheck` pseudo linter — golangci-lint's
//! `pkg/goanalysis/pkgerrors` plus the four lines of `InvalidIssue` that make
//! it override everything else.
//!
//! It is not a linter and there is no analyzer behind it: the findings are the
//! *package loader's* errors, rendered as issues. golangci-lint reports them
//! whatever the enabled set is, because a package that does not build makes
//! every other answer unreliable.
//!
//! # Two halves, and only one of them is here
//!
//! `packages.Error` carries both what `go list` said about a package and what
//! the type checker said about its source. This module emits the **first** —
//! [`ErrorKind::List`] — and nothing else. That is a measured boundary, not a
//! convenience:
//!
//! - a `go list` error's text is `go list`'s own string, byte for byte in both
//!   tools (`pattern app/dist: no matching files found`), so it can only match;
//! - a type error carries *guff's* wording, and the 2026-08-11 measurement had
//!   2 of 9 shapes differing from `go build`. Emitting those would be guessing
//!   at upstream's text.
//!
//! And the cost of guessing here is not one wrong finding. Because a typecheck
//! issue **deletes every other issue in the run** (see
//! [`keep_only_typecheck`]), one spurious one empties the whole report.
//!
//! On the corpus the second half is currently unobservable anyway: across four
//! targets and 771 packages, `go list -e` reported exactly one error, and
//! guff's own ill-typed set is empty everywhere (`compat/baselines/health.json`
//! has no rows, and an absent row means strictly zero).
//!
//! # Why this is inert on a default run today
//!
//! guff's package lister has been **native** by default since 2026-07-31
//! (`NativeListMode::from_env` answers `On` when `GUFF_NATIVE_LIST` is unset),
//! and `guff-golist` does not look at `//go:embed` at all — the word does not
//! appear in the crate. So the one error class this module emits is never
//! produced under the default lister, and `issues_from_package_errors` returns
//! an empty vector on every corpus target.
//!
//! With `GUFF_NATIVE_LIST=0`, which routes the load back through `go list`, the
//! two tools agree on all eight measured shapes — including the ones where the
//! typecheck issue deletes another linter's finding. That is what pins this
//! code: the emitter and the override are right, and what is missing is one
//! named thing, embed-pattern resolution in the native lister.
//!
//! One detail for whoever writes it: `go list -e -json=<fields>` only *computes*
//! the error when **`EmbedFiles`** is among the requested fields.
//! `EmbedPatterns` alone is not enough — measured with four field lists — and
//! asking for it costs nothing (prometheus `./...`, 0.32s vs 0.21s, inside the
//! noise). guff's own field list already asks for both.
//!
//! # What a `./...` run makes unnecessary
//!
//! Upstream's `extractErrorsImpl` recurses into a package's imports when the
//! package itself has no errors, which is how `d` importing a broken `a` gets
//! `could not import example.com/tc/a (a/a.go:5:12: …)`. Measured: that
//! finding appears when **only** `d` is linted, and disappears when `a` is in
//! the same run — `stackCrusher` reduces it to the inner message, which `a`'s
//! own error already claimed. Every corpus target is linted as `./...`, so the
//! broken package is always in the set and the direct error is always the one
//! that survives.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use guff_packages::{ErrorKind, Package};

use crate::exclude::Issue;

/// `parseErrorPosition`: `file:line[:column]`, splitting on `:` from the left.
///
/// `go list` reports the file relative to the directory it ran in, so the
/// result is joined onto `base` to match every other issue, which is absolute
/// at this point in the pipeline.
fn parse_position(pos: &str, base: &Path) -> Option<(String, i64, i64)> {
    let mut parts = pos.split(':');
    let file = parts.next()?;
    if file.is_empty() {
        return None;
    }
    let line: i64 = parts.next()?.parse().ok()?;
    // Upstream reads a column only when there are exactly three fields; a
    // fourth means this is not a position it understands.
    let column = match (parts.next(), parts.next()) {
        (Some(c), None) => c.parse().ok()?,
        (None, None) => 0,
        _ => return None,
    };
    let path = Path::new(file);
    let filename = if path.is_absolute() {
        file.to_string()
    } else {
        base.join(path).to_string_lossy().into_owned()
    };
    Some((filename, line, column))
}

/// One issue per package that failed to load, deduplicated on
/// `(file, line, column, text)` the way `BuildIssuesFromIllTypedError` does.
pub fn issues_from_package_errors(packages: &[Arc<Package>], base: &Path) -> Vec<Issue> {
    let mut seen: HashSet<(String, i64, i64, String)> = HashSet::new();
    let mut out = Vec::new();
    for pkg in packages {
        for err in &pkg.errors {
            if err.kind != ErrorKind::List {
                continue;
            }
            let Some((filename, line, column)) = parse_position(&err.pos, base) else {
                // Upstream logs an unparseable position and drops the issue.
                continue;
            };
            let key = (filename.clone(), line, column, err.msg.clone());
            if !seen.insert(key) {
                continue;
            }
            out.push(Issue {
                from_linter: "typecheck".into(),
                analyzer: "typecheck".into(),
                text: err.msg.clone(),
                severity: String::new(),
                filename,
                line,
                column,
                source_line: None,
                diagnostic: guff_analysis::Diagnostic::default(),
            });
        }
    }
    out
}

/// `InvalidIssue`, whose first four lines are the whole rule:
///
/// ```go
/// tcIssues := filterIssuesUnsafe(issues, func(i *result.Issue) bool {
///     return i.FromLinter == typeCheckName
/// })
/// if len(tcIssues) > 0 { return tcIssues, nil }
/// ```
///
/// One package that does not build silences **every** linter finding in the
/// run, including findings in packages that build fine and were never near it.
/// Measured: linting `./b/... ./c/...` where only `b` has a bad `//go:embed`
/// prints `b`'s typecheck error and drops `c`'s errorlint finding entirely.
///
/// It sits fourth in upstream's processor list — after `Cgo`, before every
/// exclusion — so the decision is made on the raw issue list and an
/// `exclude-rules` entry cannot bring the other findings back.
pub fn keep_only_typecheck(issues: &mut Vec<Issue>) {
    if issues.iter().any(|i| i.from_linter == "typecheck") {
        issues.retain(|i| i.from_linter == "typecheck");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_position_takes_two_or_three_fields() {
        let base = Path::new("/m");
        assert_eq!(
            parse_position("ui/web.go:31:12", base),
            Some(("/m/ui/web.go".into(), 31, 12))
        );
        // A position with no column is upstream's `len(parts) == 2` case.
        assert_eq!(
            parse_position("ui/web.go:31", base),
            Some(("/m/ui/web.go".into(), 31, 0))
        );
        // An absolute path is left alone.
        assert_eq!(
            parse_position("/abs/x.go:1:2", base),
            Some(("/abs/x.go".into(), 1, 2))
        );
        // "no colons", an unparseable line, and a fourth field are all dropped.
        assert_eq!(parse_position("ui/web.go", base), None);
        assert_eq!(parse_position("ui/web.go:x", base), None);
        assert_eq!(parse_position("ui/web.go:1:2:3", base), None);
    }

    #[test]
    fn one_typecheck_issue_deletes_the_rest_of_the_run() {
        let mk = |linter: &str| Issue {
            from_linter: linter.into(),
            analyzer: linter.into(),
            text: "x".into(),
            severity: String::new(),
            filename: "a.go".into(),
            line: 1,
            column: 1,
            source_line: None,
            diagnostic: guff_analysis::Diagnostic::default(),
        };

        let mut only_linters = vec![mk("errcheck"), mk("govet")];
        keep_only_typecheck(&mut only_linters);
        assert_eq!(only_linters.len(), 2, "nothing to override");

        let mut mixed = vec![mk("errcheck"), mk("typecheck"), mk("govet")];
        keep_only_typecheck(&mut mixed);
        assert_eq!(
            mixed.iter().map(|i| i.from_linter.as_str()).collect::<Vec<_>>(),
            vec!["typecheck"],
        );
    }
}
