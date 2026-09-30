//! SA4019 — multiple identical build constraints in the same file
//!
//! Port of `honnef.co/go/tools/staticcheck/sa4019` (v0.7.0).
//!
//! Upstream reads `astutil.Preamble(f)`: the text of every comment group that
//! starts before the package doc comment — or before `package` when there is
//! none — each through `CommentGroup.Text()`, and keeps the lines that start
//! with `+build `. So:
//!
//! - a `+build` line inside the package doc group, or anywhere after it, is
//!   not a constraint here;
//! - `//+build x` counts like `// +build x` (`Text()` strips `//` and one
//!   space), and so does a `+build x` line of a `/* */` preamble comment;
//! - `//go:build` is a directive `Text()` drops, so the gofmt dual form never
//!   pairs with its `// +build` twin.
//!
//! This used to collect `// +build` from every comment in the file and, when
//! none matched, from every line of the raw source — string literals included.
//! gosec's `testutils/g104_samples.go` holds test programs in raw strings, two
//! of which start with `// +build go1.10`, and that was a finding upstream does
//! not have.

use std::sync::OnceLock;

use guff_analysis::passes::inspect;
use guff_analysis::{AnalysisResult, Analyzer, Pass, RunError, RunFn};

/// `astutil.Preamble`: the preamble comment groups' `Text()`, newline-joined.
fn preamble(file: &guff::ast::File) -> String {
    let cutoff = file.doc.as_ref().map_or(file.package, |d| d.pos());
    let mut out = Vec::new();
    for cg in &file.comments {
        if cg.pos() >= cutoff {
            break;
        }
        out.push(cg.text());
    }
    out.join("\n")
}

/// honnef `buildTags`: the fields of each preamble line after `+build `.
fn build_tags(file: &guff::ast::File) -> Vec<Vec<String>> {
    preamble(file)
        .split('\n')
        .filter_map(|line| line.strip_prefix("+build "))
        .map(|rest| rest.split_whitespace().map(String::from).collect())
        .collect()
}

fn identical(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut sa: Vec<_> = a.iter().collect();
    let mut sb: Vec<_> = b.iter().collect();
    sa.sort();
    sb.sort();
    sa == sb
}

fn run(pass: &mut Pass<'_>) -> Result<Option<AnalysisResult>, RunError> {
    let _inspect = pass
        .result_of::<inspect::InspectResult>(inspect::analyzer())
        .ok_or_else(|| "SA4019 requires inspect analyzer".to_string())?;
    let mut all_pending = Vec::new();
    for file in pass.files() {
        let constraints = build_tags(file);
        for i in 0..constraints.len() {
            for j in (i + 1)..constraints.len() {
                if identical(&constraints[i], &constraints[j]) {
                    let msg = format!(
                        "identical build constraints {:?} and {:?}",
                        constraints[i].join(" "),
                        constraints[j].join(" ")
                    );
                    all_pending.push((file.package.0 as u32, msg));
                }
            }
        }
    }
    for (pos, msg) in all_pending {
        pass.report_unless_generated(pos, msg);
    }
    Ok(None)
}

fn sa4019_analyzer_impl() -> Analyzer {
    Analyzer {
        name: "SA4019",
        doc: "multiple identical build constraints in the same file",
        url: "https://staticcheck.dev/docs/checks/#SA4019",
        run: run as RunFn,
        run_despite_errors: false,
        requires: vec![inspect::analyzer()],
        fact_types: vec![],
    }
}

pub fn analyzer() -> &'static Analyzer {
    static A: OnceLock<Analyzer> = OnceLock::new();
    A.get_or_init(sa4019_analyzer_impl)
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::validate;

    #[test]
    fn sa4019_validates() {
        assert!(validate(&[analyzer()]).is_ok());
    }

    fn tags(src: &str) -> Vec<Vec<String>> {
        let fset = guff::position::FileSet::new();
        let file = guff::parser::parse_file(&fset, "a.go", src.as_bytes(), guff::parser::Mode::NONE)
            .expect("parse");
        build_tags(&file)
    }

    /// The twelve shapes of the golden case, as the preamble reads them:
    /// measured against golangci-lint 2.12.2 (staticcheck v0.7.0).
    #[test]
    fn only_the_preamble_holds_build_constraints() {
        let g = || vec!["go1.10".to_string()];
        // Two lines, one group or two.
        assert_eq!(tags("// +build go1.10\n// +build go1.10\n\npackage p\n"), vec![g(), g()]);
        assert_eq!(tags("// +build go1.10\n\n// +build go1.10\n\npackage p\n"), vec![g(), g()]);
        // No space after `//`, and a block comment: both still `+build` lines.
        assert_eq!(tags("//+build go1.10\n//+build go1.10\n\npackage p\n"), vec![g(), g()]);
        assert_eq!(tags("/*\n+build go1.10\n+build go1.10\n*/\n\npackage p\n"), vec![g(), g()]);
        // The dual form: `//go:build` is a directive `Text()` drops.
        assert_eq!(tags("//go:build go1.10\n// +build go1.10\n\npackage p\n"), vec![g()]);
        // Not the preamble: a string literal, the body, the package doc.
        assert!(tags("package p\n\nvar s = `\n// +build go1.10\n// +build go1.10\n`\n").is_empty());
        assert_eq!(tags("// +build go1.10\n\npackage p\n\n// +build go1.10\nvar x int\n"), vec![g()]);
        assert!(tags("// +build go1.10\n// +build go1.10\npackage p\n").is_empty());
        assert_eq!(
            tags("// +build go1.10\n\n// +build go1.10\n// Package p is doc.\npackage p\n"),
            vec![g()]
        );
    }

    #[test]
    fn identical_ignores_order() {
        let a = vec!["go1.10".to_string(), "!nothing".to_string()];
        let b = vec!["!nothing".to_string(), "go1.10".to_string()];
        assert!(identical(&a, &b));
        assert!(!identical(&a, &a[..1]));
    }
}
