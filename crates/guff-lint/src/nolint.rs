//! `//nolint` directive parsing and issue filtering (golangci `nolint_filter`).
//!
//! Inline and preceding-line directives suppress matching issues. When
//! `report_unused` is set, unused directives are emitted as `nolintlint`
//! findings (subset of golangci's `NeedsUnused`).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use guff::parser::{parse_file, PARSE_COMMENTS};
use guff::position::{FileSet, Pos};
use guff::walk::{preorder, NodeRef};
use guff_analysis::Diagnostic;
use guff_packages::Package;
use regex::Regex;

use crate::config::normalize_linter_name;
use crate::exclude::Issue;
use crate::registry::{analyzers_for_linter, linter_name_for_analyzer};

pub const NOLINTLINT_NAME: &str = "nolintlint";

/// One `//nolint` coverage range (possibly AST-expanded).
#[derive(Debug, Clone)]
struct IgnoredRange {
    from: i64,
    to: i64,
    col: i64,
    /// Empty = all linters (except nolintlint itself).
    linters: Vec<String>,
    matched: HashMap<String, bool>,
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
        let mut matched = self.linters.is_empty() && issue.from_linter != NOLINTLINT_NAME;

        for name in &self.linters {
            if name == &issue.from_linter || name == &issue.analyzer {
                matched = true;
                break;
            }
        }

        matched
    }
}

/// Per-file nolint index built from source (re-parsed with comments).
#[derive(Debug, Default, Clone)]
pub struct NolintIndex {
    /// Keys: absolute path, and basename, both normalized with `/`.
    files: HashMap<String, Vec<IgnoredRange>>,
    unknown_linters: HashSet<String>,
}

impl NolintIndex {
    /// Build an index by re-parsing each compiled Go file with comments.
    pub fn from_packages(packages: &[Arc<Package>]) -> Self {
        Self::build(packages, None)
    }

    /// Like [`from_packages`], but when `report_unused` is false only files
    /// referenced by `issues` are considered. Unused-directive reporting still
    /// requires a full scan.
    pub fn from_packages_for_issues(
        packages: &[Arc<Package>],
        issues: &[Issue],
        report_unused: bool,
    ) -> Self {
        if report_unused || issues.is_empty() {
            // Empty issues + no unused reporting → nothing to suppress.
            if !report_unused {
                return Self::default();
            }
            return Self::build(packages, None);
        }
        let needed = issue_path_keys(issues);
        Self::build(packages, Some(&needed))
    }

    fn build(packages: &[Arc<Package>], only: Option<&HashSet<String>>) -> Self {
        let mut index = Self::default();
        for pkg in packages {
            for path in &pkg.compiled_go_files {
                if let Some(needed) = only {
                    if !path_is_needed(path, needed) {
                        continue;
                    }
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

    fn add_file(&mut self, path: &Path) {
        let src = match std::fs::read(path) {
            Ok(b) => b,
            Err(_) => return,
        };
        // Cheap reject: real `//nolint` / `/*nolint` directives always contain
        // this literal. Skipping the full comment-aware parse for the common
        // no-directive case is the bulk of `issues+filter` time on large trees.
        if !src.windows(6).any(|w| w == b"nolint") {
            return;
        }
        let path_str = path.to_string_lossy().replace('\\', "/");
        let fset = FileSet::new();
        let file = match parse_file(&fset, &path_str, &src, PARSE_COMMENTS) {
            Ok(f) => f,
            Err(_) => return,
        };

        let inline = self.extract_inline_ranges(&fset, &file.comments);
        if inline.is_empty() {
            return;
        }
        let expanded = expand_ranges(&fset, &file, &inline);
        let mut all = inline;
        all.extend(expanded);

        self.files.insert(path_str.clone(), all.clone());
        if let Some(base) = path.file_name().and_then(|s| s.to_str()) {
            self.files.insert(base.replace('\\', "/"), all);
        }
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

    /// Drop issues covered by a nolint directive. Records matched linters.
    ///
    /// When `report_unused`, unused directives become `nolintlint` issues.
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

        if report_unused {
            kept.extend(self.collect_unused());
        }
        kept
    }

    fn suppress(&mut self, issue: &Issue) -> bool {
        let Some(ranges) = self.lookup_mut(&issue.filename) else {
            return false;
        };
        for ir in ranges {
            if ir.does_match(issue) {
                ir.matched.insert(issue.from_linter.clone(), true);
                return true;
            }
        }
        false
    }

    fn lookup_mut(&mut self, filename: &str) -> Option<&mut Vec<IgnoredRange>> {
        let norm = filename.replace('\\', "/");
        if self.files.contains_key(&norm) {
            return self.files.get_mut(&norm);
        }
        let base = Path::new(&norm)
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())?;
        self.files.get_mut(&base)
    }

    fn collect_unused(&self) -> Vec<Issue> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (filename, ranges) in &self.files {
            // Prefer absolute keys only once (skip basenames that mirror a longer key).
            if !filename.contains('/') && !filename.contains('\\') {
                let mirrored = self.files.keys().any(|k| {
                    k != filename
                        && Path::new(k)
                            .file_name()
                            .and_then(|s| s.to_str())
                            == Some(filename.as_str())
                });
                if mirrored {
                    continue;
                }
            }
            for ir in ranges {
                if ir.is_expansion {
                    continue;
                }
                let key = (filename.clone(), ir.from, ir.col, ir.comment_text.clone());
                if !seen.insert(key) {
                    continue;
                }
                if ir.linters.is_empty() {
                    if ir.matched.is_empty() {
                        out.push(unused_issue(
                            filename,
                            ir.from,
                            ir.col,
                            &ir.comment_text,
                            None,
                        ));
                    }
                } else {
                    for lint in &ir.linters {
                        if !ir.matched.contains_key(lint) {
                            out.push(unused_issue(
                                filename,
                                ir.from,
                                ir.col,
                                &ir.comment_text,
                                Some(lint),
                            ));
                        }
                    }
                }
            }
        }
        out
    }
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
    let mut text = comment.text.as_str();
    text = text.trim_start_matches('/');
    text = text.trim_start_matches('/');
    text = text.trim_start();
    if !pattern.is_match(text) {
        return None;
    }

    let pos = fset.position(group.pos());
    let end = fset.position(group.end());
    let build = |linters: Vec<String>| IgnoredRange {
        from: pos.line,
        to: end.line,
        col: pos.column,
        linters,
        matched: HashMap::new(),
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
        || linter_name_for_analyzer(name) != name
}

fn expand_ranges(
    fset: &FileSet,
    file: &guff::ast::File,
    inline: &[IgnoredRange],
) -> Vec<IgnoredRange> {
    let mut expanded = Vec::new();
    preorder(NodeRef::File(file), |node| {
        let Some((npos, nend)) = node_span(node) else {
            return true;
        };
        if npos.0 == 0 {
            return true;
        }
        let start = fset.position(npos);
        let end = fset.position(nend);
        for r in inline {
            if r.to == start.line - 1 && start.column == r.col {
                let mut er = r.clone();
                er.is_expansion = true;
                if er.to < end.line {
                    er.to = end.line;
                }
                expanded.push(er);
                break;
            }
        }
        true
    });
    expanded
}

/// Spans for nodes that commonly follow a preceding-line `//nolint`.
fn node_span(n: NodeRef<'_>) -> Option<(Pos, Pos)> {
    match n {
        NodeRef::GenDecl(d) => {
            let end = if d.rparen.is_valid() {
                Pos(d.rparen.0 + 1)
            } else {
                d.specs.first().map(|s| s.end()).unwrap_or_default()
            };
            Some((d.tok_pos, end))
        }
        NodeRef::FuncDecl(d) => {
            let end = d
                .body
                .as_ref()
                .map(|b| b.end())
                .unwrap_or_else(|| d.ty.end());
            Some((d.ty.pos(), end))
        }
        NodeRef::DeclStmt(s) => Some((s.decl.pos(), s.decl.end())),
        NodeRef::ExprStmt(s) => Some((s.x.pos(), s.x.end())),
        NodeRef::AssignStmt(s) => {
            let pos = s.lhs.first().map(|e| e.pos()).unwrap_or_default();
            let end = s.rhs.last().map(|e| e.end()).unwrap_or(pos);
            Some((pos, end))
        }
        NodeRef::ValueSpec(s) => {
            let pos = s.names.first().map(|n| n.pos()).unwrap_or_default();
            let end = if let Some(last) = s.values.last() {
                last.end()
            } else if let Some(t) = &s.ty {
                t.end()
            } else {
                s.names.last().map(|n| n.end()).unwrap_or_default()
            };
            Some((pos, end))
        }
        NodeRef::TypeSpec(s) => Some((s.name.pos(), s.ty.end())),
        NodeRef::ImportSpec(s) => {
            let pos = s
                .name
                .as_ref()
                .map(|n| n.pos())
                .unwrap_or(s.path.value_pos);
            let end = if s.end_pos.0 != 0 {
                s.end_pos
            } else {
                s.path.end()
            };
            Some((pos, end))
        }
        NodeRef::IfStmt(s) => {
            let end = s
                .else_
                .as_ref()
                .map(|e| e.end())
                .unwrap_or_else(|| s.body.end());
            Some((s.if_, end))
        }
        NodeRef::ForStmt(s) => Some((s.for_, s.body.end())),
        NodeRef::RangeStmt(s) => Some((s.for_, s.body.end())),
        NodeRef::SwitchStmt(s) => Some((s.switch, s.body.end())),
        NodeRef::TypeSwitchStmt(s) => Some((s.switch, s.body.end())),
        NodeRef::SelectStmt(s) => Some((s.select_, s.body.end())),
        NodeRef::GoStmt(s) => Some((s.go_, s.call.end())),
        NodeRef::DeferStmt(s) => Some((s.defer_, s.call.end())),
        NodeRef::ReturnStmt(s) => {
            let end = s
                .results
                .last()
                .map(|e| e.end())
                .unwrap_or(Pos(s.return_.0 + 6));
            Some((s.return_, end))
        }
        NodeRef::BlockStmt(s) => Some((s.lbrace, s.end())),
        NodeRef::CallExpr(s) => Some((s.pos(), s.end())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use guff_analysis::Diagnostic;

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
    fn unused_nolint_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.go");
        std::fs::write(&path, "package p\n\nvar x int //nolint:errcheck\n").unwrap();

        let pkg = Package {
            compiled_go_files: vec![path.clone()],
            ..Package::default()
        };
        let mut index = NolintIndex::from_packages(&[Arc::new(pkg)]);
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
}
